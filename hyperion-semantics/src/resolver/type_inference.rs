// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type inference methods for ResolverBuilder
//!
//! This module contains var type inference, method resolution, and type extraction logic.
//! Extracted from resolver.rs to keep files under 2k lines.

use super::builder::ResolverBuilder;
use crate::symbol::{ClassType, PrimitiveType, ScopeId, SymbolId, Type};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_types::{
    enclosing_type_parameter_declarations, enclosing_type_parameter_names,
    extract_and_resolve_type_with_lookup, extract_and_resolve_type_without_lookup,
    ResolutionContext, TypeStructure,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tracing::{debug, info};

/// Per-builder memo of `symbol_exists`: one file spells `List`/`Integer` hundreds
/// of times, and each workspace lookup elects a definition across the index.
struct CachedLookup<'a> {
    workspace: &'a hyperion_workspace::WorkspaceIndex,
    cache: &'a std::cell::RefCell<HashMap<String, bool>>,
}

impl hyperion_types::SymbolLookup for CachedLookup<'_> {
    fn symbol_exists(&self, fqn: &str) -> bool {
        if let Some(known) = self.cache.borrow().get(fqn) {
            return *known;
        }
        let exists = self.workspace.symbol_exists(fqn);
        self.cache.borrow_mut().insert(fqn.to_owned(), exists);
        exists
    }
}

impl ResolverBuilder {
    /// Infer the type of a var declaration
    ///
    /// This is public to allow LSP features like inlay hints to use it
    ///
    /// PERFORMANCE: Caches results by TextRange to avoid re-inferring same var
    /// (20-30% speedup on files with many var declarations)
    pub fn infer_var_type(&self, decl_node: &SyntaxNode) -> Type {
        // Check cache first - avoid expensive re-inference
        let range = decl_node.text_range();
        if let Some(cached_type) = self.type_cache.borrow().get(&range) {
            debug!("Cache hit for var type at range {:?}", range);
            return cached_type.clone();
        }

        info!("Starting var type inference for range {:?}", range);

        // Helper closure to cache result before returning
        let cache_and_return = |typ: Type| -> Type {
            self.type_cache.borrow_mut().insert(range, typ.clone());
            typ
        };

        // Find the initializer expression
        // LOCAL_VAR_DECL structure varies:
        // Simple case: [VAR_TYPE, <initializer>]
        // Ternary case: [VAR_TYPE, <condition_expr>, TERNARY_EXPR]
        // Array access: [VAR_TYPE, <array_expr>, ARRAY_ACCESS_EXPR]
        //
        // For array access, we need special handling because ARRAY_ACCESS_EXPR
        // only contains the index, not the array
        let children: Vec<_> = decl_node.children().collect();

        if children.len() >= 3
            && children[children.len() - 1].kind() == SyntaxKind::ARRAY_ACCESS_EXPR
        {
            debug!("Detected pattern: array access (var x = array[i])");
            // Special case: array access
            // Children: [VAR_TYPE, array_expr, ARRAY_ACCESS_EXPR]
            // We need to get the element type from array_expr
            //
            // Array parameters ARE correctly stored as Type::Array in the symbol table
            // (verified by comprehensive test suite in tests/array_parameter_tests.rs)
            // This code correctly extracts the element type from the array wrapper.
            let array_type = self.infer_expression_type(&children[children.len() - 2]);
            return if let Type::Array { element_type, .. } = array_type {
                info!("Var type inferred from array access: {:?}", element_type);
                cache_and_return(*element_type)
            } else {
                debug!("Failed to extract array element type");
                cache_and_return(Type::Unknown)
            };
        }

        // Special case: Qualified method call (receiver.method())
        // Structure: NAME_EXPR + DOT token + IDENT token + METHOD_CALL_EXPR
        // Example: var user = service.getUser();
        //
        // NOTE: For method chains (builder pattern), there will be multiple METHOD_CALL_EXPRs
        // We should only use this special case for single method calls
        let method_call_count = children
            .iter()
            .filter(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR)
            .count();

        if method_call_count > 1 {
            // Multiple method calls (chains).
            // First try inferring from the outermost METHOD_CALL_EXPR directly; this is
            // more robust for nested call arguments like collect(Collectors.toList()).
            if let Some(last_child) = children.last() {
                if last_child.kind() == SyntaxKind::METHOD_CALL_EXPR {
                    let inferred_type = self.infer_expression_type(last_child);
                    if !matches!(inferred_type, Type::Unknown) {
                        info!(
                            "Var type inferred from chained outer method call: {:?}",
                            inferred_type
                        );
                        return cache_and_return(inferred_type);
                    }
                }
            }

            // Fallback to explicit token-walk chain resolution for legacy/builder patterns.
            return cache_and_return(self.infer_var_from_method_chain(decl_node));
        }

        if children.len() >= 2
            && method_call_count == 1
            && children[children.len() - 1].kind() == SyntaxKind::METHOD_CALL_EXPR
        {
            // Parse method call initializers directly from the METHOD_CALL_EXPR.
            // This works for both:
            // - Current AST: METHOD_CALL_EXPR(NAME_EXPR, ARG_LIST)
            // - Legacy AST fallbacks handled inside infer_expression_type().
            let method_call_node = &children[children.len() - 1];
            let inferred_type = self.infer_expression_type(method_call_node);
            if !matches!(inferred_type, Type::Unknown) {
                info!("Var type inferred from method call: {:?}", inferred_type);
                return cache_and_return(inferred_type);
            }

            debug!("Failed to resolve method call initializer type");
            return cache_and_return(Type::Unknown);
        }

        if children.len() >= 2 {
            debug!("Detected pattern: direct initializer expression");
            // The initializer is the last child
            let initializer = &children[children.len() - 1];
            let inferred_type = self.infer_expression_type(initializer);
            info!("Var type inferred from initializer: {:?}", inferred_type);
            return cache_and_return(inferred_type);
        }

        debug!("No valid var initializer pattern found");
        cache_and_return(Type::Unknown)
    }

    /// Infer var type from a method chain (builder pattern)
    /// Example: var user = User.builder().name("John").age(30).build();
    pub(crate) fn infer_var_from_method_chain(&self, decl_node: &SyntaxNode) -> Type {
        info!("Resolving method chain for var inference");
        use rowan::NodeOrToken;

        let all_items: Vec<_> = decl_node.children_with_tokens().collect();

        // Start with the initial receiver (e.g., "User" in User.builder())
        let mut current_type = Type::Unknown;
        let mut i = 0;

        // Skip VAR_TYPE, IDENT, EQ, and whitespace until we find the first NAME_EXPR or receiver
        while i < all_items.len() {
            if let NodeOrToken::Node(node) = &all_items[i] {
                if node.kind() == SyntaxKind::NAME_EXPR {
                    // This is the initial receiver
                    // Check if it's a class name (static method call) or a variable
                    let receiver_text = node.text().to_string();
                    if receiver_text.chars().next().is_some_and(char::is_uppercase) {
                        // Class name - static method call
                        current_type = Type::Class(ClassType {
                            name: receiver_text,
                            package: None,
                            type_arguments: vec![],
                            is_diamond: false,
                        });
                    } else {
                        debug!("Chain starts with variable: {}", receiver_text);
                        // Variable - instance method call
                        current_type = self.infer_expression_type(node);
                    }
                    i += 1;
                    break;
                }
            }
            i += 1;
        }

        // Now walk through the chain: DOT + IDENT + METHOD_CALL_EXPR
        while i < all_items.len() {
            // Look for DOT
            if let NodeOrToken::Token(tok) = &all_items[i] {
                if tok.kind() == SyntaxKind::DOT && i + 2 < all_items.len() {
                    // Next should be IDENT (method name)
                    if let NodeOrToken::Token(ident_tok) = &all_items[i + 1] {
                        if ident_tok.kind() == SyntaxKind::IDENT {
                            let method_name = ident_tok.text().to_string();

                            // Check for METHOD_CALL_EXPR after the IDENT (might have whitespace in between)
                            let mut method_call_pos = i + 2;
                            while method_call_pos < all_items.len() {
                                if let NodeOrToken::Token(t) = &all_items[method_call_pos] {
                                    if t.kind() == SyntaxKind::WHITESPACE {
                                        method_call_pos += 1;
                                        continue;
                                    }
                                    break;
                                }
                                break;
                            }

                            if method_call_pos < all_items.len() {
                                if let NodeOrToken::Node(node) = &all_items[method_call_pos] {
                                    if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                                        // Resolve this method call, passing the node for type parameter inference
                                        if let Some(return_type) = self.resolve_method_return_type(
                                            &current_type,
                                            &method_name,
                                            Some(node),
                                        ) {
                                            current_type = return_type;
                                        } else {
                                            debug!(
                                                "Failed to resolve method '{}' in chain",
                                                method_name
                                            );
                                            return Type::Unknown;
                                        }

                                        i = method_call_pos + 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            i += 1;
        }

        info!("Method chain resolved to type: {:?}", current_type);
        current_type
    }

    /// Infer the type of an expression node for var type inference
    pub(crate) fn infer_expression_type(&self, expr_node: &SyntaxNode) -> Type {
        // Check cache first to avoid O(n²) behavior
        let range = expr_node.text_range();
        if let Some(cached_type) = self.type_cache.borrow().get(&range) {
            return cached_type.clone();
        }

        // Compute the type
        let inferred_type = match expr_node.kind() {
            // LITERAL_EXPR: Wrapper node for literals (int, string, etc.)
            SyntaxKind::LITERAL_EXPR => {
                // Descend into children to find the actual literal token
                let mut result = Type::Unknown;
                for child in expr_node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        result = match token.kind() {
                            SyntaxKind::INT_LITERAL => Type::Primitive(PrimitiveType::Int),
                            SyntaxKind::LONG_LITERAL => Type::Primitive(PrimitiveType::Long),
                            SyntaxKind::FLOAT_LITERAL => Type::Primitive(PrimitiveType::Float),
                            SyntaxKind::DOUBLE_LITERAL => Type::Primitive(PrimitiveType::Double),
                            SyntaxKind::CHAR_LITERAL => Type::Primitive(PrimitiveType::Char),
                            SyntaxKind::BOOLEAN_LITERAL => Type::Primitive(PrimitiveType::Boolean),
                            SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => {
                                Type::Class(ClassType {
                                    name: "String".into(),
                                    package: Some("java.lang".into()),
                                    type_arguments: vec![],
                                    is_diamond: false,
                                })
                            }
                            _ => continue,
                        };
                        break;
                    }
                }
                result
            }

            // NEW_EXPR: new ClassName()
            SyntaxKind::NEW_EXPR => {
                // Extract the type being instantiated
                let mut result = Type::Unknown;
                for child in expr_node.children() {
                    match child.kind() {
                        SyntaxKind::CLASS_TYPE => {
                            result = self.extract_class_type(&child);
                            break;
                        }
                        SyntaxKind::PRIMITIVE_TYPE => {
                            result = self.extract_primitive_type(&child).unwrap_or(Type::Unknown);
                            break;
                        }
                        SyntaxKind::ARRAY_TYPE => {
                            result = self.extract_array_type(&child).unwrap_or(Type::Unknown);
                            break;
                        }
                        _ => {}
                    }
                }

                // In `new int[10][]`, sized dimensions remain direct tokens of
                // NEW_EXPR rather than part of the parsed type node.
                let explicit_dimensions = expr_node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| token.kind() == SyntaxKind::L_BRACKET)
                    .count()
                    + expr_node
                        .children()
                        .filter(|child| child.kind() == SyntaxKind::ARRAY_TYPE)
                        .count();
                if explicit_dimensions > 0 && !matches!(result, Type::Unknown) {
                    let dimensions = u8::try_from(explicit_dimensions).unwrap_or(u8::MAX);
                    result = match result {
                        Type::Array {
                            element_type,
                            dimensions: parsed_dimensions,
                        } => Type::Array {
                            element_type,
                            dimensions: parsed_dimensions.saturating_add(dimensions),
                        },
                        element_type => Type::Array {
                            element_type: Box::new(element_type),
                            dimensions,
                        },
                    };
                }
                // Diamond operator: infer type arguments from the declaration's LHS type
                if let Type::Class(ref ct) = result {
                    if ct.is_diamond {
                        if let Some(Type::Class(target_ct)) =
                            self.resolve_diamond_target_type(expr_node)
                        {
                            result = Type::Class(ClassType {
                                type_arguments: target_ct.type_arguments,
                                is_diamond: false,
                                ..ct.clone()
                            });
                        }
                    }
                }
                result
            }

            // NAME_EXPR: Simple variable reference
            SyntaxKind::NAME_EXPR => {
                let mut result = Type::Unknown;
                // Extract the variable name
                if let Some(var_name) = self.extract_identifier(expr_node) {
                    // Find the scope at this node's offset
                    let offset = u32::from(expr_node.text_range().start());
                    let symbol_table = self.resolver.symbol_table();
                    if let Some(scope_id) = symbol_table.find_scope_at_offset(offset) {
                        // Look up the symbol in that scope and parent scopes
                        if let Some(symbol_id) = self.lookup_symbol_in_scope(&var_name, scope_id) {
                            if let Some(symbol) = symbol_table.get_symbol(symbol_id) {
                                result = symbol.type_info.clone();
                            }
                        }
                    }
                }
                result
            }

            // THIS_EXPR: current enclosing class instance
            SyntaxKind::THIS_EXPR => self.infer_enclosing_class_type(expr_node),

            // SUPER_EXPR: parent class instance (falls back to current class if parent unknown)
            SyntaxKind::SUPER_EXPR => {
                let this_type = self.infer_enclosing_class_type(expr_node);
                if let Type::Class(ref ct) = this_type {
                    if let Some(workspace) = self.workspace_index.as_ref() {
                        let this_fqn = ct
                            .package
                            .as_ref()
                            .map_or_else(|| ct.name.clone(), |pkg| format!("{pkg}.{}", ct.name));
                        if let Some(parent_fqn) = workspace.get_parent_class(&this_fqn) {
                            if let Some(parent_type) = self.parse_type_from_string(&parent_fqn) {
                                return parent_type;
                            }
                        }
                    }
                }
                this_type
            }

            // FIELD_ACCESS_EXPR: object.field or object.method (when followed by METHOD_CALL_EXPR)
            SyntaxKind::FIELD_ACCESS_EXPR => {
                // Check if this is a method call by looking for METHOD_CALL_EXPR sibling
                let parent = expr_node.parent();

                let is_method_call = parent
                    .and_then(|p| {
                        p.children()
                            .find(|c| c.kind() == SyntaxKind::METHOD_CALL_EXPR)
                    })
                    .is_some();

                if is_method_call {
                    // This is a method call: receiver.method()
                    self.resolve_qualified_method_call(expr_node)
                } else {
                    // This is a field access: receiver.field
                    let receiver = expr_node.children().next();
                    let field_name = self.extract_method_name_from_field_access(expr_node);
                    if let (Some(receiver_expr), Some(field_name)) = (receiver, field_name) {
                        let receiver_type = self.infer_expression_type(&receiver_expr);
                        self.resolve_field_type_with_builder_index(&receiver_type, &field_name)
                            .unwrap_or(Type::Unknown)
                    } else {
                        Type::Unknown
                    }
                }
            }

            // METHOD_CALL_EXPR: method() or object.method()
            SyntaxKind::METHOD_CALL_EXPR => {
                // Preferred path: extract receiver + method name from METHOD_CALL_EXPR itself.
                if let Some((receiver_type, method_name)) =
                    self.extract_method_call_target(expr_node)
                {
                    if let Some(return_type) = self.resolve_method_return_type(
                        &receiver_type,
                        &method_name,
                        Some(expr_node),
                    ) {
                        return return_type;
                    }
                }

                // Legacy fallback: METHOD_CALL_EXPR has FIELD_ACCESS_EXPR sibling in parent.
                let mut result = Type::Unknown;
                if let Some(parent) = expr_node.parent() {
                    // Look for FIELD_ACCESS_EXPR sibling
                    if let Some(field_access) = parent
                        .children()
                        .find(|c| c.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
                    {
                        result = self.resolve_qualified_method_call(&field_access);
                    }
                }
                result
            }

            // ARRAY_ACCESS_EXPR: array[index]
            // Note: This case handles array access expressions in general,
            // but for var declarations, array access is handled specially in infer_var_type
            SyntaxKind::ARRAY_ACCESS_EXPR => {
                // Get the array expression (first child would be the index)
                // This case may not be reached for var declarations due to special handling
                let mut result = Type::Unknown;
                if let Some(array_expr) = expr_node.children().next() {
                    let array_type = self.infer_expression_type(&array_expr);
                    if let Type::Array { element_type, .. } = array_type {
                        result = *element_type;
                    }
                }
                result
            }

            // TERNARY_EXPR: condition ? expr1 : expr2
            SyntaxKind::TERNARY_EXPR => {
                // Parser shapes vary:
                // - [true_branch, false_branch]
                // - [condition, true_branch, false_branch]
                // Use the trailing two expression children as the value branches.
                let mut result = Type::Unknown;
                let expr_children: Vec<_> = expr_node
                    .children()
                    .filter(|c| {
                        matches!(
                            c.kind(),
                            SyntaxKind::LITERAL_EXPR
                                | SyntaxKind::NAME_EXPR
                                | SyntaxKind::FIELD_ACCESS_EXPR
                                | SyntaxKind::METHOD_CALL_EXPR
                                | SyntaxKind::NEW_EXPR
                                | SyntaxKind::BINARY_EXPR
                                | SyntaxKind::UNARY_EXPR
                                | SyntaxKind::PAREN_EXPR
                                | SyntaxKind::CAST_EXPR
                                | SyntaxKind::TERNARY_EXPR
                                | SyntaxKind::THIS_EXPR
                                | SyntaxKind::SUPER_EXPR
                                | SyntaxKind::LAMBDA_EXPR
                                | SyntaxKind::ARRAY_ACCESS_EXPR
                                | SyntaxKind::ARRAY_INIT_EXPR
                                | SyntaxKind::CLASS_LITERAL_EXPR
                                | SyntaxKind::METHOD_REF_EXPR
                        )
                    })
                    .collect();
                if expr_children.len() >= 2 {
                    let true_idx = expr_children.len() - 2;
                    let false_idx = expr_children.len() - 1;
                    let true_type = self.infer_expression_type(&expr_children[true_idx]);
                    let false_type = self.infer_expression_type(&expr_children[false_idx]);

                    // Find the least upper bound (common supertype)
                    result = self.find_lub(&true_type, &false_type);
                }
                result
            }

            // SWITCH_EXPR: switch (x) { case ... -> expr; ... }
            SyntaxKind::SWITCH_EXPR => self.infer_switch_expression_type(expr_node),

            // Literals
            SyntaxKind::INT_LITERAL => Type::Primitive(PrimitiveType::Int),
            SyntaxKind::LONG_LITERAL => Type::Primitive(PrimitiveType::Long),
            SyntaxKind::FLOAT_LITERAL => Type::Primitive(PrimitiveType::Float),
            SyntaxKind::DOUBLE_LITERAL => Type::Primitive(PrimitiveType::Double),
            SyntaxKind::CHAR_LITERAL => Type::Primitive(PrimitiveType::Char),
            SyntaxKind::BOOLEAN_LITERAL => Type::Primitive(PrimitiveType::Boolean),
            SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => Type::Class(ClassType {
                name: "String".into(),
                package: Some("java.lang".into()),
                type_arguments: vec![],
                is_diamond: false,
            }),

            // CAST_EXPR: (Type) expr
            SyntaxKind::CAST_EXPR => {
                let first = self.extract_type_from_declaration(expr_node);
                let Some(intersection) = expr_node
                    .children()
                    .find(|child| child.kind() == SyntaxKind::INTERSECTION_TYPE)
                else {
                    return first;
                };
                // The parser stores the first bound as a sibling of the
                // INTERSECTION_TYPE node containing the remaining bounds.
                let mut types = vec![first];
                types.extend(
                    intersection
                        .children()
                        .filter(|child| child.kind() == SyntaxKind::CLASS_TYPE)
                        .map(|bound| self.extract_class_type(&bound)),
                );
                Type::Intersection { types }
            }

            // For other expressions, return Unknown
            _ => Type::Unknown,
        };

        // Cache the result before returning
        self.type_cache
            .borrow_mut()
            .insert(range, inferred_type.clone());
        inferred_type
    }

    /// Infer the type of a switch expression from its case result arms.
    ///
    /// Switch expression type is the least upper bound (LUB) of all arm types.
    fn infer_switch_expression_type(&self, switch_expr: &SyntaxNode) -> Type {
        let mut common = None;
        for expression in crate::utils::switch_result_expressions(switch_expr) {
            let Some(expression) = expression else {
                return Type::Unknown;
            };
            let result_type = self.infer_expression_type(&expression);
            if result_type == Type::Unknown {
                return Type::Unknown;
            }
            common = Some(common.map_or_else(
                || result_type.clone(),
                |previous| self.find_lub(&previous, &result_type),
            ));
        }
        common.unwrap_or(Type::Unknown)
    }

    /// Resolve the target type for a diamond `new Foo<>()` expression.
    ///
    /// Walks up to the parent declaration node and extracts the declared LHS type.
    /// Handles local variables (`List<String> x = new ArrayList<>()`) and
    /// fields (`private List<String> x = new ArrayList<>()`).
    fn resolve_diamond_target_type(&self, new_expr: &SyntaxNode) -> Option<Type> {
        let parent = new_expr.parent()?;
        match parent.kind() {
            SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::FIELD_DECL => {
                // The declared type is the first CLASS_TYPE child of the declaration
                for child in parent.children() {
                    if child.kind() == SyntaxKind::CLASS_TYPE {
                        return Some(self.extract_class_type(&child));
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// Resolve a field's type, using the builder's own workspace index when the
    /// resolver does not have one yet.
    ///
    /// `build_from_syntax` hands the index to the resolver only *after* the
    /// walk, but the walk itself infers `var` initializers, so during
    /// construction `SymbolResolver::resolve_field_type` bails out at its very
    /// first line and every `this.field` typed as `Type::Unknown`. That silently
    /// poisoned generic inference for calls like `Arrays.asList(this.name)`,
    /// whose `T` then defaulted to `java.lang.Object`.
    ///
    /// Moving the hand-over before the walk would fix this case but is not safe:
    /// construction-time inference runs against a partially built symbol table,
    /// and giving it the index makes it commit concrete-looking (but wrong)
    /// types that the later, index-backed re-inference no longer replaces --
    /// measured as 7 regressions across `var_type_inference_tests`,
    /// `advanced_generics_tests` and `wildcard_generics_tests`. Widening only
    /// this lookup keeps the hand-over point, and with it those types,
    /// unchanged.
    fn resolve_field_type_with_builder_index(
        &self,
        receiver_type: &Type,
        field_name: &str,
    ) -> Option<Type> {
        if let Some(field_type) = self.resolver.resolve_field_type(receiver_type, field_name) {
            return Some(field_type);
        }
        let workspace_index = self.workspace_index.as_ref()?;
        crate::resolver::type_resolution::resolve_effective_field(
            workspace_index,
            receiver_type,
            field_name,
        )
        .map(|(_, field_type)| field_type)
    }

    fn infer_enclosing_class_type(&self, context_node: &SyntaxNode) -> Type {
        if let Some(name) = self.class_name_stack.last() {
            return Type::Class(ClassType {
                name: name.clone(),
                package: self.current_package.clone(),
                type_arguments: vec![],
                is_diamond: false,
            });
        }

        // Fallback for targeted inference builders (e.g. for_var_inference) where
        // class_name_stack/current_package are not populated from a tree walk.
        //
        // This used to scan the class scope's *own* symbols for a
        // `SymbolKind::Class`, which can never find the enclosing class:
        // `process_type_declaration` deliberately defines a Java type in the
        // *enclosing* scope ("The type is declared in the enclosing scope. Its
        // fields and methods are declared in the class scope created above"),
        // so a class scope holds only its members. `this` therefore inferred as
        // `Type::Unknown` in every targeted-inference context, which in turn
        // made `this.field` untypable and silently poisoned generic argument
        // inference for calls like `Arrays.asList(this.name)`.
        //
        // `get_enclosing_class_at_offset` is the maintained implementation of
        // this lookup: it inspects the class scope and its parent, so it works
        // for both the Java and the Kotlin storage convention, and it picks the
        // earliest class-name range inside the scope so a nested declaration
        // stored in the outer scope is not mistaken for the outer class.
        let offset = u32::from(context_node.text_range().start());
        self.resolver
            .get_enclosing_class_at_offset(offset)
            .map_or(Type::Unknown, Type::Class)
    }

    /// Extract receiver type and method name from a method call node.
    ///
    /// Supports current parser shape:
    /// - METHOD_CALL_EXPR
    ///   - NAME_EXPR("receiver.method")
    ///   - ARG_LIST(...)
    ///
    /// Also supports unqualified calls (`method(...)`) by using the enclosing
    /// class as receiver.
    fn extract_method_call_target(&self, method_call: &SyntaxNode) -> Option<(Type, String)> {
        use rowan::NodeOrToken;

        let mut receiver_expr: Option<SyntaxNode> = None;
        let mut saw_dot = false;
        let mut method_name_after_dot: Option<String> = None;
        let mut first_ident: Option<String> = None;

        for item in method_call.children_with_tokens() {
            match item {
                NodeOrToken::Node(node) => {
                    if node.kind() != SyntaxKind::ARG_LIST && receiver_expr.is_none() {
                        receiver_expr = Some(node);
                    }
                }
                NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::DOT {
                        saw_dot = true;
                        continue;
                    }
                    if token.kind().is_identifier() {
                        if saw_dot {
                            method_name_after_dot = Some(token.text().to_string());
                            break;
                        }
                        if first_ident.is_none() {
                            first_ident = Some(token.text().to_string());
                        }
                    }
                }
            }
        }

        // Preferred/current parser shape:
        // METHOD_CALL_EXPR(NAME_EXPR|FIELD_ACCESS_EXPR, DOT, IDENT, ARG_LIST)
        if let Some(method_name) = method_name_after_dot {
            if let Some(receiver_node) = receiver_expr.as_ref() {
                if receiver_node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                    // Nested receiver call: `receiverCall().method(...)`
                    // Infer receiver type from the nested METHOD_CALL_EXPR node itself.
                    let receiver_type = self.infer_expression_type(receiver_node);
                    if !matches!(receiver_type, Type::Unknown) {
                        return Some((receiver_type, method_name));
                    }
                }
            }

            let receiver_text = receiver_expr
                .as_ref()
                .map_or_else(String::new, |n| n.text().to_string());
            let receiver_type =
                self.resolve_method_call_receiver_type(receiver_text.trim(), method_call);
            if !matches!(receiver_type, Type::Unknown) {
                return Some((receiver_type, method_name));
            }
        }

        // Legacy parser fallback:
        // METHOD_CALL_EXPR(NAME_EXPR("receiver.method"), ARG_LIST)
        if let Some(receiver_node) = &receiver_expr {
            if receiver_node.kind() == SyntaxKind::NAME_EXPR {
                let name_text_owned = receiver_node.text().to_string();
                let name_text = name_text_owned.trim();
                if let Some((receiver_text, method_name)) = name_text.rsplit_once('.') {
                    if !method_name.is_empty() {
                        let receiver_type = self
                            .resolve_method_call_receiver_type(receiver_text.trim(), method_call);
                        if !matches!(receiver_type, Type::Unknown) {
                            return Some((receiver_type, method_name.to_string()));
                        }
                    }
                }
            }
        }

        // Unqualified call: foo(...)
        // Treat receiver as the enclosing class (this.foo(...)).
        let method_name = first_ident
            .or_else(|| receiver_expr.map(|n| n.text().to_string()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;

        let receiver_type = self.class_name_stack.last().map(|class_name| {
            Type::Class(ClassType {
                name: class_name.clone(),
                package: self.current_package.clone(),
                type_arguments: vec![],
                is_diamond: false,
            })
        })?;
        Some((receiver_type, method_name))
    }

    /// Resolve the type of a method-call receiver represented as text.
    ///
    /// Handles:
    /// - Local variables/parameters (`optional`)
    /// - Simple class names for static calls (`Optional`)
    /// - Fully qualified class names (`java.util.Optional`)
    fn resolve_method_call_receiver_type(
        &self,
        receiver_text: &str,
        context_node: &SyntaxNode,
    ) -> Type {
        if receiver_text.is_empty() {
            return Type::Unknown;
        }

        // Static call heuristic: Optional.of(...)
        if receiver_text.chars().next().is_some_and(char::is_uppercase) {
            return Type::Class(ClassType {
                name: receiver_text.to_string(),
                package: None,
                type_arguments: vec![],
                is_diamond: false,
            });
        }

        // FQN class receiver: java.util.Optional.of(...)
        if receiver_text.contains('.') {
            if let Some(workspace) = self.workspace_index.as_ref() {
                if workspace.get_symbol(receiver_text).is_some() {
                    if let Some(parsed) = self.parse_type_from_string(receiver_text) {
                        return parsed;
                    }
                }
            }
        }

        // Variable receiver: optional.get(...)
        let offset = u32::from(context_node.text_range().start());
        let symbol_table = self.resolver.symbol_table();
        if let Some(scope_id) = symbol_table.find_scope_at_offset(offset) {
            if let Some(symbol_id) = self.lookup_symbol_in_scope(receiver_text, scope_id) {
                if let Some(symbol) = symbol_table.get_symbol(symbol_id) {
                    return symbol.type_info.clone();
                }
            }
        }

        Type::Unknown
    }

    /// Look up a symbol by name, walking up the scope chain
    pub(crate) fn lookup_symbol_in_scope(
        &self,
        name: &str,
        mut scope_id: ScopeId,
    ) -> Option<SymbolId> {
        let symbol_table = self.resolver.symbol_table();
        while let Some(scope) = symbol_table.get_scope(scope_id) {
            // Try to find the symbol in the current scope
            if let Some(symbol_id) = scope.lookup_local(name) {
                return Some(symbol_id);
            }

            // Move to parent scope
            if let Some(parent) = scope.parent {
                scope_id = parent;
            } else {
                // No more parent scopes
                break;
            }
        }
        None
    }

    // =========================================================================
    // Method Resolution
    // =========================================================================

    /// Resolve a qualified method call: receiver.method()
    /// Returns the return type of the method
    #[allow(clippy::unused_self)]
    pub(crate) fn type_to_fqn(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Class(ct) => Some(
                ct.package
                    .as_ref()
                    .map_or_else(|| ct.name.clone(), |pkg| format!("{pkg}.{}", ct.name)),
            ),
            _ => None,
        }
    }

    /// Parse a type string (from a workspace symbol) into a `Type`.
    ///
    /// Thin facade over [`crate::type_parser::parse_type_string`]. Returns
    /// `Type::Void` for `"void"`; the one production caller
    /// ([`crate::resolver::method_resolution::SymbolResolver::resolve_method_return_type`])
    /// re-maps that to [`Type::Unknown`] because a `var` binding to a void
    /// method has no meaningful inferred type.
    pub(crate) fn parse_type_from_string(&self, type_str: &str) -> Option<Type> {
        crate::type_parser::parse_type_string(type_str)
    }

    // =========================================================================
    // Type Extraction & Parsing
    // =========================================================================

    /// Build a revision-local resolution context for one type node.
    ///
    /// Imports and the package are file-level and therefore cached. Lexical type
    /// bindings are deliberately applied after cloning that cache: reusing the
    /// enclosing owner from the first extracted type used to resolve every later
    /// nested type in the wrong class. The syntax tree, rather than the workspace's
    /// retained import map, is authoritative so an unpublished editor revision
    /// cannot be combined with stale imports.
    fn build_resolution_context(&self, node: &SyntaxNode) -> ResolutionContext {
        // Clone outside the conditional so the immutable RefCell guard is
        // dropped before the cache is populated on a miss.
        let cached = self.resolution_context_cache.borrow().clone();
        let mut context = if let Some(cached) = cached {
            cached
        } else {
            let root = node.ancestors().last().unwrap_or_else(|| node.clone());
            let imports = crate::utils::build_imports_from_syntax(&root);
            let context = ResolutionContext::with_wildcards(
                imports.explicit,
                imports.wildcards,
                imports.current_package,
                None,
            );
            *self.resolution_context_cache.borrow_mut() = Some(context.clone());
            context
        };

        self.apply_lexical_type_context(node, &mut context);
        context.with_type_parameters(enclosing_type_parameter_names(node))
    }

    /// Add syntax-proven enclosing and member-type bindings nearest-first.
    ///
    /// A member type shadows single-type imports and same-package declarations.
    /// Keeping those facts separate from the cached file context is essential for
    /// files containing multiple top-level or nested declarations.
    fn apply_lexical_type_context(&self, node: &SyntaxNode, context: &mut ResolutionContext) {
        let mut declarations = node
            .ancestors()
            .filter(|ancestor| ancestor.kind().is_type_declaration())
            .collect::<Vec<_>>();
        if declarations.is_empty() {
            return;
        }

        // A declaration's own member types are not in scope in its header.
        let in_innermost_header = declarations[0]
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind() == SyntaxKind::L_BRACE)
            .is_none_or(|brace| node.text_range().start() < brace.text_range().start());
        if in_innermost_header {
            declarations.remove(0);
        }
        if declarations.is_empty() {
            return;
        }

        let mut prefix = context.package.clone().unwrap_or_default();
        let mut owners = Vec::with_capacity(declarations.len());
        for declaration in declarations.iter().rev() {
            let Some(name) = Self::type_declaration_name(declaration) else {
                continue;
            };
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(&name);
            owners.push((declaration.clone(), name, prefix.clone()));
        }
        owners.reverse();
        context.parent_class_fqn = owners.first().map(|(_, _, fqn)| fqn.clone());

        // Collect into a separate map so lexical declarations replace explicit
        // imports, while a declaration in the nearest owner wins over outer ones.
        let mut lexical_bindings = std::collections::HashMap::new();
        for (declaration, name, owner_fqn) in &owners {
            lexical_bindings
                .entry(name.clone())
                .or_insert_with(|| owner_fqn.clone());
            for member_name in self.member_type_names(declaration).iter() {
                lexical_bindings
                    .entry(member_name.clone())
                    .or_insert_with(|| format!("{owner_fqn}.{member_name}"));
            }
        }
        context.imports.extend(lexical_bindings);
    }

    /// Names of the type declarations directly inside `declaration`.
    ///
    /// Memoized per declaration: this is consulted once per type resolution, and
    /// scanning every member each time made whole-file resolution quadratic in
    /// the number of members.
    fn member_type_names(&self, declaration: &SyntaxNode) -> Arc<Vec<String>> {
        let key = declaration.text_range();
        if let Some(cached) = self.member_type_names_cache.borrow().get(&key) {
            return Arc::clone(cached);
        }

        let names = Arc::new(
            declaration
                .children()
                .filter(|child| child.kind().is_type_declaration())
                .filter_map(|member| Self::type_declaration_name(&member))
                .collect::<Vec<_>>(),
        );
        self.member_type_names_cache
            .borrow_mut()
            .insert(key, Arc::clone(&names));
        names
    }

    fn type_declaration_name(node: &SyntaxNode) -> Option<String> {
        node.children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind().is_type_identifier())
            .map(|token| token.text().to_owned())
    }

    /// Split an FQN into (package, name), handling nested classes properly
    ///
    /// For nested classes like `com.example.Outer.Inner`, we need to look up the symbol
    /// in the workspace to determine where the package ends and the class name begins.
    ///
    /// Returns (package, name) where:
    /// - `com.example.Outer.Inner` -> (Some("com.example"), "Outer.Inner")
    /// - `java.lang.String` -> (Some("java.lang"), "String")
    /// - `String` -> (None, "String")
    fn split_fqn_for_nested_class(&self, fqn: &str) -> (Option<String>, String) {
        // No dot means no package
        if !fqn.contains('.') {
            return (None, fqn.to_string());
        }
        if let Some(cached) = self.fqn_split_cache.borrow().get(fqn) {
            return cached.clone();
        }
        let split = self.split_fqn_for_nested_class_uncached(fqn);
        self.fqn_split_cache
            .borrow_mut()
            .insert(fqn.to_owned(), split.clone());
        split
    }

    fn split_fqn_for_nested_class_uncached(&self, fqn: &str) -> (Option<String>, String) {
        // Try workspace lookup if available
        if let Some(workspace) = &self.workspace_index {
            if let Some(symbol) = workspace.get_symbol(fqn) {
                // Symbol found!
                // For nested classes, the container field will be the outer class FQN
                // e.g., for "com.example.Outer.Inner":
                //   - fqn: "com.example.Outer.Inner"
                //   - container: Some("com.example.Outer")
                //   - simple_name: "Inner"

                if symbol.is_class_or_interface() {
                    if let Some(ref container_fqn) = symbol.container {
                        // This is a nested class - extract package from container
                        // container_fqn is the outer class FQN like "com.example.Outer"
                        // We need to extract the package part and build the full nested class name

                        // First, get the outer class's package by removing its simple name
                        if let Some(last_dot) = container_fqn.rfind('.') {
                            let package = container_fqn[..last_dot].to_string();
                            let outer_class_name = container_fqn[last_dot + 1..].to_string();
                            let full_name = format!("{}.{}", outer_class_name, symbol.simple_name);

                            tracing::debug!(
                                "split_fqn_for_nested_class: workspace lookup '{}' (nested class) -> package='{}', name='{}'",
                                fqn,
                                package,
                                full_name
                            );

                            return (Some(package), full_name);
                        }
                    } else {
                        // Top-level class - extract package from FQN
                        if let Some(last_dot) = fqn.rfind('.') {
                            let package = fqn[..last_dot].to_string();

                            tracing::debug!(
                                "split_fqn_for_nested_class: workspace lookup '{}' (top-level class) -> package='{}', name='{}'",
                                fqn,
                                package,
                                symbol.simple_name
                            );

                            return (Some(package), symbol.simple_name.to_string());
                        }
                    }
                }
            }
        }

        // Fallback: heuristic split (last dot)
        // This will be incorrect for nested classes, but it's the best we can do
        fqn.rfind('.').map_or_else(
            || (None, fqn.to_string()),
            |last_dot| {
                let pkg = fqn[..last_dot].to_string();
                let nm = fqn[last_dot + 1..].to_string();

                // Log potential nested class issue (when package contains uppercase chars)
                if pkg.chars().any(char::is_uppercase) {
                    tracing::debug!(
                        "split_fqn_for_nested_class: heuristic split '{}' -> package='{}', name='{}' \
                         (may be incorrect for nested classes)",
                        fqn,
                        pkg,
                        nm
                    );
                }

                (Some(pkg), nm)
            },
        )
    }

    /// Convert TypeStructure from hyperion-types to Type enum
    ///
    /// Properly handles nested classes by consulting the workspace index.
    /// For `com.example.Outer.Inner`:
    ///   - Looks up symbol in workspace
    ///   - If found, uses symbol's package info: package="com.example", name="Outer.Inner"
    ///   - If not found, falls back to heuristic split
    #[allow(clippy::only_used_in_recursion)]
    fn type_structure_to_type(&self, ts: TypeStructure) -> Type {
        if ts.base == "?" {
            return Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            };
        }
        if matches!(ts.base.as_str(), "? extends" | "? super") {
            let mut bounds = ts.type_args.into_iter();
            let Some(bound) = bounds.next() else {
                return Type::Unknown;
            };
            if bounds.next().is_some() {
                return Type::Unknown;
            }
            let bound = Some(Box::new(self.type_structure_to_type(bound)));
            return if ts.base == "? extends" {
                Type::Wildcard {
                    upper_bound: bound,
                    lower_bound: None,
                }
            } else {
                Type::Wildcard {
                    upper_bound: None,
                    lower_bound: bound,
                }
            };
        }

        // Handle primitives
        if ts.is_primitive {
            let prim = match ts.base.as_str() {
                "boolean" => PrimitiveType::Boolean,
                "byte" => PrimitiveType::Byte,
                "char" => PrimitiveType::Char,
                "short" => PrimitiveType::Short,
                "int" => PrimitiveType::Int,
                "long" => PrimitiveType::Long,
                "float" => PrimitiveType::Float,
                "double" => PrimitiveType::Double,
                _ => {
                    tracing::warn!("Unknown primitive type: {}", ts.base);
                    return Type::Unknown;
                }
            };

            // Handle arrays of primitives
            if ts.dimensions > 0 {
                return Type::Array {
                    element_type: Box::new(Type::Primitive(prim)),
                    dimensions: u8::try_from(ts.dimensions).unwrap_or(u8::MAX),
                };
            }

            return Type::Primitive(prim);
        }

        // Parse FQN into package and name, handling nested classes properly
        let (package, name) = self.split_fqn_for_nested_class(&ts.base);

        // Recursively convert type arguments
        let type_arguments = ts
            .type_args
            .into_iter()
            .map(|arg| self.type_structure_to_type(arg))
            .collect();

        let class_type = Type::Class(ClassType {
            name,
            package,
            type_arguments,
            is_diamond: ts.is_diamond,
        });

        // Handle arrays
        if ts.dimensions > 0 {
            Type::Array {
                element_type: Box::new(class_type),
                dimensions: u8::try_from(ts.dimensions).unwrap_or(u8::MAX),
            }
        } else {
            class_type
        }
    }

    /// Extract class type with generic type arguments
    ///
    /// Uses hyperion-types for extraction and resolution, then converts to Type enum.
    pub(crate) fn extract_class_type(&self, node: &SyntaxNode) -> Type {
        self.extract_class_type_with_bounds(node, &mut Vec::new())
    }

    fn extract_class_type_with_bounds(&self, node: &SyntaxNode, active: &mut Vec<String>) -> Type {
        let context = self.build_resolution_context(node);

        // Use workspace-aware resolution if available (handles nested classes properly)
        let result = self.workspace_index.as_ref().map_or_else(
            || extract_and_resolve_type_without_lookup(node, &context),
            |workspace| {
                extract_and_resolve_type_with_lookup(
                    node,
                    &context,
                    &CachedLookup {
                        workspace: workspace.as_ref(),
                        cache: &self.symbol_exists_cache,
                    },
                )
            },
        );

        match result {
            Ok(type_structure) => {
                fn referenced_names(ty: &TypeStructure, names: &mut HashSet<String>) {
                    names.insert(ty.base.clone());
                    for argument in &ty.type_args {
                        referenced_names(argument, names);
                    }
                }
                let mut referenced = HashSet::new();
                referenced_names(&type_structure, &mut referenced);
                let ty = self.type_structure_to_type(type_structure);
                let mut substitutions = HashMap::new();
                for parameter in enclosing_type_parameter_declarations(node) {
                    let name = parameter.children_with_tokens().find_map(|element| {
                        element.as_token().and_then(|token| {
                            (token.kind() == SyntaxKind::IDENT).then(|| token.text().to_string())
                        })
                    });
                    let Some(name) = name else { continue };
                    if !referenced.contains(&name) || substitutions.contains_key(&name) {
                        continue;
                    }
                    let bounds = if active.contains(&name) || active.len() >= 32 {
                        // Recursive bounds (T extends Comparable<T>) retain a
                        // finite reference to T instead of expanding forever.
                        Vec::new()
                    } else {
                        active.push(name.clone());
                        let bounds = parameter
                            .children()
                            .filter(|child| child.kind() == SyntaxKind::TYPE_BOUND)
                            .flat_map(|bound| bound.children())
                            .filter(|child| child.kind() == SyntaxKind::CLASS_TYPE)
                            .map(|bound| self.extract_class_type_with_bounds(&bound, active))
                            .collect();
                        active.pop();
                        bounds
                    };
                    substitutions.insert(name.clone(), Type::TypeVariable { name, bounds });
                }
                super::type_parameter_mapping::apply_type_parameter_substitutions(
                    ty,
                    &substitutions,
                )
            }
            Err(e) => {
                tracing::debug!(
                    "Failed to extract class type from node (kind={:?}): {}. Returning Unknown.",
                    node.kind(),
                    e
                );
                Type::Unknown
            }
        }
    }

    /// Extract primitive type from PRIMITIVE_TYPE node
    ///
    /// Uses hyperion-types for extraction, then converts to Type enum.
    pub(crate) fn extract_primitive_type(&self, node: &SyntaxNode) -> Option<Type> {
        let context = self.build_resolution_context(node);

        // Use workspace-aware resolution if available
        let result = self.workspace_index.as_ref().map_or_else(
            || extract_and_resolve_type_without_lookup(node, &context),
            |workspace| {
                extract_and_resolve_type_with_lookup(
                    node,
                    &context,
                    &CachedLookup {
                        workspace: workspace.as_ref(),
                        cache: &self.symbol_exists_cache,
                    },
                )
            },
        );

        match result {
            Ok(type_structure) => Some(self.type_structure_to_type(type_structure)),
            Err(e) => {
                tracing::debug!(
                    "Failed to extract primitive type from node (kind={:?}): {}. Returning None.",
                    node.kind(),
                    e
                );
                None
            }
        }
    }

    /// Extract array type (with dimensions)
    ///
    /// Uses hyperion-types for extraction and resolution, then converts to Type enum.
    pub(crate) fn extract_array_type(&self, node: &SyntaxNode) -> Option<Type> {
        let context = self.build_resolution_context(node);

        // Use workspace-aware resolution if available
        let result = self.workspace_index.as_ref().map_or_else(
            || extract_and_resolve_type_without_lookup(node, &context),
            |workspace| {
                extract_and_resolve_type_with_lookup(
                    node,
                    &context,
                    &CachedLookup {
                        workspace: workspace.as_ref(),
                        cache: &self.symbol_exists_cache,
                    },
                )
            },
        );

        match result {
            Ok(type_structure) => Some(self.type_structure_to_type(type_structure)),
            Err(e) => {
                tracing::debug!(
                    "Failed to extract array type from node (kind={:?}): {}. Returning None.",
                    node.kind(),
                    e
                );
                None
            }
        }
    }

    // =========================================================================
    // Least Upper Bound (LUB) for Type Inference
    // =========================================================================

    /// Find the least upper bound (common supertype) of two types
    ///
    /// Used for ternary expressions, array initializers, and other contexts
    /// where we need to find a common type from multiple expressions.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Same type
    /// lub(String, String) = String
    ///
    /// // Primitive widening
    /// lub(int, long) = long
    /// lub(float, double) = double
    ///
    /// // Reference types (requires class hierarchy lookup)
    /// lub(String, StringBuilder) = CharSequence
    /// lub(ArrayList, LinkedList) = List
    ///
    /// // Unrelated types
    /// lub(String, Integer) = Object
    /// ```
    pub(crate) fn find_lub(&self, type1: &Type, type2: &Type) -> Type {
        // Same type - trivial case
        if type1 == type2 {
            return type1.clone();
        }

        // One is Unknown - use the other (error recovery)
        if matches!(type1, Type::Unknown) {
            return type2.clone();
        }
        if matches!(type2, Type::Unknown) {
            return type1.clone();
        }

        // Null type - the other type is the LUB (null is assignable to any reference type)
        if matches!(type1, Type::Null) {
            return type2.clone();
        }
        if matches!(type2, Type::Null) {
            return type1.clone();
        }

        // Primitive widening (int + long = long, float + double = double)
        if let (Type::Primitive(p1), Type::Primitive(p2)) = (type1, type2) {
            return Type::Primitive(self.widen_primitives(*p1, *p2));
        }

        // Arrays - LUB of element types, dimensions must match
        if let (
            Type::Array {
                element_type: e1,
                dimensions: d1,
            },
            Type::Array {
                element_type: e2,
                dimensions: d2,
            },
        ) = (type1, type2)
        {
            if d1 == d2 {
                // Same dimensions - find LUB of element types
                let element_lub = self.find_lub(e1, e2);
                return Type::Array {
                    element_type: Box::new(element_lub),
                    dimensions: *d1,
                };
            }
            // Different dimensions - fall back to Object
            // In Java, all arrays are subclasses of Object
            return Type::Class(ClassType::object());
        }

        // Reference types - find common supertype via class hierarchy
        if let (Type::Class(c1), Type::Class(c2)) = (type1, type2) {
            return self.find_class_lub(c1, c2);
        }

        // Mixed primitive and reference - no common type except boxed types
        // This is a complex case (autoboxing) - for now, return Object
        if (matches!(type1, Type::Primitive(_)) && matches!(type2, Type::Class(_)))
            || (matches!(type1, Type::Class(_)) && matches!(type2, Type::Primitive(_)))
        {
            debug!(
                "LUB of primitive and reference type (autoboxing context): {:?} and {:?}",
                type1, type2
            );
            return Type::Class(ClassType::object());
        }

        // Default fallback - Object (all reference types extend Object)
        debug!(
            "LUB fallback to Object for unhandled case: {:?} and {:?}",
            type1, type2
        );
        Type::Class(ClassType::object())
    }

    /// Widen two primitive types according to Java widening rules
    ///
    /// Returns the wider of the two types, or the first type if they're incompatible.
    #[allow(clippy::unused_self)]
    fn widen_primitives(&self, p1: PrimitiveType, p2: PrimitiveType) -> PrimitiveType {
        use PrimitiveType::{Boolean, Byte, Double, Float, Int, Long, Short};

        // Boolean doesn't widen to anything
        if p1 == Boolean || p2 == Boolean {
            return p1; // Return first type (no valid widening)
        }

        // Widening hierarchy: byte -> short -> int -> long -> float -> double
        //                      char -> int -> long -> float -> double
        //
        // Note: char doesn't widen to short or byte (different sign semantics)

        match (p1, p2) {
            // Same type
            (a, b) if a == b => a,

            // Double is the widest
            (Double, _) | (_, Double) => Double,

            // Float is next widest (but not for double)
            (Float, _) | (_, Float) => Float,

            // Long is next (but not for float/double)
            (Long, _) | (_, Long) => Long,

            // Short can hold byte
            (Short, Byte) | (Byte, Short) => Short,

            // Int can hold byte, short, char; fallback for all remaining integral types
            _ => Int,
        }
    }

    /// Find the least upper bound of two class types
    ///
    /// This requires class hierarchy information from the workspace.
    /// If workspace is not available, we fall back to Object.
    fn find_class_lub(&self, c1: &ClassType, c2: &ClassType) -> Type {
        // Same class - trivial case
        if c1.qualified_name() == c2.qualified_name() {
            return Type::Class(c1.clone());
        }

        // If we don't have workspace index, we can't traverse hierarchy
        let Some(workspace) = &self.workspace_index else {
            debug!("No workspace index available for LUB calculation - falling back to Object");
            return Type::Class(ClassType::object());
        };

        // Get fully qualified names
        let fqn1 = c1.qualified_name();
        let fqn2 = c2.qualified_name();

        // Build supertype chains for both classes
        let chain1 = self.get_supertype_chain(&fqn1, workspace);
        let chain2 = self.get_supertype_chain(&fqn2, workspace);

        debug!(
            "LUB: chain1 for {} = {:?}",
            fqn1,
            chain1.iter().take(5).collect::<Vec<_>>()
        );
        debug!(
            "LUB: chain2 for {} = {:?}",
            fqn2,
            chain2.iter().take(5).collect::<Vec<_>>()
        );

        let common = chain1
            .iter()
            .filter(|ancestor| chain2.contains(ancestor))
            .cloned()
            .collect::<Vec<_>>();

        // A representable least upper bound is the unique common candidate
        // whose own canonical supertype closure contains every other common
        // candidate. This chooses Reader over Closeable and Readable over
        // Object without electing one of several incomparable interfaces by
        // index order.
        let mut most_specific = common.iter().filter(|candidate| {
            let closure = self.get_supertype_chain(candidate, workspace);
            common.iter().all(|other| closure.contains(other))
        });
        let first_most_specific = most_specific.next();
        let selected = first_most_specific
            .filter(|_| most_specific.next().is_none())
            .or_else(|| {
                // The complete Java LUB may be an intersection type. This
                // path currently returns one ClassType, so retain the nearest
                // common class (normally Object) as a conservative upper
                // bound instead of guessing an incomparable interface.
                common.iter().find(|ancestor| {
                    !workspace
                        .get_symbol(ancestor)
                        .is_some_and(|symbol| symbol.kind == lsp_types::SymbolKind::INTERFACE)
                })
            });
        if let Some(ancestor) = selected {
            debug!("LUB: Found common ancestor: {}", ancestor);
            return self.fqn_to_type(ancestor);
        }

        // No common ancestor found - fall back to Object
        debug!(
            "LUB: No common ancestor found for {} and {} - falling back to Object",
            fqn1, fqn2
        );
        Type::Class(ClassType::object())
    }

    /// Get the supertype chain for a class (including the class itself)
    ///
    /// Returns a list of FQNs starting from the class and going up to Object.
    /// For example, for ArrayList:
    /// ["java.util.ArrayList", "java.util.AbstractList", "java.util.AbstractCollection", "java.lang.Object"]
    ///
    /// Also includes interfaces in the chain.
    #[allow(clippy::unused_self)]
    fn get_supertype_chain(
        &self,
        fqn: &str,
        workspace: &std::sync::Arc<hyperion_workspace::WorkspaceIndex>,
    ) -> Vec<String> {
        use std::collections::{HashSet, VecDeque};

        // Traverse canonical direct edges breadth-first. The member-lookup
        // inheritance chain intentionally exhausts the concrete class chain
        // before interfaces; LUB needs graph distance for both roles.
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut pending = VecDeque::from([fqn.to_owned()]);
        while let Some(current) = pending.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            chain.push(current.clone());

            let parent = workspace.get_parent_class(&current);
            if let Some(parent) = parent.as_ref() {
                pending.push_back(parent.clone());
            }
            for supertype in workspace.direct_supertypes(&current) {
                if parent.as_deref() != Some(supertype.as_str()) {
                    pending.push_back(supertype);
                }
            }
        }

        // Always end with Object (unless we're already at Object)
        if !chain.contains(&"java.lang.Object".to_string()) && fqn != "java.lang.Object" {
            chain.push("java.lang.Object".to_string());
        }

        chain
    }

    /// Convert an FQN to a Type
    ///
    /// Splits package and class name properly (handling nested classes).
    fn fqn_to_type(&self, fqn: &str) -> Type {
        let (package, name) = self.split_fqn_for_nested_class(fqn);
        Type::Class(ClassType {
            name,
            package,
            type_arguments: vec![],
            is_diamond: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Type-argument splitting was extracted into `crate::type_parser` along with
    // the rest of the type-string parser; coverage now lives in
    // `type_parser::tests::type_argument_splitting`.

    // =========================================================================
    // LUB (Least Upper Bound) Tests
    // =========================================================================

    #[test]
    fn test_widen_primitives_same_type() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        assert_eq!(
            builder.widen_primitives(PrimitiveType::Int, PrimitiveType::Int),
            PrimitiveType::Int
        );
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Double, PrimitiveType::Double),
            PrimitiveType::Double
        );
    }

    #[test]
    fn test_widen_primitives_int_long() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        assert_eq!(
            builder.widen_primitives(PrimitiveType::Int, PrimitiveType::Long),
            PrimitiveType::Long
        );
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Long, PrimitiveType::Int),
            PrimitiveType::Long
        );
    }

    #[test]
    fn test_widen_primitives_float_double() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        assert_eq!(
            builder.widen_primitives(PrimitiveType::Float, PrimitiveType::Double),
            PrimitiveType::Double
        );
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Double, PrimitiveType::Float),
            PrimitiveType::Double
        );
    }

    #[test]
    fn test_widen_primitives_byte_short_int() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        assert_eq!(
            builder.widen_primitives(PrimitiveType::Byte, PrimitiveType::Short),
            PrimitiveType::Short
        );
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Byte, PrimitiveType::Int),
            PrimitiveType::Int
        );
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Short, PrimitiveType::Int),
            PrimitiveType::Int
        );
    }

    #[test]
    fn test_widen_primitives_char_int() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        // Char widens to int (not short or byte)
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Char, PrimitiveType::Int),
            PrimitiveType::Int
        );
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Char, PrimitiveType::Short),
            PrimitiveType::Int
        );
        assert_eq!(
            builder.widen_primitives(PrimitiveType::Char, PrimitiveType::Byte),
            PrimitiveType::Int
        );
    }

    #[test]
    fn test_lub_same_type() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        let t1 = Type::Primitive(PrimitiveType::Int);
        let t2 = Type::Primitive(PrimitiveType::Int);
        assert_eq!(
            builder.find_lub(&t1, &t2),
            Type::Primitive(PrimitiveType::Int)
        );

        let t3 = Type::Class(ClassType::string());
        let t4 = Type::Class(ClassType::string());
        assert_eq!(builder.find_lub(&t3, &t4), Type::Class(ClassType::string()));
    }

    #[test]
    fn test_lub_unknown_types() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        let unknown = Type::Unknown;
        let int_type = Type::Primitive(PrimitiveType::Int);

        // Unknown + anything = anything
        assert_eq!(
            builder.find_lub(&unknown, &int_type),
            Type::Primitive(PrimitiveType::Int)
        );
        assert_eq!(
            builder.find_lub(&int_type, &unknown),
            Type::Primitive(PrimitiveType::Int)
        );
    }

    #[test]
    fn test_lub_null_type() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        let null_type = Type::Null;
        let string_type = Type::Class(ClassType::string());

        // Null + reference type = reference type
        assert_eq!(
            builder.find_lub(&null_type, &string_type),
            Type::Class(ClassType::string())
        );
        assert_eq!(
            builder.find_lub(&string_type, &null_type),
            Type::Class(ClassType::string())
        );
    }

    #[test]
    fn test_lub_primitives() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        // int + long = long
        let int_type = Type::Primitive(PrimitiveType::Int);
        let long_type = Type::Primitive(PrimitiveType::Long);
        assert_eq!(
            builder.find_lub(&int_type, &long_type),
            Type::Primitive(PrimitiveType::Long)
        );

        // float + double = double
        let float_type = Type::Primitive(PrimitiveType::Float);
        let double_type = Type::Primitive(PrimitiveType::Double);
        assert_eq!(
            builder.find_lub(&float_type, &double_type),
            Type::Primitive(PrimitiveType::Double)
        );

        // byte + short = short
        let byte_type = Type::Primitive(PrimitiveType::Byte);
        let short_type = Type::Primitive(PrimitiveType::Short);
        assert_eq!(
            builder.find_lub(&byte_type, &short_type),
            Type::Primitive(PrimitiveType::Short)
        );
    }

    #[test]
    fn test_lub_arrays_same_element_type() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        let arr1 = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        let arr2 = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };

        assert_eq!(
            builder.find_lub(&arr1, &arr2),
            Type::Array {
                element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
                dimensions: 1,
            }
        );
    }

    #[test]
    fn test_lub_arrays_different_element_types() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        // String[] and Integer[] should give Object[]
        let arr1 = Type::Array {
            element_type: Box::new(Type::Class(ClassType::string())),
            dimensions: 1,
        };
        let arr2 = Type::Array {
            element_type: Box::new(Type::Class(ClassType {
                name: "Integer".into(),
                package: Some("java.lang".into()),
                type_arguments: vec![],
                is_diamond: false,
            })),
            dimensions: 1,
        };

        // Should be Object[] (common supertype)
        let result = builder.find_lub(&arr1, &arr2);
        if let Type::Array {
            element_type,
            dimensions,
        } = result
        {
            assert_eq!(dimensions, 1);
            assert_eq!(*element_type, Type::Class(ClassType::object()));
        } else {
            panic!("Expected array type, got {:?}", result);
        }
    }

    #[test]
    fn test_lub_arrays_different_dimensions() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        let arr1 = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 1,
        };
        let arr2 = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Int)),
            dimensions: 2,
        };

        // Different dimensions -> Object
        assert_eq!(
            builder.find_lub(&arr1, &arr2),
            Type::Class(ClassType::object())
        );
    }

    #[test]
    fn test_lub_primitive_and_reference() {
        use crate::resolver::SymbolResolver;
        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        let int_type = Type::Primitive(PrimitiveType::Int);
        let string_type = Type::Class(ClassType::string());

        // Primitive + reference -> Object (autoboxing context)
        assert_eq!(
            builder.find_lub(&int_type, &string_type),
            Type::Class(ClassType::object())
        );
    }

    #[test]
    fn test_diamond_infer_from_local_var_decl() {
        use crate::resolver::SymbolResolver;
        use hyperion_parser::{Lexer, Parser};

        let code = r"
            class Test {
                void test() {
                    java.util.List<String> names = new java.util.ArrayList<>();
                }
            }
        ";

        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        // Find the NEW_EXPR node
        fn find_new_expr(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::NEW_EXPR {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_new_expr(&child) {
                    return Some(found);
                }
            }
            None
        }

        let new_expr = find_new_expr(&root).expect("Should find NEW_EXPR");
        let inferred = builder.infer_expression_type(&new_expr);

        match inferred {
            Type::Class(ct) => {
                assert_eq!(ct.name, "ArrayList");
                assert!(!ct.is_diamond, "Diamond should be resolved");
                assert_eq!(
                    ct.type_arguments.len(),
                    1,
                    "Should have inferred 1 type arg from LHS"
                );
                match &ct.type_arguments[0] {
                    Type::Class(arg_ct) => assert_eq!(arg_ct.name, "String"),
                    other => panic!("Expected Class type arg, got {:?}", other),
                }
            }
            other => panic!("Expected Class type, got {:?}", other),
        }
    }

    #[test]
    fn test_diamond_no_target_stays_diamond() {
        use crate::resolver::SymbolResolver;
        use hyperion_parser::{Lexer, Parser};

        // Diamond in a context where there's no resolvable target type
        // (e.g., standalone expression — contrived but tests fallback)
        let code = r"
            class Test {
                void test() {
                    Object x = new java.util.ArrayList<>();
                }
            }
        ";

        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let resolver = SymbolResolver::new();
        let builder = ResolverBuilder::new_for_testing(resolver);

        fn find_new_expr(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::NEW_EXPR {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_new_expr(&child) {
                    return Some(found);
                }
            }
            None
        }

        let new_expr = find_new_expr(&root).expect("Should find NEW_EXPR");
        let inferred = builder.infer_expression_type(&new_expr);

        // Object has no type args, so diamond can't be resolved — stays as ArrayList (no args)
        match inferred {
            Type::Class(ct) => {
                assert_eq!(ct.name, "ArrayList");
                // Diamond resolved from Object, which has no type args → empty
                assert!(ct.type_arguments.is_empty());
            }
            other => panic!("Expected Class type, got {:?}", other),
        }
    }

    // LUB must prefer the nearest common superclass over a shared
    // marker interface. get_supertype_chain previously popped from a Vec
    // (depth-first, interfaces surfacing first), so FileReader/StringReader
    // produced Closeable instead of Reader.
    #[test]
    fn test_lub_prefers_nearest_superclass_over_marker_interface() {
        use crate::db::FileId;
        use std::path::PathBuf;
        use std::sync::Arc;

        fn type_symbol(
            fqn: &str,
            simple_name: &str,
            kind: lsp_types::SymbolKind,
            parent_class: Option<&str>,
            interfaces: &[&str],
        ) -> hyperion_workspace::Symbol {
            hyperion_workspace::Symbol {
                fqn: fqn.into(),
                simple_name: simple_name.into(),
                range: hyperion_workspace::TextRange::new(0, 100),
                kind,
                modifiers: hyperion_workspace::Modifiers::default(),
                container: None,
                return_type: None,
                parameters: vec![],
                parent_class: parent_class.map(Into::into),
                interfaces: interfaces.iter().map(|i| (*i).into()).collect(),
                documentation: None,
                type_parameters: vec![],
                is_deprecated: false,
                receiver_type: None,
                kotlin: None,
                throws: None,
            }
        }

        let index =
            hyperion_workspace::WorkspaceIndex::new(PathBuf::from("/tmp/hyperion-lub-test"));
        index.add_external_symbol(type_symbol(
            "java.io.Closeable",
            "Closeable",
            lsp_types::SymbolKind::INTERFACE,
            None,
            &[],
        ));
        index.add_external_symbol(type_symbol(
            "java.io.Reader",
            "Reader",
            lsp_types::SymbolKind::CLASS,
            Some("java.lang.Object"),
            &["java.io.Closeable"],
        ));
        index.add_external_symbol(type_symbol(
            "java.io.FileReader",
            "FileReader",
            lsp_types::SymbolKind::CLASS,
            Some("java.io.Reader"),
            &["java.io.Closeable"],
        ));
        index.add_external_symbol(type_symbol(
            "java.io.StringReader",
            "StringReader",
            lsp_types::SymbolKind::CLASS,
            Some("java.io.Reader"),
            &["java.io.Closeable"],
        ));

        let builder = ResolverBuilder::with_workspace(FileId::new(0), Arc::new(index));

        let file_reader = Type::Class(ClassType::new(
            "FileReader".to_string(),
            Some("java.io".to_string()),
        ));
        let string_reader = Type::Class(ClassType::new(
            "StringReader".to_string(),
            Some("java.io".to_string()),
        ));

        match builder.find_lub(&file_reader, &string_reader) {
            Type::Class(ct) => assert_eq!(
                ct.qualified_name(),
                "java.io.Reader",
                "nearest common superclass must win over Closeable"
            ),
            other => panic!("Expected Class LUB, got {other:?}"),
        }
    }
}
