// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Symbol Table Builder
//!
//! Builds a SymbolTable for Kotlin files using the same infrastructure as Java.
//! This enables:
//! - Local variable navigation
//! - Type inference
//! - Semantic analysis
//!
//! Unlike the separate `hyperion_kotlin::ScopeGraph`, this integrates with the
//! shared semantics infrastructure for cross-language consistency.

use crate::db::FileId;
use crate::symbol::{
    ClassType, PrimitiveType, ScopeId, ScopeKind, SourceLocation, Symbol, SymbolKind, SymbolTable,
    Type, Visibility,
};
use hyperion_kotlin::{SyntaxKind as KtKind, SyntaxNode as KtSyntaxNode};
use tracing::debug;

/// The name token of a `PARAMETER` node.
///
/// Modifier keywords such as `vararg`, `override`, `open` or `noinline` are
/// also accepted as identifiers, so the *first* identifier-like token is wrong
/// for `vararg caCerts: X509Certificate`. The name is the last one before the
/// `:` (or the default value's `=`).
pub(crate) fn parameter_name_token(
    node: &hyperion_kotlin::SyntaxNode,
) -> Option<hyperion_kotlin::SyntaxToken> {
    let mut name = None;
    for element in node.children_with_tokens() {
        let rowan::NodeOrToken::Token(token) = element else {
            continue;
        };
        if matches!(token.kind(), KtKind::COLON | KtKind::EQ) {
            break;
        }
        if is_identifier_token(&token) {
            name = Some(token);
        }
    }
    name
}

/// Kotlin permits contextual/soft keywords in identifier positions.
pub(crate) fn is_identifier_token(token: &hyperion_kotlin::SyntaxToken) -> bool {
    matches!(
        token.kind(),
        KtKind::IDENT
            | KtKind::BACKTICK_IDENT
            | KtKind::DATA_KW
            | KtKind::SEALED_KW
            | KtKind::CATCH_KW
            | KtKind::FINALLY_KW
            | KtKind::CONSTRUCTOR_KW
            | KtKind::INIT_KW
            | KtKind::BY_KW
            | KtKind::EXPECT_KW
            | KtKind::ACTUAL_KW
            | KtKind::COMPANION_KW
            | KtKind::SUSPEND_KW
            | KtKind::INLINE_KW
            | KtKind::NOINLINE_KW
            | KtKind::CROSSINLINE_KW
            | KtKind::OPERATOR_KW
            | KtKind::INFIX_KW
            | KtKind::LATEINIT_KW
            | KtKind::VARARG_KW
            | KtKind::OPEN_KW
            | KtKind::ABSTRACT_KW
            | KtKind::OVERRIDE_KW
            | KtKind::TYPEALIAS_KW
            | KtKind::REIFIED_KW
    )
}

/// Kotlin symbol table builder
///
/// Walks the Kotlin AST and builds a SymbolTable with proper scope hierarchy.
/// Similar to Java's ResolverBuilder but adapted for Kotlin syntax.
pub struct KotlinResolverBuilder {
    /// The symbol table being built
    table: SymbolTable,
    /// Current scope stack (innermost scope is last)
    scope_stack: Vec<ScopeId>,
    /// The file being processed
    file_id: FileId,
    /// Current package name
    current_package: Option<String>,
}

impl KotlinResolverBuilder {
    /// Create a new Kotlin resolver builder
    pub fn new(file_id: FileId) -> Self {
        let table = SymbolTable::new();
        let root_scope = table.root_scope();

        Self {
            table,
            scope_stack: vec![root_scope],
            file_id,
            current_package: None,
        }
    }

    /// Build the symbol table from a Kotlin syntax tree
    pub fn build(mut self, root: &KtSyntaxNode) -> SymbolTable {
        self.walk_node(root);

        // Set file_id on all scopes
        self.table.set_all_scopes_file_id(self.file_id);

        self.table
    }

    /// Get the current scope ID
    fn current_scope(&self) -> ScopeId {
        *self
            .scope_stack
            .last()
            .expect("Scope stack should never be empty")
    }

    /// Enter a new scope
    fn enter_scope(&mut self, kind: ScopeKind, node: &KtSyntaxNode) -> ScopeId {
        let parent = self.current_scope();
        let range = node.text_range();
        let scope_id = self.table.create_scope_with_range(
            kind,
            Some(parent),
            range.start().into(),
            range.end().into(),
        );
        self.scope_stack.push(scope_id);
        scope_id
    }

    /// Exit the current scope
    fn exit_scope(&mut self) {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
        }
    }

    /// Define a symbol in the current scope
    fn define_symbol(&mut self, symbol: Symbol) {
        let scope_id = self.current_scope();
        self.table.add_symbol(symbol, scope_id);
    }

    /// Walk the AST and build the symbol table
    fn walk_node(&mut self, node: &KtSyntaxNode) {
        match node.kind() {
            KtKind::PACKAGE_DECL => self.process_package(node),
            KtKind::CLASS_DECL => self.process_class(node),
            KtKind::INTERFACE_DECL => self.process_interface(node),
            KtKind::OBJECT_DECL => self.process_object(node),
            KtKind::FUNCTION_DECL => self.process_function(node),
            KtKind::PROPERTY_DECL => self.process_property(node),
            KtKind::LAMBDA_EXPR => self.process_lambda(node),
            KtKind::BLOCK => self.process_block(node),
            _ => {
                // Walk children for unhandled nodes
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
        }
    }

    /// Process a package declaration
    fn process_package(&mut self, node: &KtSyntaxNode) {
        let mut parts = Vec::new();
        for token in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if is_identifier_token(&token) {
                    parts.push(token.text().to_string());
                }
            }
        }

        if !parts.is_empty() {
            self.current_package = Some(parts.join("."));
            debug!("KT: Set package to {:?}", self.current_package);
        }

        // Continue walking children
        for child in node.children() {
            self.walk_node(&child);
        }
    }

    /// Process a class declaration
    fn process_class(&mut self, node: &KtSyntaxNode) {
        // Extract class name
        let Some((class_name, name_start, name_end)) = self.extract_class_name(node) else {
            debug!("KT: Skipping class without name");
            return;
        };

        debug!("KT: Processing class: {}", class_name);

        // Enter class scope
        let _class_scope = self.enter_scope(ScopeKind::Class, node);

        // Build ClassType
        let (simple_name, package) = self.split_name(&class_name);
        let class_type = Type::Class(ClassType {
            name: simple_name.clone(),
            package,
            type_arguments: vec![],
            is_diamond: false,
        });

        let location = SourceLocation::new(self.file_id, name_start, name_end);

        let symbol = Symbol::new(
            simple_name,
            SymbolKind::Class {
                is_interface: false,
                is_enum: false,
                is_record: false,
                is_annotation: false,
            },
            class_type,
            self.extract_visibility(node),
            location,
        );

        self.define_symbol(symbol);

        // Primary-constructor parameters: `val`/`var` ones are properties of the
        // class (fields for name resolution), plain ones are visible to
        // property initializers and `init` blocks.
        for list in node
            .children()
            .filter(|child| child.kind() == KtKind::PARAMETER_LIST)
        {
            for parameter in list
                .children()
                .filter(|child| child.kind() == KtKind::PARAMETER)
            {
                self.process_constructor_parameter(&parameter);
            }
        }

        // Walk children (properties, methods, nested classes)
        for child in node.children() {
            if child.kind() != KtKind::PARAMETER_LIST {
                self.walk_node(&child);
            }
        }

        // Exit class scope
        self.exit_scope();
    }

    fn process_constructor_parameter(&mut self, node: &KtSyntaxNode) {
        let Some((name, start, end)) = self.extract_parameter_name(node) else {
            return;
        };
        let is_property = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .any(|token| matches!(token.kind(), KtKind::VAL_KW | KtKind::VAR_KW));
        let is_val = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .any(|token| token.kind() == KtKind::VAL_KW);
        let ty = self.extract_parameter_type(node).unwrap_or(Type::Unknown);
        let location = SourceLocation::new(self.file_id, start, end);
        let kind = if is_property {
            SymbolKind::Field {
                field_type: ty.clone(),
            }
        } else {
            SymbolKind::Parameter {
                param_type: ty.clone(),
                is_varargs: false,
            }
        };
        let visibility = if node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .any(|token| token.kind() == KtKind::PRIVATE_KW)
        {
            Visibility::Private
        } else {
            Visibility::Public
        };
        let mut symbol = Symbol::new(name, kind, ty, visibility, location);
        if is_property && is_val {
            symbol.modifiers.set(crate::symbol::Modifiers::FINAL);
        }
        self.define_symbol(symbol);
    }

    /// Process an interface declaration
    fn process_interface(&mut self, node: &KtSyntaxNode) {
        let Some((interface_name, name_start, name_end)) = self.extract_interface_name(node) else {
            return;
        };

        debug!("KT: Processing interface: {}", interface_name);

        let _interface_scope = self.enter_scope(ScopeKind::Class, node);

        let (simple_name, package) = self.split_name(&interface_name);
        let interface_type = Type::Class(ClassType {
            name: simple_name.clone(),
            package,
            type_arguments: vec![],
            is_diamond: false,
        });

        let location = SourceLocation::new(self.file_id, name_start, name_end);

        let symbol = Symbol::new(
            simple_name,
            SymbolKind::Class {
                is_interface: true,
                is_enum: false,
                is_record: false,
                is_annotation: false,
            },
            interface_type,
            self.extract_visibility(node),
            location,
        );

        self.define_symbol(symbol);

        // Walk children
        for child in node.children() {
            self.walk_node(&child);
        }

        self.exit_scope();
    }

    /// Process an object declaration
    fn process_object(&mut self, node: &KtSyntaxNode) {
        let Some((object_name, name_start, name_end)) = self.extract_object_name(node) else {
            return;
        };

        debug!("KT: Processing object: {}", object_name);

        let _object_scope = self.enter_scope(ScopeKind::Class, node);

        let (simple_name, package) = self.split_name(&object_name);
        let object_type = Type::Class(ClassType {
            name: simple_name.clone(),
            package,
            type_arguments: vec![],
            is_diamond: false,
        });

        let location = SourceLocation::new(self.file_id, name_start, name_end);

        let symbol = Symbol::new(
            simple_name,
            SymbolKind::Class {
                is_interface: false,
                is_enum: false,
                is_record: false,
                is_annotation: false,
            },
            object_type,
            self.extract_visibility(node),
            location,
        );

        self.define_symbol(symbol);

        // Walk children
        for child in node.children() {
            self.walk_node(&child);
        }

        self.exit_scope();
    }

    /// Process a function declaration
    fn process_function(&mut self, node: &KtSyntaxNode) {
        let Some((func_name, name_start, name_end)) = self.extract_function_name(node) else {
            return;
        };

        debug!("KT: Processing function: {}", func_name);

        // Enter function scope
        let _func_scope = self.enter_scope(ScopeKind::Method, node);

        let location = SourceLocation::new(self.file_id, name_start, name_end);

        // Extract return type (if specified)
        let return_type = self.extract_return_type(node);

        let symbol = Symbol::new(
            func_name,
            SymbolKind::Method {
                is_constructor: false,
                parameters: vec![],
                return_type: Some(return_type.unwrap_or(Type::Unknown)),
                type_parameters: vec![],
            },
            Type::Unknown,
            self.extract_visibility(node),
            location,
        );

        self.define_symbol(symbol);

        // Process parameters
        self.process_function_parameters(node);

        // Walk function body
        for child in node.children() {
            if child.kind() == KtKind::BLOCK {
                self.walk_node(&child);
            }
        }

        self.exit_scope();
    }

    /// Process function parameters
    fn process_function_parameters(&mut self, node: &KtSyntaxNode) {
        // Only this declaration's own parameter list: `descendants()` would also
        // pull in the parameters of nested functions and anonymous objects and
        // define them in the enclosing scope, shadowing real members.
        for list in node
            .children()
            .filter(|child| child.kind() == KtKind::PARAMETER_LIST)
        {
            for child in list
                .children()
                .filter(|child| child.kind() == KtKind::PARAMETER)
            {
                self.process_parameter(&child);
            }
        }
    }

    /// Process a parameter
    fn process_parameter(&mut self, node: &KtSyntaxNode) {
        let Some((param_name, name_start, name_end)) = self.extract_parameter_name(node) else {
            return;
        };

        debug!("KT: Processing parameter: {}", param_name);

        let location = SourceLocation::new(self.file_id, name_start, name_end);

        let param_type = self.extract_parameter_type(node).unwrap_or(Type::Unknown);

        let symbol = Symbol::new(
            param_name,
            SymbolKind::Parameter {
                param_type: param_type.clone(),
                is_varargs: node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .any(|token| token.kind() == KtKind::VARARG_KW),
            },
            param_type,
            Visibility::Public,
            location,
        );

        self.define_symbol(symbol);
    }

    /// Process a property declaration
    fn process_property(&mut self, node: &KtSyntaxNode) {
        let Some((prop_name, name_start, name_end)) = self.extract_property_name(node) else {
            return;
        };

        debug!("KT: Processing property: {}", prop_name);

        let location = SourceLocation::new(self.file_id, name_start, name_end);

        // Extract property type
        let prop_type = self.extract_property_type(node).unwrap_or(Type::Unknown);

        // Determine if this is a field or local variable based on scope
        let current_scope_kind = self
            .table
            .get_scope(self.current_scope())
            .map_or(ScopeKind::Package, |s| s.kind);

        let (kind, visibility) = match current_scope_kind {
            ScopeKind::Class => (
                SymbolKind::Field {
                    field_type: prop_type.clone(),
                },
                self.extract_visibility(node),
            ),
            _ => (
                SymbolKind::LocalVariable {
                    var_type: prop_type.clone(),
                    is_final: self.is_val(node),
                },
                Visibility::Public,
            ),
        };

        let mut symbol = Symbol::new(prop_name, kind, prop_type, visibility, location);
        if self.is_val(node) {
            symbol.modifiers.set(crate::symbol::Modifiers::FINAL);
        }

        self.define_symbol(symbol);

        // Walk initializer if present
        for child in node.children() {
            if child.kind() != KtKind::TYPE_REF {
                self.walk_node(&child);
            }
        }
    }

    /// Process a lambda expression
    fn process_lambda(&mut self, node: &KtSyntaxNode) {
        debug!("KT: Processing lambda");

        // Enter lambda scope
        let _lambda_scope = self.enter_scope(ScopeKind::Block, node);

        // Check if lambda has explicit parameters
        let mut has_explicit_params = false;
        for child in node.children() {
            if child.kind() == KtKind::LAMBDA_PARAMETERS {
                self.process_lambda_parameters(&child);
                has_explicit_params = true;
            }
        }

        // If no explicit parameters, inject implicit 'it' parameter
        if !has_explicit_params {
            debug!("KT: Lambda has no explicit parameters, injecting implicit 'it'");
            self.inject_implicit_it_parameter(node);
        }

        // Walk lambda body
        for child in node.children() {
            if child.kind() != KtKind::LAMBDA_PARAMETERS {
                self.walk_node(&child);
            }
        }

        self.exit_scope();
    }

    /// Process lambda parameters
    fn process_lambda_parameters(&mut self, node: &KtSyntaxNode) {
        for child in node.children() {
            self.process_lambda_parameter(&child);
        }
    }

    /// Process a lambda parameter
    fn process_lambda_parameter(&mut self, node: &KtSyntaxNode) {
        let Some((param_name, name_start, name_end)) = self.extract_lambda_parameter_name(node)
        else {
            return;
        };

        debug!("KT: Processing lambda parameter: {}", param_name);

        let location = SourceLocation::new(self.file_id, name_start, name_end);

        let symbol = Symbol::new(
            param_name,
            SymbolKind::Parameter {
                param_type: Type::Unknown,
                is_varargs: false,
            },
            Type::Unknown,
            Visibility::Public,
            location,
        );

        self.define_symbol(symbol);
    }

    /// Inject implicit 'it' parameter for lambdas without explicit parameters
    fn inject_implicit_it_parameter(&mut self, lambda_node: &KtSyntaxNode) {
        // Get the text range of the lambda for the source location
        let range = lambda_node.text_range();
        let start = u32::from(range.start());
        let end = u32::from(range.end());

        let location = SourceLocation::new(self.file_id, start, end);

        // For now, create 'it' with Unknown type
        // Type inference will be added in the next step
        let symbol = Symbol::new(
            "it".into(),
            SymbolKind::Parameter {
                param_type: Type::Unknown,
                is_varargs: false,
            },
            Type::Unknown,
            Visibility::Public,
            location,
        );

        debug!("KT: Injected implicit 'it' parameter");
        self.define_symbol(symbol);
    }

    /// Process a block
    fn process_block(&mut self, node: &KtSyntaxNode) {
        debug!("KT: Processing block");

        // Enter block scope
        let _block_scope = self.enter_scope(ScopeKind::Block, node);

        // Walk children
        for child in node.children() {
            self.walk_node(&child);
        }

        self.exit_scope();
    }

    // =========================================================================
    // Helper methods for extracting names and types
    //
    // These methods take &self for consistent method-call syntax on the builder,
    // even though some don't access builder state. They operate on AST nodes
    // and logically belong to the builder since they're only meaningful during
    // symbol table construction.
    // =========================================================================

    #[allow(clippy::unused_self)]
    fn extract_class_name(&self, node: &KtSyntaxNode) -> Option<(String, u32, u32)> {
        let mut seen_class_kw = false;
        for token in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if token.kind() == KtKind::CLASS_KW {
                    seen_class_kw = true;
                } else if seen_class_kw && is_identifier_token(&token) {
                    let name = token.text().to_string();
                    let start = u32::from(token.text_range().start());
                    let end = u32::from(token.text_range().end());
                    return Some((name, start, end));
                }
            }
        }
        None
    }

    #[allow(clippy::unused_self)]
    fn extract_interface_name(&self, node: &KtSyntaxNode) -> Option<(String, u32, u32)> {
        let mut seen_interface_kw = false;
        for token in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if token.kind() == KtKind::INTERFACE_KW {
                    seen_interface_kw = true;
                } else if seen_interface_kw && is_identifier_token(&token) {
                    let name = token.text().to_string();
                    let start = u32::from(token.text_range().start());
                    let end = u32::from(token.text_range().end());
                    return Some((name, start, end));
                }
            }
        }
        None
    }

    fn extract_object_name(&self, node: &KtSyntaxNode) -> Option<(String, u32, u32)> {
        // Objects can be anonymous (companion object) or named
        let mut seen_object_kw = false;
        for token in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if token.kind() == KtKind::OBJECT_KW {
                    seen_object_kw = true;
                } else if seen_object_kw && is_identifier_token(&token) {
                    let name = token.text().to_string();
                    let start = u32::from(token.text_range().start());
                    let end = u32::from(token.text_range().end());
                    return Some((name, start, end));
                }
            }
        }

        // If no name found, check if it's a companion object
        if self.has_modifier(node, "companion") {
            let start = u32::from(node.text_range().start());
            let end = u32::from(node.text_range().end());
            return Some(("Companion".to_string(), start, end));
        }

        None
    }

    #[allow(clippy::unused_self)]
    fn extract_function_name(&self, node: &KtSyntaxNode) -> Option<(String, u32, u32)> {
        let mut seen_fun_kw = false;
        for token in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if token.kind() == KtKind::FUN_KW {
                    seen_fun_kw = true;
                } else if seen_fun_kw && is_identifier_token(&token) {
                    let name = token.text().to_string();
                    let start = u32::from(token.text_range().start());
                    let end = u32::from(token.text_range().end());
                    return Some((name, start, end));
                }
            }
        }
        None
    }

    #[allow(clippy::unused_self)]
    fn extract_property_name(&self, node: &KtSyntaxNode) -> Option<(String, u32, u32)> {
        let mut seen_val_var = false;
        for token in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if matches!(token.kind(), KtKind::VAL_KW | KtKind::VAR_KW) {
                    seen_val_var = true;
                } else if seen_val_var && is_identifier_token(&token) {
                    let name = token.text().to_string();
                    let start = u32::from(token.text_range().start());
                    let end = u32::from(token.text_range().end());
                    return Some((name, start, end));
                }
            }
        }
        None
    }

    #[allow(clippy::unused_self)]
    fn extract_parameter_name(&self, node: &KtSyntaxNode) -> Option<(String, u32, u32)> {
        let token = parameter_name_token(node)?;
        Some((
            token.text().to_string(),
            u32::from(token.text_range().start()),
            u32::from(token.text_range().end()),
        ))
    }

    #[allow(clippy::unused_self)]
    fn extract_lambda_parameter_name(&self, node: &KtSyntaxNode) -> Option<(String, u32, u32)> {
        for token in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if is_identifier_token(&token) {
                    let name = token.text().to_string();
                    let start = u32::from(token.text_range().start());
                    let end = u32::from(token.text_range().end());
                    return Some((name, start, end));
                }
            }
        }
        None
    }

    fn extract_return_type(&self, node: &KtSyntaxNode) -> Option<Type> {
        for child in node.children() {
            if child.kind() == KtKind::TYPE_REF {
                return self.extract_type_from_type_ref(&child);
            }
        }
        None
    }

    fn extract_parameter_type(&self, node: &KtSyntaxNode) -> Option<Type> {
        for child in node.children() {
            if child.kind() == KtKind::TYPE_REF {
                return self.extract_type_from_type_ref(&child);
            }
        }
        None
    }

    fn extract_property_type(&self, node: &KtSyntaxNode) -> Option<Type> {
        for child in node.children() {
            if child.kind() == KtKind::TYPE_REF {
                return self.extract_type_from_type_ref(&child);
            }
        }

        // No annotation: recover the type of a constructor initializer
        // (`val sb = StringBuilder()`). Without this, every local `val` in the
        // symbol table is `Unknown`, so anything resolving through the table -
        // notably the lambda-receiver and lambda-parameter inference in
        // `KotlinTypeInferenceEngine` - goes blind on locals.
        Self::extract_constructor_initializer_type(node)
    }

    /// Type of a `val x = Foo(...)` initializer: an uppercase callee applied
    /// directly to an argument list names its own type.
    ///
    /// Deliberately narrow. Anything else (`foo()`, `a.b()`, literals, `when`)
    /// needs real inference and is left `Unknown` rather than guessed at.
    fn extract_constructor_initializer_type(node: &KtSyntaxNode) -> Option<Type> {
        let mut after_eq = node
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| !token.kind().is_trivia())
            .skip_while(|token| token.kind() != KtKind::EQ)
            .skip(1);

        let callee = after_eq.next()?;
        if callee.kind() != KtKind::IDENT
            || !callee.text().starts_with(char::is_uppercase)
            || after_eq.next()?.kind() != KtKind::L_PAREN
        {
            return None;
        }

        Some(Type::Class(ClassType {
            name: callee.text().to_string(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        }))
    }

    fn extract_type_from_type_ref(&self, type_ref: &KtSyntaxNode) -> Option<Type> {
        kotlin_type_from_text(
            &type_ref.text().to_string(),
            self.current_package.as_deref(),
        )
    }

    fn extract_visibility(&self, node: &KtSyntaxNode) -> Visibility {
        // Check for visibility modifiers in the node or its siblings
        if let Some(parent) = node.parent() {
            for sibling in parent.children() {
                if sibling.kind() == KtKind::MODIFIERS {
                    return self.extract_visibility_from_modifiers(&sibling);
                }
                if sibling == *node {
                    break;
                }
            }
        }

        // Default visibility in Kotlin is public
        Visibility::Public
    }

    #[allow(clippy::unused_self)]
    fn extract_visibility_from_modifiers(&self, modifiers: &KtSyntaxNode) -> Visibility {
        for token in modifiers.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                match token.kind() {
                    KtKind::PRIVATE_KW => return Visibility::Private,
                    KtKind::PROTECTED_KW => return Visibility::Protected,
                    KtKind::INTERNAL_KW => return Visibility::PackagePrivate,
                    KtKind::PUBLIC_KW => return Visibility::Public,
                    _ => {}
                }
            }
        }
        Visibility::Public
    }

    #[allow(clippy::unused_self)]
    fn has_modifier(&self, node: &KtSyntaxNode, modifier: &str) -> bool {
        if let Some(parent) = node.parent() {
            for sibling in parent.children() {
                if sibling.kind() == KtKind::MODIFIERS {
                    for token in sibling.descendants_with_tokens() {
                        if let rowan::NodeOrToken::Token(token) = token {
                            if token.text() == modifier {
                                return true;
                            }
                        }
                    }
                }
                if sibling == *node {
                    break;
                }
            }
        }
        false
    }

    #[allow(clippy::unused_self)]
    fn is_val(&self, node: &KtSyntaxNode) -> bool {
        for token in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = token {
                if token.kind() == KtKind::VAL_KW {
                    return true;
                }
            }
        }
        false
    }

    /// Split a name into simple name and package
    ///
    /// Examples:
    /// - "com.example.Foo" -> ("Foo", Some("com.example"))
    /// - "Foo" -> ("Foo", current_package)
    fn split_name(&self, name: &str) -> (String, Option<String>) {
        name.rfind('.').map_or_else(
            // No package in the name, use current package
            || (name.to_string(), self.current_package.clone()),
            |last_dot| {
                let simple_name = name[last_dot + 1..].to_string();
                let package = Some(name[..last_dot].to_string());
                (simple_name, package)
            },
        )
    }
}

/// The type a Kotlin type reference's text writes, as the symbol table
/// records a declared type in a file of `current_package`. Shared with
/// expression inference, so a constructor call's type arguments read as a
/// declared type's do.
pub(crate) fn kotlin_type_from_text(text: &str, current_package: Option<&str>) -> Option<Type> {
    let text = text.trim().strip_suffix('?').unwrap_or(text.trim());
    let parsed = crate::type_parser::parse_type_string(text)?;
    Some(normalize_kotlin_type(parsed, current_package))
}

/// Apply Kotlin primitive and default-package semantics while preserving
/// the generic structure produced by the shared type parser.
fn normalize_kotlin_type(typ: Type, current_package: Option<&str>) -> Type {
    let Type::Class(mut class) = typ else {
        return typ;
    };

    class.type_arguments = class
        .type_arguments
        .into_iter()
        .map(|argument| normalize_kotlin_type(argument, current_package))
        .collect();

    if class.package.is_none() && class.type_arguments.is_empty() {
        let primitive = match class.name.as_str() {
            "Int" => Some(PrimitiveType::Int),
            "Long" => Some(PrimitiveType::Long),
            "Short" => Some(PrimitiveType::Short),
            "Byte" => Some(PrimitiveType::Byte),
            "Float" => Some(PrimitiveType::Float),
            "Double" => Some(PrimitiveType::Double),
            "Boolean" => Some(PrimitiveType::Boolean),
            "Char" => Some(PrimitiveType::Char),
            _ => None,
        };
        if let Some(primitive) = primitive {
            return Type::Primitive(primitive);
        }
    }

    if class.package.is_none() {
        class.package = match class.name.as_str() {
            "String" | "Any" | "Unit" | "Nothing" => Some("kotlin".to_string()),
            "List" | "MutableList" | "Set" | "MutableSet" | "Map" | "MutableMap" | "Collection"
            | "MutableCollection" | "Iterable" | "Sequence" => {
                Some("kotlin.collections".to_string())
            }
            _ => current_package.map(str::to_string),
        };
    }

    Type::Class(class)
}
