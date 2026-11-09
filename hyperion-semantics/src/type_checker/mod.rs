// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Checker
//!
//! Performs semantic type checking on the AST to detect type mismatches,
//! incompatible assignments, and other type-related errors.
//!
//! This module walks the AST and validates:
//! - Variable declarations: `String foo = 1;` → Error
//! - Variable assignments: `var foo = 1; foo = "Hey";` → Error
//! - Method argument count and assignability for resolved candidates
//!
//! Declared method return types are not yet checked against every return
//! statement.

use crate::db::FileId;
use crate::file_semantics::FileSemantics;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::symbol::{Symbol, SymbolKind, Type};
use hyperion_parser::{SyntaxKind, SyntaxNode};
use hyperion_workspace::{ImportMap, WorkspaceIndex};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, warn};

// Submodules containing helper methods
mod ast_extraction;
mod exception_flow;
mod method_validation;
mod scope;
mod type_resolution;

#[cfg(test)]
#[path = "../type_checker_tests.rs"]
mod tests;

use crate::utils::{build_imports_from_syntax, strip_generics};

/// Type checker that validates type consistency in the AST
///
/// This checker performs basic type validation for:
/// - Explicit type declarations matching their initializer types
/// - Assignment type compatibility
/// - Null assignment to primitive types
///
/// Note: For `var` declarations, type inference is handled by the resolver
/// (see resolver/type_inference.rs). This checker focuses on validating
/// explicit type annotations.
pub struct TypeChecker {
    /// Cached contextual type extraction shared with semantic inference.
    syntax_type_resolver: Option<crate::resolver::ResolverBuilder>,
    /// Stack of scopes, each containing a map from variable name to its type
    /// Inner scopes shadow outer scopes when variables have the same name
    variable_types_stack: Vec<HashMap<String, Type>>,
    /// Collected type errors
    errors: Vec<SemanticError>,
    /// Optional workspace context for import-aware type checking
    workspace_context: Option<WorkspaceContext>,
    /// Known type parameters in this file (best-effort to avoid false positives)
    type_parameters: HashSet<String>,
    /// Local type declarations in this file (classes/interfaces/enums/records)
    local_types: HashSet<String>,
    /// Pre-computed file semantics (expression types) from the semantic walker
    file_semantics: Option<Arc<FileSemantics>>,
    /// Hierarchy memo shared by every provability query of this pass.
    facts_cache: crate::compile_checks::facts::TypeFactsCache,
    /// Return expectation of each enclosing body, innermost last. `None` for
    /// bodies whose return type is not known here (lambdas, class bodies).
    return_expectations: Vec<Option<ReturnExpectation>>,
    /// Exception handlers surrounding the node being walked, innermost last.
    handlers: Vec<crate::compile_checks::exceptions::Handler>,
}

/// What a `return` statement inside the current body must look like.
#[derive(Debug, Clone)]
enum ReturnExpectation {
    /// `void` method, constructor or initializer: `return;` only.
    Nothing,
    /// A declared, non-void return type.
    Value(Type),
}

/// Workspace context for import-aware type resolution
struct WorkspaceContext {
    /// The file being type-checked (used for diagnostic logging and debugging)
    file_id: FileId,
    /// Imports for the current file
    imports: ImportMap,
    /// Workspace index for class hierarchy lookups
    workspace_index: Arc<WorkspaceIndex>,
}

impl TypeChecker {
    /// Create a new type checker
    pub fn new() -> Self {
        Self {
            // Pre-allocate root scope for typical method size (10-15 params + locals)
            variable_types_stack: vec![HashMap::with_capacity(15)],
            errors: Vec::new(),
            workspace_context: None,
            syntax_type_resolver: None,
            type_parameters: HashSet::new(),
            local_types: HashSet::new(),
            file_semantics: None,
            facts_cache: crate::compile_checks::facts::TypeFactsCache::default(),
            return_expectations: Vec::new(),
            handlers: Vec::new(),
        }
    }

    /// Set pre-computed file semantics for expression type lookups.
    pub fn set_file_semantics(&mut self, file_sem: Arc<FileSemantics>) {
        self.file_semantics = Some(file_sem);
    }

    /// Check a file's AST and return type errors (without workspace context)
    ///
    /// This method walks the AST and validates:
    /// 1. Variable declarations have compatible types with their initializers
    /// 2. Variable assignments are type-compatible
    /// 3. Primitive types are not assigned null
    ///
    /// # Arguments
    /// * `root` - The root syntax node of the AST
    /// * `symbols` - Resolved symbols from the symbol resolver
    ///
    /// # Returns
    /// A vector of semantic errors found during type checking
    pub fn check(&mut self, root: &SyntaxNode, symbols: &[Symbol]) -> Vec<SemanticError> {
        self.workspace_context = None;
        self.syntax_type_resolver = None;
        // Clear state from previous checks (if instance is reused)
        self.variable_types_stack.clear();
        self.variable_types_stack.push(HashMap::with_capacity(15));
        self.errors.clear();
        self.return_expectations.clear();
        self.handlers.clear();
        self.type_parameters.clear();
        self.local_types.clear();

        // First, build the variable type map from symbols
        self.build_variable_type_map(symbols);
        self.collect_local_types(root);

        // Walk the AST and check for type errors
        self.walk_node(root);

        // Return collected errors
        std::mem::take(&mut self.errors)
    }

    /// Check a file's AST with full workspace context for import-aware type checking
    ///
    /// This method provides better type checking by:
    /// - Resolving simple type names to FQNs using imports
    /// - Using class hierarchy from the workspace index for subtype checks
    ///
    /// # Arguments
    /// * `root` - The root syntax node of the AST
    /// * `symbols` - Resolved symbols from the symbol resolver
    /// * `file_id` - The file being type-checked
    /// * `workspace_index` - The workspace index for class lookups
    ///
    /// # Returns
    /// A vector of semantic errors found during type checking
    pub fn check_with_context(
        &mut self,
        root: &SyntaxNode,
        symbols: &[Symbol],
        file_id: FileId,
        workspace_index: Arc<WorkspaceIndex>,
    ) -> Vec<SemanticError> {
        // Clear state from previous checks (if instance is reused)
        self.variable_types_stack.clear();
        self.variable_types_stack.push(HashMap::with_capacity(15));
        self.errors.clear();
        self.return_expectations.clear();
        self.handlers.clear();
        self.type_parameters.clear();
        self.local_types.clear();

        // Prefer imports extracted from the in-memory syntax tree, since it reflects the
        // current editor buffer and avoids false "cannot find symbol" when the workspace
        // index ImportMap is stale or missing for this FileId.
        let syntax_imports = build_imports_from_syntax(root);

        // Still log workspace-index import availability for debugging.
        let indexed_imports_opt = workspace_index.get_imports(file_id);
        match indexed_imports_opt {
            Some(ref indexed_imports) => {
                debug!(
                    "TYPE_CHECKER: Workspace ImportMap for file {:?}: {} explicit, {} wildcards, pkg={:?}",
                    file_id,
                    indexed_imports.explicit.len(),
                    indexed_imports.wildcards.len(),
                    indexed_imports.current_package
                );
            }
            None => {
                warn!(
                    "TYPE_CHECKER: No workspace ImportMap found for file {:?}; using imports extracted from syntax",
                    file_id
                );
            }
        }

        self.syntax_type_resolver = Some(crate::resolver::ResolverBuilder::with_workspace(
            file_id,
            Arc::clone(&workspace_index),
        ));

        // Store workspace context
        self.workspace_context = Some(WorkspaceContext {
            file_id,
            imports: syntax_imports,
            workspace_index,
        });

        if let Some(ref ctx) = self.workspace_context {
            debug!("Type checking file {:?}", ctx.file_id);
        }

        // First, build the variable type map from symbols
        self.build_variable_type_map(symbols);
        self.collect_local_types(root);

        // Walk the AST and check for type errors
        self.walk_node(root);

        // Log error summary for diagnostics
        if !self.errors.is_empty() {
            if let Some(ref ctx) = self.workspace_context {
                debug!(
                    "Type checking file {:?} found {} semantic errors",
                    ctx.file_id,
                    self.errors.len()
                );
            }
        }

        // Clear context and return errors
        self.workspace_context = None;
        std::mem::take(&mut self.errors)
    }

    /// Build a map of variable names to their types from the symbol table
    fn build_variable_type_map(&mut self, symbols: &[Symbol]) {
        for symbol in symbols {
            if matches!(
                symbol.kind,
                SymbolKind::LocalVariable { .. }
                    | SymbolKind::Parameter { .. }
                    | SymbolKind::Field { .. }
            ) {
                // Insert into the current (top-level) scope
                if let Some(current_scope) = self.variable_types_stack.last_mut() {
                    current_scope.insert(symbol.name.clone(), symbol.type_info.clone());
                }
            }

            if matches!(symbol.kind, SymbolKind::TypeParameter { .. }) {
                self.type_parameters.insert(symbol.name.clone());
            }
        }
    }

    /// Walk the AST node and check for type errors
    fn walk_node(&mut self, node: &SyntaxNode) {
        use SyntaxKind::{
            ASSIGNMENT_EXPR, BLOCK_STMT, CLASS_DECL, CONSTRUCTOR_DECL, ENUM_DECL, FIELD_DECL,
            INTERFACE_DECL, LOCAL_VAR_DECL, METHOD_CALL_EXPR, METHOD_DECL, PARAM, RECORD_DECL,
            TYPE_PARAM,
        };

        match node.kind() {
            BLOCK_STMT => {
                // Enter a new scope for the block
                self.enter_scope();

                // Walk all children in this scope
                for child in node.children() {
                    self.walk_node(&child);
                }

                // Exit the scope when leaving the block
                self.exit_scope();
            }
            CLASS_DECL | INTERFACE_DECL | ENUM_DECL | RECORD_DECL | METHOD_DECL
            | CONSTRUCTOR_DECL => {
                let expectation = match node.kind() {
                    METHOD_DECL => self.declared_return_expectation(node),
                    CONSTRUCTOR_DECL => Some(ReturnExpectation::Nothing),
                    _ => None,
                };
                self.return_expectations.push(expectation);
                let handler = match node.kind() {
                    METHOD_DECL | CONSTRUCTOR_DECL => self.method_handler(node),
                    _ => crate::compile_checks::exceptions::Handler::Opaque,
                };
                self.handlers.push(handler);
                self.enter_scope();
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.exit_scope();
                self.handlers.pop();
                self.return_expectations.pop();
            }
            SyntaxKind::STATIC_INIT | SyntaxKind::INSTANCE_INIT => {
                self.return_expectations
                    .push(Some(ReturnExpectation::Nothing));
                self.handlers
                    .push(crate::compile_checks::exceptions::Handler::Opaque);
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.handlers.pop();
                self.return_expectations.pop();
            }
            SyntaxKind::LAMBDA_EXPR => {
                // A lambda's return type comes from its target type.
                self.return_expectations.push(None);
                let handler = self.lambda_handler(node);
                self.handlers.push(handler);
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.handlers.pop();
                self.return_expectations.pop();
            }
            SyntaxKind::TRY_STMT | SyntaxKind::TRY_WITH_RESOURCES_STMT => self.walk_try(node),
            SyntaxKind::THROW_STMT => {
                self.check_throw_statement(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::RETURN_STMT => {
                self.check_return(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::DO_WHILE_STMT
            | SyntaxKind::FOR_STMT => {
                self.check_condition(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::FOREACH_STMT => {
                self.check_foreach_iterable(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            LOCAL_VAR_DECL => self.check_variable_declaration(node),
            ASSIGNMENT_EXPR => self.check_assignment(node),
            METHOD_CALL_EXPR => self.check_method_call(node),
            SyntaxKind::NEW_EXPR => self.check_new_expr(node),
            FIELD_DECL => {
                if let Some((name, typ)) = self.extract_field_name_and_type(node) {
                    self.insert_variable(name, typ);
                }
                // Field initializers run under every constructor's handlers.
                self.handlers
                    .push(crate::compile_checks::exceptions::Handler::Opaque);
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.handlers.pop();
            }
            PARAM => {
                if let Some((name, typ)) = self.extract_param_name_and_type(node) {
                    self.insert_variable(name, typ);
                }
            }
            TYPE_PARAM => {
                if let Some(param_name) = self.extract_type_param_name(node) {
                    self.type_parameters.insert(param_name);
                }
            }
            _ => {
                // Recursively walk children
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
        }
    }

    fn collect_local_types(&mut self, root: &SyntaxNode) {
        use SyntaxKind::{ANNOTATION_DECL, CLASS_DECL, ENUM_DECL, INTERFACE_DECL, RECORD_DECL};

        for node in root.descendants() {
            match node.kind() {
                CLASS_DECL | INTERFACE_DECL | ENUM_DECL | RECORD_DECL | ANNOTATION_DECL => {
                    if let Some(name) = self.extract_type_decl_name(&node) {
                        self.local_types.insert(name);
                    }
                }
                SyntaxKind::TYPE_PARAM => {
                    if let Some(name) = self.extract_type_param_name(&node) {
                        self.type_parameters.insert(name);
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::unused_self)]
    fn extract_type_decl_name(&self, node: &SyntaxNode) -> Option<String> {
        for elem in node.children_with_tokens() {
            if let Some(token) = elem.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
        None
    }

    /// Check a variable declaration for type mismatches
    ///
    /// Validates: `String foo = 1;` → Error
    fn check_variable_declaration(&mut self, node: &SyntaxNode) {
        // Extract variable name, declared type, and initializer type
        let var_name = self.extract_variable_name(node);
        let declared_type = self.extract_declared_type(node);
        let initializer_type = self.extract_initializer_type(node);
        let initializer_node = self.initializer_expression(node);
        // With several declarators the "initializer" found here belongs to the
        // last one; judging the first variable against it would be a guess.
        let has_multiple_declarators = crate::utils::declarator_identifiers(node).len() > 1;

        // Add variable to current scope for tracking
        // For explicit types: use declared_type
        // For var declarations: infer from initializer_type
        if let Some(ref name) = &var_name {
            let type_to_track = declared_type.as_ref().or(initializer_type.as_ref());

            if let Some(tracked_type) = type_to_track {
                self.insert_variable(name.clone(), tracked_type.clone());
            }
        }

        // Check for type mismatch
        if let (Some(var_name), Some(declared), Some(actual)) =
            (var_name, declared_type, initializer_type)
        {
            // Check if the actual type can be assigned to the declared type
            // Uses workspace context when available for better type resolution
            if !has_multiple_declarators
                && self.is_definitely_not_assignable(&declared, &actual, initializer_node.as_ref())
            {
                // Special case: check for null assigned to primitive
                if matches!(actual, Type::Null) && declared.is_primitive() {
                    self.errors.push(SemanticError::new(
                        node.text_range(),
                        SemanticErrorKind::NullToPrimitive {
                            primitive_type: declared,
                            variable_name: var_name,
                        },
                    ));
                } else {
                    self.errors.push(SemanticError::new(
                        node.text_range(),
                        SemanticErrorKind::DeclarationTypeMismatch {
                            declared_type: declared,
                            actual_type: actual,
                            variable_name: var_name,
                        },
                    ));
                }
            }
        }
        // Continue walking children
        for child in node.children() {
            self.walk_node(&child);
        }
    }

    /// Check an assignment statement for type mismatches
    ///
    /// Validates: `foo = "Hey";` where `foo` was declared as `int` → Error
    ///
    /// Note: In the AST, ASSIGNMENT_EXPR only contains "= value", the LHS variable
    /// is in a sibling NAME_EXPR node. We need to look at the parent to find it.
    fn check_assignment(&mut self, node: &SyntaxNode) {
        let target_chain = self.extract_assignment_target_chain(node);
        let assigned_type = self.extract_assignment_value_type(node);
        let value_node = self.assignment_value_expression(node);

        if let (Some(assigned), Some(variable_type)) = (
            assigned_type,
            self.resolve_assignment_target_type(&target_chain),
        ) {
            let var_name = target_chain
                .last()
                .cloned()
                .unwrap_or_else(|| "<unknown>".to_string());
            // Check if the assigned type is compatible
            // Uses workspace context when available for better type resolution
            if self.is_definitely_not_assignable(&variable_type, &assigned, value_node.as_ref()) {
                // Special case: null to primitive
                if matches!(assigned, Type::Null) && variable_type.is_primitive() {
                    self.errors.push(SemanticError::new(
                        node.text_range(),
                        SemanticErrorKind::NullToPrimitive {
                            primitive_type: variable_type,
                            variable_name: var_name,
                        },
                    ));
                } else {
                    self.errors.push(SemanticError::new(
                        node.text_range(),
                        SemanticErrorKind::AssignmentTypeMismatch {
                            variable_type,
                            assigned_type: assigned,
                            variable_name: var_name,
                        },
                    ));
                }
            }
        }
        // Continue walking children
        for child in node.children() {
            self.walk_node(&child);
        }
    }

    /// Check a method call for signature mismatches
    ///
    /// Validates:
    /// - Argument count matches method signature
    /// - Argument types are compatible with parameter types
    fn check_method_call(&mut self, node: &SyntaxNode) {
        // Extract method information
        let method_name = self.extract_method_name(node);
        let receiver_type = self.extract_receiver_type(node);
        let arguments = self.extract_method_arguments(node);

        // Need method name to proceed
        let method_name = if let Some(name) = method_name {
            name
        } else {
            // Continue walking children even if we can't extract method name
            for child in node.children() {
                self.walk_node(&child);
            }
            return;
        };

        // Infer argument types for overload resolution
        let argument_types: Vec<Option<Type>> = arguments
            .iter()
            .map(|arg| self.infer_expression_type(arg))
            .collect();

        let strict_arguments: Vec<bool> = arguments
            .iter()
            .map(|argument| self.type_arguments_are_declared(argument))
            .collect();
        self.report_inapplicable_call(
            node,
            &method_name,
            receiver_type.as_ref(),
            &argument_types,
            &strict_arguments,
        );
        self.check_call_exceptions(
            node,
            &method_name,
            receiver_type.as_ref(),
            &argument_types,
            &strict_arguments,
        );

        // Continue walking children
        for child in node.children() {
            self.walk_node(&child);
        }
    }

    fn resolve_assignment_target_type(&self, chain: &[String]) -> Option<Type> {
        if chain.is_empty() {
            return None;
        }

        if chain.len() == 1 {
            return self.get_variable(&chain[0]);
        }

        self.resolve_field_chain_type(chain)
    }

    fn resolve_field_chain_type(&self, chain: &[String]) -> Option<Type> {
        let ctx = self.workspace_context.as_ref()?;
        let mut current_type = self.get_variable(&chain[0])?;

        for field_name in &chain[1..] {
            current_type = self.resolve_field_type_for_receiver(&current_type, field_name, ctx)?;
        }

        Some(current_type)
    }

    fn resolve_field_type_for_receiver(
        &self,
        receiver_type: &Type,
        field_name: &str,
        ctx: &WorkspaceContext,
    ) -> Option<Type> {
        let Type::Class(class_type) = receiver_type else {
            return None;
        };

        let receiver_fqn = Self::resolve_class_fqn(class_type, ctx);
        let receiver_raw = strip_generics(&receiver_fqn);
        crate::resolver::resolve_effective_field_for_owner(
            &ctx.workspace_index,
            &receiver_raw,
            receiver_type,
            field_name,
        )
        .map(|(_, field_type)| field_type)
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}
