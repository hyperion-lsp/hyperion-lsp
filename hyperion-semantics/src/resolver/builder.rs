// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Builder for constructing symbol resolvers from syntax trees
//!
//! This module provides the ResolverBuilder type which walks a syntax tree
//! and populates a SymbolTable with symbol definitions and scope information.

use super::SymbolResolver;
use crate::db::FileId;
use crate::symbol::{ClassType, ScopeKind, Symbol, SymbolTable, Type, Visibility};
use hyperion_parser::syntax::{AstNode, Param, SyntaxKind, SyntaxNode};
use hyperion_types::count_array_dimensions;
use hyperion_workspace::WorkspaceIndex;
use rowan::TextRange;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Debug, Clone, Copy)]
enum TypeDeclarationKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

/// Pattern variable information extracted from instanceof expressions
#[derive(Debug, Clone)]
pub(super) struct PatternVariable {
    pub(super) name: String,
    pub(super) var_type: Type,
    pub(super) start: u32,
    pub(super) end: u32,
}

/// Narrowed type information from instanceof checks (smart cast)
/// Example: `if (obj instanceof String)` narrows `obj` to `String`
#[derive(Debug, Clone)]
pub(super) struct NarrowedVariable {
    /// The variable name being narrowed
    pub(super) name: String,
    /// The narrowed type
    pub(super) narrowed_type: Type,
}

/// Builder for constructing symbol resolvers from syntax trees
pub struct ResolverBuilder {
    pub(super) resolver: SymbolResolver,
    pub(super) current_file: FileId,
    /// Optional workspace index for method resolution in var type inference
    pub(super) workspace_index: Option<Arc<WorkspaceIndex>>,
    /// Pattern variables collected from conditions (for flow-sensitive scoping)
    pub(super) pending_pattern_vars: Vec<PatternVariable>,
    /// Narrowed types collected from instanceof checks (smart cast)
    pub(super) pending_narrowings: Vec<NarrowedVariable>,
    /// Current package name (e.g., "com.example")
    pub(super) current_package: Option<String>,
    /// Stack of enclosing class names for building inner class FQNs
    pub(super) class_name_stack: Vec<String>,
    /// Cache for expression type inference to prevent O(n²) behavior
    pub(super) type_cache: RefCell<HashMap<TextRange, Type>>,
    /// Cached resolution context (imports + wildcards). Built once per file, reused for all
    /// type extractions. Imports are file-level and don't change during a single builder pass.
    pub(super) resolution_context_cache: RefCell<Option<hyperion_types::ResolutionContext>>,
    /// Member type declarations of each enclosing type, keyed by its range.
    ///
    /// Resolving a type consults the member types of every enclosing
    /// declaration. Rescanning a declaration's children each time is linear in
    /// its member count, which made whole-file resolution quadratic on large
    /// generated classes. The members of a given declaration do not change
    /// during a builder pass, so they are scanned once.
    pub(super) member_type_names_cache: RefCell<HashMap<TextRange, Arc<Vec<String>>>>,
    /// `split_fqn_for_nested_class` results: the workspace is fixed for the
    /// lifetime of one builder, and a file spells `List`/`Integer` hundreds of times.
    pub(super) fqn_split_cache: RefCell<HashMap<String, (Option<String>, String)>>,
    pub(super) symbol_exists_cache: RefCell<HashMap<String, bool>>,
}

impl ResolverBuilder {
    // =========================================================================
    // Builder & Initialization
    // =========================================================================

    /// Create a new resolver builder for the given file
    pub fn new(file_id: FileId) -> Self {
        Self {
            resolver: SymbolResolver::new(),
            current_file: file_id,
            workspace_index: None,
            pending_pattern_vars: Vec::new(),
            pending_narrowings: Vec::new(),
            current_package: None,
            class_name_stack: Vec::new(),
            type_cache: RefCell::new(HashMap::new()),
            resolution_context_cache: RefCell::new(None),
            member_type_names_cache: RefCell::new(HashMap::new()),
            fqn_split_cache: RefCell::new(HashMap::new()),
            symbol_exists_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Create a resolver builder for testing (uses dummy FileId)
    #[cfg(test)]
    pub fn new_for_testing(resolver: SymbolResolver) -> Self {
        Self {
            resolver,
            current_file: FileId::new(0),
            workspace_index: None,
            pending_pattern_vars: Vec::new(),
            pending_narrowings: Vec::new(),
            current_package: None,
            class_name_stack: Vec::new(),
            type_cache: RefCell::new(HashMap::new()),
            resolution_context_cache: RefCell::new(None),
            member_type_names_cache: RefCell::new(HashMap::new()),
            fqn_split_cache: RefCell::new(HashMap::new()),
            symbol_exists_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Create a new resolver builder with workspace index for method resolution
    pub fn with_workspace(file_id: FileId, workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self {
            resolver: SymbolResolver::new(),
            current_file: file_id,
            workspace_index: Some(workspace_index),
            pending_pattern_vars: Vec::new(),
            pending_narrowings: Vec::new(),
            current_package: None,
            class_name_stack: Vec::new(),
            type_cache: RefCell::new(HashMap::new()),
            resolution_context_cache: RefCell::new(None),
            member_type_names_cache: RefCell::new(HashMap::new()),
            fqn_split_cache: RefCell::new(HashMap::new()),
            symbol_exists_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Create builder with pre-built symbol table and workspace index for targeted var inference
    ///
    /// This is used when you have a cached symbol table (e.g., from Salsa) and need to
    /// infer var types. The symbol table provides symbol lookups during expression type
    /// inference, while the workspace index enables method return type resolution.
    ///
    /// # Example
    /// ```ignore
    /// let cached_table = db.file_symbol_table(file_id);
    /// let builder = ResolverBuilder::for_var_inference(
    ///     file_id,
    ///     (*cached_table).clone(),
    ///     workspace_index,
    /// );
    /// let var_type = builder.infer_var_type(&var_decl_node);
    /// ```
    pub fn for_var_inference(
        file_id: FileId,
        symbol_table: SymbolTable,
        workspace_index: Arc<WorkspaceIndex>,
    ) -> Self {
        // The resolver needs the index too, not just the builder: member lookups
        // such as `resolve_field_type` bail out immediately without one, so
        // `this.field` silently inferred as `Type::Unknown` in every targeted
        // inference. `build_from_syntax` hands the index over at the end of a
        // tree walk; these constructors do no walk, so they must do it here.
        let mut resolver = SymbolResolver::with_symbol_table(symbol_table);
        resolver.set_workspace_index(Arc::clone(&workspace_index));
        Self {
            resolver,
            current_file: file_id,
            workspace_index: Some(workspace_index),
            pending_pattern_vars: Vec::new(),
            pending_narrowings: Vec::new(),
            current_package: None,
            class_name_stack: Vec::new(),
            type_cache: RefCell::new(HashMap::new()),
            resolution_context_cache: RefCell::new(None),
            member_type_names_cache: RefCell::new(HashMap::new()),
            fqn_split_cache: RefCell::new(HashMap::new()),
            symbol_exists_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Create builder with cached symbol table (Arc-wrapped) for var inference without cloning
    ///
    /// This avoids the clone overhead when using Salsa-cached symbol tables.
    pub fn for_var_inference_cached(
        file_id: FileId,
        symbol_table: Arc<SymbolTable>,
        workspace_index: Arc<WorkspaceIndex>,
    ) -> Self {
        // See `for_var_inference`: the resolver needs the index for member lookups.
        let mut resolver = SymbolResolver::with_cached_table(symbol_table);
        resolver.set_workspace_index(Arc::clone(&workspace_index));
        Self {
            resolver,
            current_file: file_id,
            workspace_index: Some(workspace_index),
            pending_pattern_vars: Vec::new(),
            pending_narrowings: Vec::new(),
            current_package: None,
            class_name_stack: Vec::new(),
            type_cache: RefCell::new(HashMap::new()),
            resolution_context_cache: RefCell::new(None),
            member_type_names_cache: RefCell::new(HashMap::new()),
            fqn_split_cache: RefCell::new(HashMap::new()),
            symbol_exists_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Build symbol table from a syntax tree
    pub fn build_from_syntax(mut self, root: &SyntaxNode) -> SymbolResolver {
        self.walk_node(root);
        // Set file_id on all scopes for cross-file disambiguation
        self.resolver
            .symbol_table_mut()
            .set_all_scopes_file_id(self.current_file);
        if let Some(workspace_index) = self.workspace_index.take() {
            self.resolver.set_workspace_index(workspace_index);
        }
        self.resolver
    }

    /// Build just the symbol table from a syntax tree (for caching)
    ///
    /// This returns the SymbolTable directly without the resolver wrapper.
    /// Use this for Salsa caching where the table can be cloned and shared.
    /// To use the cached table, create a resolver via `SymbolResolver::with_symbol_table()`.
    pub fn build_symbol_table(mut self, root: &SyntaxNode) -> SymbolTable {
        self.walk_node(root);
        // Set file_id on all scopes for cross-file disambiguation
        self.resolver
            .symbol_table_mut()
            .set_all_scopes_file_id(self.current_file);
        // Unwrap the Arc - we're consuming self so this is the only reference
        Arc::try_unwrap(self.resolver.symbol_table).unwrap_or_else(|arc| (*arc).clone())
    }

    /// Walk a syntax node and populate the symbol table
    fn walk_node(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::PACKAGE_DECL => self.process_package(node),
            SyntaxKind::CLASS_DECL => {
                self.process_type_declaration(node, TypeDeclarationKind::Class);
            }
            SyntaxKind::INTERFACE_DECL => {
                self.process_type_declaration(node, TypeDeclarationKind::Interface);
            }
            SyntaxKind::ENUM_DECL => {
                self.process_type_declaration(node, TypeDeclarationKind::Enum);
            }
            SyntaxKind::RECORD_DECL => {
                self.process_type_declaration(node, TypeDeclarationKind::Record);
            }
            SyntaxKind::ANNOTATION_DECL => {
                self.process_type_declaration(node, TypeDeclarationKind::Annotation);
            }
            SyntaxKind::ENUM_CONSTANT => self.process_enum_constant(node),
            SyntaxKind::METHOD_DECL => self.process_method(node),
            SyntaxKind::CONSTRUCTOR_DECL => self.process_constructor(node),
            SyntaxKind::FIELD_DECL => self.process_field(node),
            SyntaxKind::LOCAL_VAR_DECL => self.process_local_var(node),
            SyntaxKind::PARAM => self.process_parameter(node),
            SyntaxKind::BLOCK_STMT => self.process_block(node),
            SyntaxKind::TRY_STMT => self.process_try_stmt(node),
            SyntaxKind::IF_STMT => self.process_if_stmt(node),
            SyntaxKind::FOREACH_STMT => self.process_foreach(node),
            SyntaxKind::PATTERN_INSTANCEOF_EXPR => self.collect_pattern_instanceof(node),
            SyntaxKind::SWITCH_STMT => self.process_switch_stmt(node),
            SyntaxKind::SWITCH_EXPR => self.process_switch_expr(node),
            _ => {
                // Recursively walk children
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
        }
    }

    // =========================================================================
    // AST Processing Methods
    // =========================================================================

    fn process_package(&mut self, node: &SyntaxNode) {
        // Extract package name from PACKAGE_DECL
        let mut parts = Vec::new();
        for token in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(t) = token {
                if t.kind().is_identifier() {
                    parts.push(t.text().to_string());
                }
            }
        }
        if !parts.is_empty() {
            self.current_package = Some(parts.join("."));
            tracing::debug!("Extracted package: {:?}", self.current_package);
        }
    }

    fn process_type_declaration(&mut self, node: &SyntaxNode, kind: TypeDeclarationKind) {
        if let Some((name, ident_start, ident_end)) = self.extract_type_name_with_range(node) {
            let scope_start = u32::from(node.text_range().start());
            let scope_end = u32::from(node.text_range().end());
            let parent = self.resolver.current_scope_id();
            let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
                ScopeKind::Class,
                Some(parent),
                scope_start,
                scope_end,
            );

            let class_name = if self.class_name_stack.is_empty() {
                name.clone()
            } else {
                format!("{}.{}", self.class_name_stack.join("."), name)
            };
            let class_type = Type::Class(ClassType::new(class_name, self.current_package.clone()));
            let symbol = Symbol::new(
                name.clone(),
                crate::symbol::SymbolKind::Class {
                    is_interface: matches!(kind, TypeDeclarationKind::Interface),
                    is_enum: matches!(kind, TypeDeclarationKind::Enum),
                    is_record: matches!(kind, TypeDeclarationKind::Record),
                    is_annotation: matches!(kind, TypeDeclarationKind::Annotation),
                },
                class_type,
                Visibility::Public,
                crate::symbol::SourceLocation::new(self.current_file, ident_start, ident_end),
            );
            // The type is declared in the enclosing scope. Its fields and
            // methods are declared in the class scope created above.
            self.resolver.define_symbol(symbol);
            self.resolver.scope_stack.push(scope_id);
            self.class_name_stack.push(name);

            for child in node.children() {
                if matches!(kind, TypeDeclarationKind::Record)
                    && child.kind() == SyntaxKind::PARAM_LIST
                {
                    self.process_record_components(&child);
                } else {
                    self.walk_node(&child);
                }
            }

            self.class_name_stack.pop();
            self.resolver.exit_scope();
        }
    }

    fn process_enum_constant(&mut self, node: &SyntaxNode) {
        let Some((name, start, end)) = self.extract_identifier_with_range(node) else {
            return;
        };
        let enum_type = Type::Class(ClassType::new(
            self.class_name_stack.join("."),
            self.current_package.clone(),
        ));
        let mut symbol = Symbol::new(
            name,
            crate::symbol::SymbolKind::EnumConstant {
                enum_type: enum_type.clone(),
            },
            enum_type,
            Visibility::Public,
            crate::symbol::SourceLocation::new(self.current_file, start, end),
        );
        symbol.modifiers.set(crate::symbol::Modifiers::STATIC);
        symbol.modifiers.set(crate::symbol::Modifiers::FINAL);
        self.resolver.define_symbol(symbol);

        // Constant arguments can contain lambdas and anonymous-class bodies with
        // their own definitions, so continue walking after defining the constant.
        for child in node.children() {
            self.walk_node(&child);
        }
    }

    fn process_record_components(&mut self, params: &SyntaxNode) {
        for param in params.children().filter(|n| n.kind() == SyntaxKind::PARAM) {
            let Some((name, start, end)) = self.extract_identifier_with_range(&param) else {
                continue;
            };
            let component_type = self.extract_type_from_declaration(&param);
            let mut symbol = Symbol::new(
                name,
                crate::symbol::SymbolKind::Field {
                    field_type: component_type.clone(),
                },
                component_type,
                Visibility::Private,
                crate::symbol::SourceLocation::new(self.current_file, start, end),
            );
            symbol.modifiers.set(crate::symbol::Modifiers::FINAL);
            self.resolver.define_symbol(symbol);
        }
    }

    fn process_constructor(&mut self, node: &SyntaxNode) {
        // Constructor names equal their enclosing class names. Defining one in
        // the class scope would replace the class symbol in SymbolTable's
        // name-keyed map, so create only the lexical scope here. Constructor
        // declarations themselves are indexed by the workspace extractor.
        let parent = self.resolver.current_scope_id();
        let range = node.text_range();
        let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
            ScopeKind::Method,
            Some(parent),
            u32::from(range.start()),
            u32::from(range.end()),
        );
        self.resolver.scope_stack.push(scope_id);
        for child in node.children() {
            self.walk_node(&child);
        }
        self.resolver.exit_scope();
    }

    fn process_method(&mut self, node: &SyntaxNode) {
        if let Some((name, ident_start, ident_end)) = self.extract_identifier_with_range(node) {
            // Use identifier range for the symbol location
            // BUG FIX: Define method symbol in the PARENT scope (class/interface scope)
            // BEFORE entering the method's body scope. This makes the method visible
            // to sibling methods in the same class.
            let symbol = Symbol::new(
                name,
                crate::symbol::SymbolKind::Method {
                    is_constructor: false,
                    parameters: vec![],
                    return_type: Some(Type::Unknown),
                    type_parameters: vec![],
                },
                Type::Unknown,
                Visibility::Public,
                crate::symbol::SourceLocation::new(self.current_file, ident_start, ident_end),
            );

            // Define the method symbol in the current (class) scope
            self.resolver.define_symbol(symbol);

            // Now create and enter the method's body scope
            let scope_start = u32::from(node.text_range().start());
            let scope_end = u32::from(node.text_range().end());

            let parent = self.resolver.current_scope_id();
            let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
                ScopeKind::Method,
                Some(parent),
                scope_start,
                scope_end,
            );
            self.resolver.scope_stack.push(scope_id);

            for child in node.children() {
                self.walk_node(&child);
            }

            self.resolver.exit_scope();
        }
    }

    fn process_field(&mut self, node: &SyntaxNode) {
        let base_type = self.extract_type_from_declaration(node);
        for declarator in hyperion_types::field_declarators(node) {
            let name = declarator.name;
            let start = declarator.name_range.start().into();
            let end = declarator.name_range.end().into();
            let extra = u8::try_from(declarator.extra_array_dimensions).unwrap_or(u8::MAX);
            let declared_type = match base_type.clone() {
                Type::Array {
                    element_type,
                    dimensions,
                } => Type::Array {
                    element_type,
                    dimensions: dimensions.saturating_add(extra),
                },
                ty if extra > 0 => Type::Array {
                    element_type: Box::new(ty),
                    dimensions: extra,
                },
                ty => ty,
            };

            let symbol = Symbol::new(
                name,
                crate::symbol::SymbolKind::Field {
                    field_type: declared_type.clone(),
                },
                declared_type,
                Visibility::Private,
                crate::symbol::SourceLocation::new(self.current_file, start, end),
            );

            self.resolver.define_symbol(symbol);
        }

        self.walk_initializer(node);
    }

    fn process_local_var(&mut self, node: &SyntaxNode) {
        let inferred = node
            .children()
            .any(|child| child.kind() == SyntaxKind::VAR_TYPE);
        if inferred {
            // A switch initializer can yield its arm-local variables or pattern
            // bindings. Build those scopes before inferring the enclosing var.
            // An inferred variable cannot legally refer to itself in its own
            // initializer; explicit declarations keep their existing ordering.
            self.walk_initializer(node);
        }
        let mut declarators = crate::utils::declarator_identifiers(node);
        if declarators.is_empty() {
            declarators = self
                .extract_identifier_with_range(node)
                .and_then(|_| {
                    node.children_with_tokens()
                        .find_map(|elem| elem.into_token().filter(|t| t.kind().is_identifier()))
                })
                .into_iter()
                .collect();
        }
        if !declarators.is_empty() {
            // Extract the declared type from the node
            let declared_type = self.extract_type_from_declaration(node);
            for token in declarators {
                let range = token.text_range();
                let symbol = Symbol::new(
                    token.text().to_owned(),
                    crate::symbol::SymbolKind::LocalVariable {
                        var_type: declared_type.clone(),
                        is_final: false,
                    },
                    declared_type.clone(),
                    Visibility::Public, // Local variables don't have visibility
                    crate::symbol::SourceLocation::new(
                        self.current_file,
                        u32::from(range.start()),
                        u32::from(range.end()),
                    ),
                );
                self.resolver.define_symbol(symbol);
            }
        }

        if !inferred {
            self.walk_initializer(node);
        }
    }

    /// Walk the initializer expression of a field or local variable declaration.
    ///
    /// Declarations define a symbol and then stop, but their initializers can
    /// themselves contain declarations: lambda parameters (`x` in
    /// `Function<String, String> f = x -> x.trim()`), anonymous class bodies,
    /// and switch-expression arms. Without this pass those symbols never reach
    /// the symbol table, so every use of them resolves to nothing and is
    /// reported as an undefined symbol.
    ///
    /// Type nodes are skipped: they declare no symbols, and descending into
    /// them would let a generic argument be mistaken for a declaration.
    fn walk_initializer(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            if matches!(
                child.kind(),
                SyntaxKind::CLASS_TYPE
                    | SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::ARRAY_TYPE
                    | SyntaxKind::VAR_TYPE
                    | SyntaxKind::TYPE_ARGS
                    | SyntaxKind::QUALIFIED_NAME
            ) {
                continue;
            }
            self.walk_initializer_expression(&child);
        }
    }

    /// Walk one initializer expression, looking for declarations it introduces
    /// into the *enclosing* scope.
    ///
    /// An anonymous class body is the exception. Its members belong to the
    /// anonymous type, not to the block holding the variable, and the parser
    /// emits them as direct children of the NEW_EXPR rather than wrapping them
    /// in a declaration node. Walking them here would register the anonymous
    /// `run`/`helper` as siblings of the variable being declared and displace
    /// the scope the call site resolves against. Constructor arguments are
    /// ordinary expressions and may still contain lambdas, so they are walked.
    fn walk_initializer_expression(&mut self, node: &SyntaxNode) {
        if node.kind() == SyntaxKind::NEW_EXPR {
            for child in node.children() {
                if child.kind() == SyntaxKind::ARG_LIST {
                    self.walk_node(&child);
                }
            }
            return;
        }
        self.walk_node(node);
    }

    fn process_parameter(&mut self, node: &SyntaxNode) {
        if let Some((name, start, end)) = self.extract_identifier_with_range(node) {
            // Check if this is a varargs parameter
            let param = <Param as AstNode>::cast(node.clone());
            let is_varargs = param
                .as_ref()
                .is_some_and(hyperion_parser::syntax::Param::is_varargs);

            // Extract the declared type from the node
            let mut declared_type = self.extract_type_from_declaration(node);

            // Varargs parameters are syntactic sugar for arrays
            // String... args -> String[] args
            if is_varargs {
                declared_type = Type::Array {
                    element_type: Box::new(declared_type),
                    dimensions: 1,
                };
            }

            let symbol = Symbol::new(
                name,
                crate::symbol::SymbolKind::Parameter {
                    param_type: declared_type.clone(),
                    is_varargs,
                },
                declared_type,
                Visibility::Public,
                crate::symbol::SourceLocation::new(self.current_file, start, end),
            );

            self.resolver.define_symbol(symbol);
        }
    }

    // =========================================================================
    // Pattern Matching & Flow-Sensitive Scoping
    // =========================================================================

    fn collect_pattern_instanceof(&mut self, node: &SyntaxNode) {
        // PATTERN_INSTANCEOF_EXPR structure (after Java 21+ pattern matching update):
        // - First child: the expression being checked (NAME_EXPR, etc.)
        // - instanceof keyword (token)
        // - TYPE_PATTERN
        //   - Type node (CLASS_TYPE, PRIMITIVE_TYPE, or ARRAY_TYPE)
        //   - Pattern variable identifier (IDENT token)
        //
        // For Java 16+ flow-sensitive scoping, we collect pattern variables
        // instead of immediately adding them to the symbol table.
        // They will be added to the appropriate scope by process_if_stmt.

        use rowan::NodeOrToken;

        // First, find the TYPE_PATTERN child
        let type_pattern = node
            .children()
            .find(|c| c.kind() == SyntaxKind::TYPE_PATTERN);

        let Some(type_pattern) = type_pattern else {
            // No TYPE_PATTERN found - walk children for nested expressions
            for child in node.children() {
                self.walk_node(&child);
            }
            return;
        };

        let mut pattern_type: Option<Type> = None;
        let mut pattern_var_name: Option<(String, u32, u32)> = None;

        // Walk through TYPE_PATTERN's children to find type and variable name
        for item in type_pattern.children_with_tokens() {
            match item {
                NodeOrToken::Token(token) => {
                    if token.kind().is_identifier() && pattern_type.is_some() {
                        // This is the pattern variable name (comes after the type)
                        let name = token.text().to_string();
                        let start = u32::from(token.text_range().start());
                        let end = u32::from(token.text_range().end());
                        pattern_var_name = Some((name, start, end));
                    }
                }
                NodeOrToken::Node(child) => {
                    if pattern_type.is_none() {
                        // This should be the type node
                        match child.kind() {
                            SyntaxKind::CLASS_TYPE => {
                                pattern_type = Some(self.extract_class_type(&child));
                            }
                            SyntaxKind::PRIMITIVE_TYPE => {
                                pattern_type = self.extract_primitive_type(&child);
                            }
                            SyntaxKind::ARRAY_TYPE => {
                                pattern_type = self.extract_array_type(&child);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Also process the expression being checked (first child of PATTERN_INSTANCEOF_EXPR)
        for child in node.children() {
            if child.kind() != SyntaxKind::TYPE_PATTERN {
                self.walk_node(&child);
            }
        }

        // Collect pattern variable if we found both type and name
        if let (Some(var_type), Some((name, start, end))) = (pattern_type, pattern_var_name) {
            self.pending_pattern_vars.push(PatternVariable {
                name,
                var_type,
                start,
                end,
            });
        }
    }

    fn process_if_stmt(&mut self, node: &SyntaxNode) {
        // IF_STMT structure:
        // - condition (expression that may contain PATTERN_INSTANCEOF_EXPR or INSTANCEOF_EXPR)
        // - then_block (block statement or single statement)
        // - optional else_block
        //
        // For Java 16+ pattern matching, pattern variables in the condition
        // are in scope only in the then block.
        //
        // For smart cast (traditional instanceof), type narrowings are applied
        // to the then block for better completion suggestions.
        //
        // For AND chains (Java 16+), pattern variables from LHS are visible in RHS:
        //   if (obj instanceof String s && s.length() > 10) { ... }

        // Clear any pending pattern variables and narrowings from previous processing
        self.pending_pattern_vars.clear();
        self.pending_narrowings.clear();

        // Split children into condition parts and then/else blocks
        let mut condition_nodes = Vec::new();
        let mut then_block = None;
        let mut else_block = None;

        for child in node.children() {
            match child.kind() {
                // Statement nodes indicate the then/else blocks
                SyntaxKind::BLOCK_STMT
                | SyntaxKind::IF_STMT
                | SyntaxKind::WHILE_STMT
                | SyntaxKind::FOR_STMT
                | SyntaxKind::RETURN_STMT
                | SyntaxKind::EXPR_STMT
                | SyntaxKind::BREAK_STMT
                | SyntaxKind::CONTINUE_STMT => {
                    if then_block.is_none() {
                        then_block = Some(child);
                    } else {
                        else_block = Some(child);
                    }
                }
                // Everything else is part of the condition
                _ => {
                    condition_nodes.push(child);
                }
            }
        }

        // Process all condition nodes to collect pattern variables and narrowings
        for condition_node in condition_nodes {
            self.process_condition_with_patterns(&condition_node);
        }

        // Collect the pattern variables and narrowings found in the condition
        let pattern_vars_for_then = std::mem::take(&mut self.pending_pattern_vars);
        let narrowings_for_then = std::mem::take(&mut self.pending_narrowings);

        // Process then block with pattern variables and narrowings in scope
        if let Some(then_block) = then_block {
            if then_block.kind() == SyntaxKind::BLOCK_STMT {
                // It's a block - create scope and add pattern variables
                let start = u32::from(then_block.text_range().start());
                let end = u32::from(then_block.text_range().end());

                let parent = self.resolver.current_scope_id();
                let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
                    ScopeKind::Block,
                    Some(parent),
                    start,
                    end,
                );
                self.resolver.scope_stack.push(scope_id);

                // Push narrowing and pattern variable scopes (runtime tracking)
                // Manual push/pop to allow mutable access to resolver
                self.resolver.push_narrowing_scope();
                self.resolver.push_pattern_scope();

                // Add pattern variables to this scope
                for pattern_var in pattern_vars_for_then {
                    let symbol = Symbol::new(
                        pattern_var.name.clone(),
                        crate::symbol::SymbolKind::LocalVariable {
                            var_type: pattern_var.var_type.clone(),
                            is_final: true, // Pattern variables are effectively final
                        },
                        pattern_var.var_type.clone(),
                        Visibility::Public,
                        crate::symbol::SourceLocation::new(
                            self.current_file,
                            pattern_var.start,
                            pattern_var.end,
                        ),
                    );
                    self.resolver.define_symbol(symbol);
                    // Also add to pattern variables stack for quick type lookup
                    self.resolver
                        .add_pattern_variable(pattern_var.name, pattern_var.var_type);
                }

                // Add type narrowings for smart cast
                // Store in both the resolver (for runtime) and the scope (for persistent lookup)
                for narrowing in &narrowings_for_then {
                    debug!(
                        "Applying instanceof narrowing in then-block: {} -> {:?}",
                        narrowing.name, narrowing.narrowed_type
                    );
                    // Add to resolver's runtime tracking
                    self.resolver
                        .add_narrowed_type(narrowing.name.clone(), narrowing.narrowed_type.clone());
                }

                // Also store narrowed types in the scope for persistent lookup during completion
                if let Some(scope) = self.resolver.symbol_table_mut().get_scope_mut(scope_id) {
                    for narrowing in narrowings_for_then {
                        scope.add_narrowed_type(narrowing.name, narrowing.narrowed_type);
                    }
                }

                // Process block contents
                for child in then_block.children() {
                    self.walk_node(&child);
                }

                // Manually pop scopes (order matters - opposite of push)
                self.resolver.pop_pattern_scope();
                self.resolver.pop_narrowing_scope();
                self.resolver.exit_scope();
            } else {
                // Single statement - we can't easily add narrowings to a scope here
                // since there's no block scope. For now, just use runtime tracking.
                // Manual push/pop to allow mutable access to resolver
                self.resolver.push_narrowing_scope();
                for narrowing in narrowings_for_then {
                    self.resolver
                        .add_narrowed_type(narrowing.name, narrowing.narrowed_type);
                }
                self.walk_node(&then_block);
                // Manually pop narrowing scope
                self.resolver.pop_narrowing_scope();
            }
        }

        // Process else block (pattern variables and narrowings NOT in scope)
        if let Some(else_block) = else_block {
            self.walk_node(&else_block);
        }
    }

    /// Process condition expression with pattern variable awareness
    ///
    /// This handles:
    /// - Simple pattern instanceof: `obj instanceof String s`
    /// - AND chains: `obj instanceof String s && s.length() > 10`
    /// - OR chains: `obj instanceof String s || other` (patterns NOT available)
    /// - Nested expressions
    fn process_condition_with_patterns(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::PATTERN_INSTANCEOF_EXPR => {
                // Collect the pattern variable
                self.collect_pattern_instanceof(node);
            }
            SyntaxKind::INSTANCEOF_EXPR => {
                // Collect type narrowing for smart cast (traditional instanceof without pattern var)
                self.collect_instanceof_narrowing(node);
            }
            SyntaxKind::BINARY_EXPR => {
                // Handle && and || operators specially for pattern scoping
                self.process_binary_condition(node);
            }
            _ => {
                // Regular expression - walk children recursively
                for child in node.children() {
                    self.process_condition_with_patterns(&child);
                }
            }
        }
    }

    /// Collect type narrowing from traditional instanceof expression (smart cast)
    ///
    /// For `if (obj instanceof String)`, this collects that `obj` should be
    /// narrowed to `String` in the then-block for smart completion.
    fn collect_instanceof_narrowing(&mut self, node: &SyntaxNode) {
        // INSTANCEOF_EXPR currently includes the left-hand side expression directly:
        // [LHS_EXPR, INSTANCEOF_KW, TYPE...]
        // Keep a fallback to the older shape (LHS in parent) for compatibility.
        let variable_name = self.extract_instanceof_variable_name(node);

        // Extract the type from this INSTANCEOF_EXPR node
        // AST structure for `instanceof String[]`:
        //   - CLASS_TYPE (text: "String")
        //   - ARRAY_TYPE (text: "[]")
        // They are siblings, so check for ARRAY_TYPE first and combine with element type
        let has_array_type = node
            .children()
            .any(|child| child.kind() == SyntaxKind::ARRAY_TYPE);

        let narrowed_type = if has_array_type {
            // Find the element type (CLASS_TYPE or PRIMITIVE_TYPE sibling)
            let element_type = node.children().find_map(|child| match child.kind() {
                SyntaxKind::CLASS_TYPE => Some(self.extract_class_type(&child)),
                SyntaxKind::PRIMITIVE_TYPE => self.extract_primitive_type(&child),
                _ => None,
            });

            // Count array dimensions
            let dimensions = node
                .children()
                .filter(|child| child.kind() == SyntaxKind::ARRAY_TYPE)
                .count();

            element_type.map(|elem| Type::Array {
                element_type: Box::new(elem),
                dimensions: u8::try_from(dimensions).unwrap_or(u8::MAX),
            })
        } else {
            // No array type, extract directly
            node.children().find_map(|child| match child.kind() {
                SyntaxKind::CLASS_TYPE => Some(self.extract_class_type(&child)),
                SyntaxKind::PRIMITIVE_TYPE => self.extract_primitive_type(&child),
                _ => None,
            })
        };

        // Collect narrowing if we found both variable name and type
        if let (Some(name), Some(ty)) = (&variable_name, &narrowed_type) {
            debug!("Collected instanceof narrowing: {} -> {:?}", name, ty);
            self.pending_narrowings.push(NarrowedVariable {
                name: name.clone(),
                narrowed_type: ty.clone(),
            });
        }
    }

    fn extract_instanceof_variable_name(&self, node: &SyntaxNode) -> Option<String> {
        use rowan::NodeOrToken;

        // Preferred shape: LHS is inside this INSTANCEOF_EXPR before `instanceof`.
        let mut lhs_expr: Option<SyntaxNode> = None;
        for item in node.children_with_tokens() {
            match item {
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::INSTANCEOF_KW => break,
                NodeOrToken::Node(expr) => lhs_expr = Some(expr),
                _ => {}
            }
        }

        if let Some(expr) = lhs_expr {
            if let Some(name) = Self::extract_name_from_expr(&expr) {
                return Some(name);
            }
        }

        // Backward-compatibility fallback: LHS is a sibling before INSTANCEOF_EXPR.
        let parent = node.parent()?;
        let mut variable_name: Option<String> = None;
        for child in parent.children() {
            if child.kind() == SyntaxKind::INSTANCEOF_EXPR
                && child.text_range() == node.text_range()
            {
                break;
            }
            if let Some(name) = Self::extract_name_from_expr(&child) {
                variable_name = Some(name);
            }
        }
        variable_name
    }

    fn extract_name_from_expr(node: &SyntaxNode) -> Option<String> {
        match node.kind() {
            SyntaxKind::NAME_EXPR => node.children_with_tokens().find_map(|c| {
                c.into_token()
                    .filter(|t| t.kind().is_identifier())
                    .map(|t| t.text().to_string())
            }),
            // Handle parenthesized names: `(obj) instanceof String`
            SyntaxKind::PAREN_EXPR => node
                .children()
                .find_map(|child| Self::extract_name_from_expr(&child)),
            _ => None,
        }
    }

    /// Process binary expressions in conditions (&&, ||, etc.)
    fn process_binary_condition(&mut self, node: &SyntaxNode) {
        // Find the operator token to determine how to handle patterns
        let operator = node
            .children_with_tokens()
            .find_map(rowan::NodeOrToken::into_token)
            .and_then(|t| {
                // Check if this is a binary operator
                match t.kind() {
                    SyntaxKind::AMP_AMP | SyntaxKind::PIPE_PIPE => Some(t.kind()),
                    _ => None,
                }
            });

        match operator {
            Some(SyntaxKind::AMP_AMP) => {
                // AND: pattern variables from LHS are visible in RHS
                self.process_and_condition(node);
            }
            Some(SyntaxKind::PIPE_PIPE) => {
                // OR: pattern variables NOT available in RHS or after
                // (would need both branches to define the same pattern)
                // Just walk normally without special pattern handling
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            _ => {
                // Other binary operators (==, <, !=, etc.)
                // Continue processing for nested patterns
                for child in node.children() {
                    self.process_condition_with_patterns(&child);
                }
            }
        }
    }

    /// Process AND condition with pattern variable flow-sensitive scoping
    ///
    /// For: `obj instanceof String s && s.length() > 10`
    /// - Process LHS: collect 's'
    /// - Create temporary scope with 's' for RHS evaluation
    /// - Process RHS: 's' is visible here
    /// - Exit temporary scope
    /// - Both LHS and RHS patterns added to then-block by process_if_stmt
    fn process_and_condition(&mut self, node: &SyntaxNode) {
        let mut children = node.children();

        // Process LHS - collect any pattern variables
        if let Some(lhs) = children.next() {
            self.process_condition_with_patterns(&lhs);
        }

        // Snapshot pattern variables collected from LHS
        let lhs_patterns = self.pending_pattern_vars.clone();

        // Process RHS with LHS patterns temporarily in scope
        if let Some(rhs) = children.next() {
            if lhs_patterns.is_empty() {
                // No pattern variables from LHS, process normally
                self.process_condition_with_patterns(&rhs);
            } else {
                // Create temporary scope for RHS evaluation
                // This makes LHS pattern variables visible in RHS
                self.enter_temp_pattern_scope(&lhs_patterns);
                self.process_condition_with_patterns(&rhs);
                self.exit_temp_pattern_scope();
            }
        }

        // pending_pattern_vars now contains patterns from both LHS and RHS
        // They will be added to the then-block by process_if_stmt
    }

    /// Create a temporary scope with pattern variables for RHS evaluation
    ///
    /// This allows code like: `obj instanceof String s && s.length() > 10`
    /// where 's' needs to be visible when processing `s.length() > 10`
    fn enter_temp_pattern_scope(&mut self, patterns: &[PatternVariable]) {
        // Create a temporary block scope
        let parent = self.resolver.current_scope_id();
        let temp_scope = self
            .resolver
            .symbol_table_mut()
            .create_scope(ScopeKind::Block, Some(parent));
        self.resolver.scope_stack.push(temp_scope);

        // Add pattern variables to this temporary scope
        for pattern in patterns {
            let symbol = Symbol::new(
                pattern.name.clone(),
                crate::symbol::SymbolKind::LocalVariable {
                    var_type: pattern.var_type.clone(),
                    is_final: true, // Pattern variables are effectively final
                },
                pattern.var_type.clone(),
                Visibility::Public,
                crate::symbol::SourceLocation::new(self.current_file, pattern.start, pattern.end),
            );
            self.resolver.define_symbol(symbol);
        }
    }

    /// Exit the temporary pattern variable scope
    fn exit_temp_pattern_scope(&mut self) {
        self.resolver.exit_scope();
    }

    fn process_block(&mut self, node: &SyntaxNode) {
        let start = u32::from(node.text_range().start());
        let end = u32::from(node.text_range().end());

        // Create scope with range
        let parent = self.resolver.current_scope_id();
        let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
            ScopeKind::Block,
            Some(parent),
            start,
            end,
        );
        self.resolver.scope_stack.push(scope_id);

        for child in node.children() {
            self.walk_node(&child);
        }

        self.resolver.exit_scope();
    }

    fn process_foreach(&mut self, node: &SyntaxNode) {
        // FOREACH_STMT structure (Java enhanced for loop):
        // for (Type variable : collection) { body }
        //
        // The AST contains:
        // - Type node (CLASS_TYPE, PRIMITIVE_TYPE, ARRAY_TYPE, or VAR_TYPE)
        // - IDENT token (loop variable name)
        // - Expression (the collection/iterable)
        // - Statement (loop body)
        //
        // We need to:
        // 1. Extract the loop variable type and name
        // 2. For var, infer type from the iterable expression
        // 3. Create a scope for the loop body
        // 4. Register the loop variable in that scope
        // 5. Process the loop body

        use rowan::NodeOrToken;

        let mut loop_var_type: Option<Type> = None;
        let mut loop_var_name: Option<(String, u32, u32)> = None;
        let mut loop_body: Option<SyntaxNode> = None;
        let mut iterable_expr: Option<SyntaxNode> = None;
        let mut past_colon = false;
        let mut is_var = false;

        // First pass: extract type, variable name, and iterable expression
        for item in node.children_with_tokens() {
            match item {
                NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::COLON {
                        past_colon = true;
                    } else if token.kind().is_identifier() && !past_colon {
                        // This is the loop variable name (comes after type, before colon)
                        // Allow contextual keywords (record, var, yield) as variable names
                        let name = token.text().to_string();
                        let start = u32::from(token.text_range().start());
                        let end = u32::from(token.text_range().end());
                        loop_var_name = Some((name, start, end));
                    }
                }
                NodeOrToken::Node(child) => {
                    if past_colon {
                        // After colon: extract iterable expression and loop body
                        if iterable_expr.is_none()
                            && !matches!(
                                child.kind(),
                                SyntaxKind::BLOCK_STMT
                                    | SyntaxKind::EXPR_STMT
                                    | SyntaxKind::IF_STMT
                                    | SyntaxKind::WHILE_STMT
                                    | SyntaxKind::FOR_STMT
                                    | SyntaxKind::FOREACH_STMT
                            )
                        {
                            // First expression after colon is the iterable
                            iterable_expr = Some(child);
                        } else if loop_body.is_none()
                            && matches!(
                                child.kind(),
                                SyntaxKind::BLOCK_STMT
                                    | SyntaxKind::EXPR_STMT
                                    | SyntaxKind::IF_STMT
                                    | SyntaxKind::WHILE_STMT
                                    | SyntaxKind::FOR_STMT
                                    | SyntaxKind::FOREACH_STMT
                            )
                        {
                            loop_body = Some(child);
                        }
                    } else {
                        // Before colon: extract type
                        match child.kind() {
                            SyntaxKind::VAR_TYPE => {
                                is_var = true;
                            }
                            SyntaxKind::CLASS_TYPE => {
                                loop_var_type = Some(self.extract_class_type(&child));
                            }
                            SyntaxKind::PRIMITIVE_TYPE => {
                                loop_var_type = self.extract_primitive_type(&child);
                            }
                            SyntaxKind::ARRAY_TYPE => {
                                // `Object[] row` parses as CLASS_TYPE followed by a
                                // bracket-only ARRAY_TYPE sibling; the element type is
                                // the type seen just before it. Dropping it left the
                                // loop variable undeclared ("cannot find symbol: row").
                                let brackets = child
                                    .children_with_tokens()
                                    .filter(|element| element.kind() == SyntaxKind::L_BRACKET)
                                    .count();
                                match (child.children().next(), loop_var_type.take()) {
                                    (None, Some(element)) if brackets > 0 => {
                                        loop_var_type = Some(Type::Array {
                                            element_type: Box::new(element),
                                            dimensions: u8::try_from(brackets).unwrap_or(1),
                                        });
                                    }
                                    _ => loop_var_type = self.extract_array_type(&child),
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // If var, infer type from iterable expression
        if is_var && loop_var_type.is_none() {
            if let Some(iter_expr) = &iterable_expr {
                loop_var_type = Some(self.infer_foreach_element_type(iter_expr));
            }
        }

        // Create a scope for the loop body with the loop variable
        if let (Some(var_type), Some((var_name, var_start, var_end))) =
            (loop_var_type, loop_var_name)
        {
            let start = u32::from(node.text_range().start());
            let end = u32::from(node.text_range().end());

            // Create scope for the loop body
            let parent = self.resolver.current_scope_id();
            let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
                ScopeKind::Block,
                Some(parent),
                start,
                end,
            );
            self.resolver.scope_stack.push(scope_id);

            // Register the loop variable
            let loop_var_symbol = Symbol::new(
                var_name,
                crate::symbol::SymbolKind::LocalVariable {
                    var_type: var_type.clone(),
                    is_final: true, // Foreach loop variables are effectively final
                },
                var_type,
                Visibility::Public,
                crate::symbol::SourceLocation::new(self.current_file, var_start, var_end),
            );
            self.resolver.define_symbol(loop_var_symbol);

            // Process the loop body
            if let Some(body) = loop_body {
                self.walk_node(&body);
            }

            // Exit the loop scope
            self.resolver.exit_scope();
        }
    }

    /// Infer the element type for a foreach loop variable
    /// Given an iterable expression (e.g., List<User>), extract the element type (User)
    fn infer_foreach_element_type(&self, iterable_expr: &SyntaxNode) -> Type {
        debug!("Inferring element type for foreach loop");
        // Infer the type of the iterable expression
        let iterable_type = self.infer_expression_type(iterable_expr);
        debug!("Iterable type: {:?}", iterable_type);

        // Extract element type based on the iterable type
        let element_type = match &iterable_type {
            Type::Array { element_type, .. } => {
                // For arrays, the element type is directly stored
                (**element_type).clone()
            }
            Type::Class(ct) => {
                // For generic collections (List<E>, Set<E>, Iterable<E>), extract the first type argument
                // Common collection types:
                // - java.util.List<E>
                // - java.util.Set<E>
                // - java.util.Collection<E>
                // - java.lang.Iterable<E>
                if ct.type_arguments.is_empty() {
                    // No type arguments - return raw type or Unknown
                    Type::Unknown
                } else {
                    // Return the first type argument (element type)
                    ct.type_arguments[0].clone()
                }
            }
            _ => Type::Unknown,
        };

        info!("Foreach element type inferred: {:?}", element_type);
        element_type
    }

    fn process_try_stmt(&mut self, node: &SyntaxNode) {
        // Try statements create their own scope for resources
        let start = u32::from(node.text_range().start());
        let end = u32::from(node.text_range().end());

        // Create scope with range
        let parent = self.resolver.current_scope_id();
        let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
            ScopeKind::Block,
            Some(parent),
            start,
            end,
        );
        self.resolver.scope_stack.push(scope_id);

        // Process resources in try-with-resources and catch clauses
        for child in node.children() {
            if child.kind() == SyntaxKind::RESOURCE_SPEC {
                self.process_resource_spec(&child);
            } else if child.kind() == SyntaxKind::CATCH_CLAUSE {
                self.process_catch_clause(&child);
            } else {
                self.walk_node(&child);
            }
        }

        self.resolver.exit_scope();
    }

    fn process_resource_spec(&mut self, node: &SyntaxNode) {
        // RESOURCE_SPEC contains resources declared in try-with-resources
        // Resources can be:
        // 1. LOCAL_VAR_DECL nodes (new parser, wrapped declarations)
        // 2. Loose tokens (old parser, for compatibility)

        // First try the new approach: look for LOCAL_VAR_DECL nodes
        for child in node.children() {
            if child.kind() == SyntaxKind::LOCAL_VAR_DECL {
                // Process as a regular local variable declaration
                self.process_local_var(&child);
            }
        }

        // If no LOCAL_VAR_DECL nodes found, fall back to loose token parsing
        // (for compatibility with older parser versions or edge cases)
        let has_local_var_decl = node
            .children()
            .any(|c| c.kind() == SyntaxKind::LOCAL_VAR_DECL);
        if has_local_var_decl {
            return;
        }

        // Fallback: parse loose tokens (old approach)
        use rowan::NodeOrToken;

        let mut current_type: Option<Type> = None;
        let mut pending_ident: Option<(String, u32, u32)> = None;

        for item in node.descendants_with_tokens() {
            match item {
                NodeOrToken::Node(child) => {
                    // Extract type from CLASS_TYPE, PRIMITIVE_TYPE, or ARRAY_TYPE nodes
                    if matches!(
                        child.kind(),
                        SyntaxKind::CLASS_TYPE
                            | SyntaxKind::PRIMITIVE_TYPE
                            | SyntaxKind::ARRAY_TYPE
                    ) {
                        current_type = Some(self.extract_type_from_declaration(&child));
                    }
                }
                NodeOrToken::Token(token) => {
                    if token.kind().is_identifier() {
                        // This might be the variable name
                        // Allow contextual keywords (record, var, yield) as variable names
                        let name = token.text().to_string();
                        let start = u32::from(token.text_range().start());
                        let end = u32::from(token.text_range().end());
                        pending_ident = Some((name, start, end));
                    } else if token.kind() == SyntaxKind::EQ {
                        // EQ confirms the previous IDENT was a variable declaration
                        if let (Some((name, start, end)), Some(var_type)) =
                            (pending_ident.take(), current_type.take())
                        {
                            let symbol = Symbol::new(
                                name,
                                crate::symbol::SymbolKind::LocalVariable {
                                    var_type: var_type.clone(),
                                    is_final: false,
                                },
                                var_type,
                                Visibility::Public, // Try-with-resources variables don't have visibility
                                crate::symbol::SourceLocation::new(self.current_file, start, end),
                            );
                            self.resolver.define_symbol(symbol);
                        }
                    } else if token.kind() == SyntaxKind::SEMICOLON {
                        // Reset for next resource
                        current_type = None;
                        pending_ident = None;
                    }
                }
            }
        }
    }

    fn process_catch_clause(&mut self, node: &SyntaxNode) {
        // CATCH_CLAUSE structure:
        // catch ( ExceptionType1 [| ExceptionType2 ...] paramName ) { block }
        //
        // For multi-catch (e.g., IOException | SQLException e), we extract the first
        // exception type as a simplification. All exceptions inherit from Throwable,
        // so common methods like getMessage() will resolve correctly.

        use rowan::NodeOrToken;

        let mut exception_type: Option<Type> = None;
        let mut param_name: Option<(String, u32, u32)> = None;
        let mut catch_block: Option<SyntaxNode> = None;

        // Walk direct children only to avoid descending into CLASS_TYPE nodes
        let mut seen_r_paren = false;
        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(node_child) => {
                    // Extract the first CLASS_TYPE as the exception type
                    if exception_type.is_none() && node_child.kind() == SyntaxKind::CLASS_TYPE {
                        exception_type = Some(self.extract_class_type(&node_child));
                    }
                    // Find the catch block
                    if node_child.kind() == SyntaxKind::BLOCK_STMT && catch_block.is_none() {
                        catch_block = Some(node_child);
                    }
                }
                NodeOrToken::Token(token) => {
                    // The parameter name IDENT comes after types but before R_PAREN
                    if token.kind().is_identifier()
                        && exception_type.is_some()
                        && param_name.is_none()
                        && !seen_r_paren
                    {
                        let name = token.text().to_string();
                        let start = u32::from(token.text_range().start());
                        let end = u32::from(token.text_range().end());
                        param_name = Some((name, start, end));
                    }
                    if token.kind() == SyntaxKind::R_PAREN {
                        seen_r_paren = true;
                    }
                }
            }
        }

        // Create a scope for the catch block
        if let Some(block) = catch_block {
            let start = u32::from(block.text_range().start());
            let end = u32::from(block.text_range().end());
            let parent = self.resolver.current_scope_id();
            let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
                ScopeKind::Block,
                Some(parent),
                start,
                end,
            );
            self.resolver.scope_stack.push(scope_id);

            // Define the catch parameter in the catch block scope
            if let (Some((name, start, end)), Some(param_type)) = (param_name, exception_type) {
                let symbol = Symbol::new(
                    name,
                    crate::symbol::SymbolKind::Parameter {
                        param_type: param_type.clone(),
                        is_varargs: false,
                    },
                    param_type,
                    Visibility::Public,
                    crate::symbol::SourceLocation::new(self.current_file, start, end),
                );
                self.resolver.define_symbol(symbol);
            }

            // Walk the catch block contents
            self.walk_node(&block);

            self.resolver.exit_scope();
        }
    }

    fn process_switch_stmt(&mut self, node: &SyntaxNode) {
        // SWITCH_STMT structure:
        // switch (expression) {
        //     case Pattern1 -> statement;
        //     case Pattern2 -> statement;
        //     ...
        // }
        //
        // For Java 21+ pattern matching in switch, pattern variables declared in
        // a case pattern are scoped to that case block only.

        // Walk all children - switch expression and case clauses
        for child in node.children() {
            if child.kind() == SyntaxKind::CASE_CLAUSE {
                self.process_case_clause(&child);
            } else {
                self.walk_node(&child);
            }
        }
    }

    fn process_switch_expr(&mut self, node: &SyntaxNode) {
        // SWITCH_EXPR is similar to SWITCH_STMT but returns a value
        // Same pattern variable scoping rules apply
        for child in node.children() {
            if child.kind() == SyntaxKind::CASE_CLAUSE {
                self.process_case_clause(&child);
            } else {
                self.walk_node(&child);
            }
        }
    }

    fn process_case_clause(&mut self, node: &SyntaxNode) {
        // CASE_CLAUSE structure:
        // case Pattern -> statement
        // case Pattern -> { statements }
        // case Pattern: statement; break;
        //
        // Pattern can be:
        // - TYPE_PATTERN: "Integer i"
        // - RECORD_PATTERN: "Point(int x, int y)"
        // - Literal or constant (no pattern variables)
        //
        // Pattern variables are scoped to the case block

        // Create a scope for this case
        let start = u32::from(node.text_range().start());
        let end = u32::from(node.text_range().end());
        let parent = self.resolver.current_scope_id();
        let scope_id = self.resolver.symbol_table_mut().create_scope_with_range(
            ScopeKind::Block,
            Some(parent),
            start,
            end,
        );
        self.resolver.scope_stack.push(scope_id);

        // Extract pattern variables from this case
        for child in node.children() {
            match child.kind() {
                SyntaxKind::TYPE_PATTERN => {
                    self.extract_type_pattern_variable(&child);
                }
                SyntaxKind::RECORD_PATTERN => {
                    self.extract_record_pattern_variables(&child);
                }
                _ => {
                    // Process other children (case body, etc.)
                    self.walk_node(&child);
                }
            }
        }

        self.resolver.exit_scope();
    }

    fn extract_type_pattern_variable(&mut self, node: &SyntaxNode) {
        self.extract_type_pattern_variable_with_inferred(node, None);
    }

    /// Declare the pattern variable of a `TYPE_PATTERN`.
    ///
    /// `inferred` supplies the type for a component written `var`, which the
    /// pattern itself does not state. JLS 14.30.1 takes it from the record
    /// component in the corresponding position; see
    /// [`Self::record_pattern_component_types`]. `None` means the declaring
    /// record was not found, in which case the variable is still declared --
    /// it is a real binding whose declaration site is what navigation,
    /// references and rename need -- with an unknown type.
    fn extract_type_pattern_variable_with_inferred(
        &mut self,
        node: &SyntaxNode,
        inferred: Option<Type>,
    ) {
        // TYPE_PATTERN structure:
        // Type identifier
        // Example: "String s", "Integer i", or "var x" inside a record pattern

        use rowan::NodeOrToken;

        let mut pattern_type: Option<Type> = None;
        let mut pattern_var_name: Option<(String, u32, u32)> = None;

        for item in node.children_with_tokens() {
            match item {
                NodeOrToken::Token(token) => {
                    // Variable name comes after type
                    if token.kind().is_identifier()
                        && pattern_type.is_some()
                        && pattern_var_name.is_none()
                    {
                        let name = token.text().to_string();
                        // Skip underscore patterns (unnamed patterns)
                        if name == "_" {
                            continue;
                        }
                        let start = u32::from(token.text_range().start());
                        let end = u32::from(token.text_range().end());
                        pattern_var_name = Some((name, start, end));
                    }
                }
                NodeOrToken::Node(child) => {
                    // Extract type if we haven't found it yet
                    if pattern_type.is_none() {
                        match child.kind() {
                            // `var` states no type. Before this arm existed the
                            // loop below never accepted the identifier that
                            // follows (it requires a type to have been seen
                            // first), so `case Point(var x, var y)` declared no
                            // bindings at all and `x` resolved to nothing.
                            SyntaxKind::VAR_TYPE => {
                                pattern_type = Some(inferred.clone().unwrap_or(Type::Unknown));
                            }
                            SyntaxKind::CLASS_TYPE => {
                                pattern_type = Some(self.extract_class_type(&child));
                            }
                            SyntaxKind::PRIMITIVE_TYPE => {
                                pattern_type = self.extract_primitive_type(&child);
                            }
                            SyntaxKind::ARRAY_TYPE => {
                                pattern_type = self.extract_array_type(&child);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Register pattern variable if found
        if let (Some(var_type), Some((name, start, end))) = (pattern_type, pattern_var_name) {
            let symbol = Symbol::new(
                name,
                crate::symbol::SymbolKind::LocalVariable {
                    var_type: var_type.clone(),
                    is_final: true, // Pattern variables are effectively final
                },
                var_type,
                Visibility::Public,
                crate::symbol::SourceLocation::new(self.current_file, start, end),
            );
            self.resolver.define_symbol(symbol);
        }
    }

    fn extract_record_pattern_variables(&mut self, node: &SyntaxNode) {
        // RECORD_PATTERN structure:
        // RecordType(component1, component2, ...)
        // Example: "Point(int x, int y)" or "Circle(Point(var x, var y), var r)"
        //
        // Components can be:
        // - TYPE_PATTERN: nested type pattern with variable
        // - RECORD_PATTERN: nested record pattern (recursive)
        //
        // We recursively extract all pattern variables from nested patterns

        let component_types = self.record_pattern_component_types(node);
        let mut position = 0usize;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::TYPE_PATTERN => {
                    // A nested record pattern is spelled as a TYPE_PATTERN
                    // holding only the record's type, immediately followed by
                    // the RECORD_PATTERN that holds its components. That
                    // TYPE_PATTERN binds nothing and must not consume a
                    // component position of its own.
                    let is_nested_record_head = child
                        .next_sibling()
                        .is_some_and(|next| next.kind() == SyntaxKind::RECORD_PATTERN);
                    if is_nested_record_head {
                        continue;
                    }
                    self.extract_type_pattern_variable_with_inferred(
                        &child,
                        component_types.get(position).cloned(),
                    );
                    position += 1;
                }
                SyntaxKind::RECORD_PATTERN => {
                    // Nested record pattern - recurse
                    self.extract_record_pattern_variables(&child);
                    position += 1;
                }
                _ => {}
            }
        }
    }

    /// Declared types of the components of the record a `RECORD_PATTERN`
    /// matches, in declaration order.
    ///
    /// JLS 14.30.1 infers a `var` component's type from the record component
    /// occupying the same *position*, not the one sharing its name --
    /// `Point(var a, var b)` binds `a` to `Point`'s first component whatever
    /// that component is called.
    ///
    /// The parser emits the matched type as a `TYPE_PATTERN` holding just a
    /// `CLASS_TYPE`, immediately preceding the `RECORD_PATTERN`, so the record's
    /// simple name is the previous sibling's last identifier. Only record
    /// declarations in the same compilation unit are consulted; a record
    /// declared elsewhere yields an empty vector and its `var` components are
    /// declared with an unknown type rather than a guessed one.
    fn record_pattern_component_types(&self, record_pattern: &SyntaxNode) -> Vec<Type> {
        let Some(record_name) = record_pattern
            .prev_sibling()
            .filter(|sibling| sibling.kind() == SyntaxKind::TYPE_PATTERN)
            .and_then(|pattern| {
                pattern
                    .children()
                    .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            })
            .and_then(|class_type| {
                // A CLASS_TYPE holds its name in a nested QUALIFIED_NAME, so the
                // identifier tokens are not direct children. Stop at TYPE_ARGS so
                // `Box<Point>` yields `Box`, not `Point`.
                class_type
                    .descendants_with_tokens()
                    .take_while(|element| element.kind() != SyntaxKind::TYPE_ARGS)
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| token.kind().is_identifier())
                    .last()
                    .map(|token| token.text().to_string())
            })
        else {
            return Vec::new();
        };

        let Some(root) = record_pattern.ancestors().last() else {
            return Vec::new();
        };
        root.descendants()
            .filter(|node| node.kind() == SyntaxKind::RECORD_DECL)
            .find(|node| {
                self.extract_type_name_with_range(node)
                    .is_some_and(|(name, _, _)| name == record_name)
            })
            .and_then(|declaration| {
                declaration
                    .children()
                    .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
            })
            .map(|params| {
                params
                    .children()
                    .filter(|param| param.kind() == SyntaxKind::PARAM)
                    .map(|param| self.extract_type_from_declaration(&param))
                    .collect()
            })
            .unwrap_or_default()
    }

    // =========================================================================
    // Utility Methods
    // =========================================================================

    /// Extract the first identifier from a node
    /// Includes contextual keywords (record, var, yield) which can be used as identifiers
    #[allow(clippy::unused_self)]
    pub(super) fn extract_identifier(&self, node: &SyntaxNode) -> Option<String> {
        node.children_with_tokens().find_map(|elem| {
            elem.into_token()
                .filter(|t| t.kind().is_identifier())
                .map(|t| t.text().to_string())
        })
    }

    /// Extract the first identifier token and its range from a node
    /// Includes contextual keywords (record, var, yield) which can be used as identifiers
    #[allow(clippy::unused_self)]
    pub(super) fn extract_identifier_with_range(
        &self,
        node: &SyntaxNode,
    ) -> Option<(String, u32, u32)> {
        node.children_with_tokens().find_map(|elem| {
            elem.into_token()
                .filter(|t| t.kind().is_identifier())
                .map(|t| {
                    let range = t.text_range();
                    (
                        t.text().to_string(),
                        u32::from(range.start()),
                        u32::from(range.end()),
                    )
                })
        })
    }

    /// Extract a declared class/interface/enum/record/annotation name.
    ///
    /// Unlike variable identifiers, a type declaration name must be an IDENT
    /// token. Restricted keywords such as `record` are classified as usable
    /// identifiers in expression contexts and must not be mistaken for the
    /// declaration name here.
    fn extract_type_name_with_range(&self, node: &SyntaxNode) -> Option<(String, u32, u32)> {
        node.children_with_tokens().find_map(|elem| {
            elem.into_token()
                .filter(|token| token.kind() == SyntaxKind::IDENT)
                .map(|token| {
                    let range = token.text_range();
                    (
                        token.text().to_string(),
                        u32::from(range.start()),
                        u32::from(range.end()),
                    )
                })
        })
    }

    /// Extract type from a declaration node (variable, field, or parameter)
    pub(crate) fn extract_type_from_declaration(&self, node: &SyntaxNode) -> Type {
        // Collect all children to detect array types
        let children: Vec<_> = node.children().collect();

        // Check if this is an array type (has ARRAY_TYPE child)
        let has_array_type = children.iter().any(|c| c.kind() == SyntaxKind::ARRAY_TYPE);

        if has_array_type {
            // For parameters like "User[] users", we have CLASS_TYPE + ARRAY_TYPE as siblings
            // Find the element type from CLASS_TYPE or PRIMITIVE_TYPE
            let mut element_type: Option<Type> = None;
            let mut dimensions = 0u8;

            for child in &children {
                match child.kind() {
                    SyntaxKind::CLASS_TYPE => {
                        element_type = Some(self.extract_class_type(child));
                    }
                    SyntaxKind::PRIMITIVE_TYPE => {
                        element_type = self.extract_primitive_type(child);
                    }
                    SyntaxKind::ARRAY_TYPE => {
                        // Count dimensions from brackets
                        dimensions +=
                            u8::try_from(count_array_dimensions(child)).unwrap_or(u8::MAX);
                    }
                    _ => {}
                }
            }

            if let Some(elem) = element_type {
                if dimensions == 0 {
                    dimensions = 1; // Default to 1 dimension if brackets weren't counted
                }
                return Type::Array {
                    element_type: Box::new(elem),
                    dimensions,
                };
            }
        }

        // No array type, process normally
        for child in children {
            match child.kind() {
                SyntaxKind::VAR_TYPE => {
                    // This is a 'var' type (Java 10+) - infer from initializer
                    return self.infer_var_type(node);
                }
                SyntaxKind::CLASS_TYPE => {
                    return self.extract_class_type(&child);
                }
                SyntaxKind::PRIMITIVE_TYPE => {
                    // Extract primitive type
                    if let Some(ty) = self.extract_primitive_type(&child) {
                        return ty;
                    }
                }
                _ => {}
            }
        }
        Type::Unknown
    }
}
