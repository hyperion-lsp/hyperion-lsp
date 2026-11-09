// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Type Checker
//!
//! Performs semantic error checking specific to Kotlin, including:
//! - When expression exhaustiveness on sealed classes
//! - Null safety violations (member access on nullable types)
//! - Extension function scope validation
//!
//! This complements the Java TypeChecker by providing Kotlin-specific validations.

use crate::db::FileId;
use crate::kotlin_resolver::{is_identifier_token, KotlinResolverBuilder};
use crate::resolver::SymbolResolver;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::symbol::{SymbolKind as SemanticSymbolKind, SymbolTable};
use hyperion_kotlin::smart_cast::SmartCastContext;
use hyperion_kotlin::types::KotlinType;
use hyperion_kotlin::{SyntaxKind, SyntaxNode};
use hyperion_workspace::{ImportMap, WorkspaceIndex};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

/// Kotlin-specific type checker that validates Kotlin semantic rules
pub struct KotlinTypeChecker {
    /// Stack of scopes for variable type tracking
    /// Inner scopes shadow outer scopes when variables have the same name
    variable_types_stack: Vec<HashMap<String, KotlinType>>,

    /// Smart cast context for tracking type narrowing
    smart_cast_context: SmartCastContext,

    /// Collected semantic errors
    errors: Vec<SemanticError>,

    /// Workspace context for import-aware checking
    workspace_context: Option<WorkspaceContext>,
}

/// Workspace context for Kotlin type checking
struct WorkspaceContext {
    /// The file being type-checked (stored for future error reporting)
    _file_id: FileId,
    /// Imports for the current file
    imports: ImportMap,
    /// Workspace index for class hierarchy and sealed class lookups
    workspace_index: Arc<WorkspaceIndex>,
    /// Local extension functions defined in the current file
    local_extensions: Vec<String>,
    /// Scope-aware symbols shared with inference/navigation.
    symbol_table: Arc<SymbolTable>,
    /// Immutable declarations that already have an initializer/delegate.
    ///
    /// A local `val x: Int; x = 1` is initialization, not reassignment. We
    /// deliberately suppress assignments to initializer-less vals rather than
    /// attempt incomplete definite-assignment dataflow.
    initialized_vals: HashSet<(u32, u32)>,
    /// Member methods declared in local classes within the same file.
    ///
    /// Used to avoid false positives where a member call is mistaken for an out-of-scope
    /// extension function.
    local_class_members: HashMap<String, HashSet<String>>,
}

impl KotlinTypeChecker {
    /// Create a new Kotlin type checker
    pub fn new() -> Self {
        Self {
            // Pre-allocate root scope for typical Kotlin function size
            variable_types_stack: vec![HashMap::with_capacity(15)],
            smart_cast_context: SmartCastContext::new(),
            errors: Vec::new(),
            workspace_context: None,
        }
    }

    /// Check a Kotlin file's AST with full workspace context
    ///
    /// This performs Kotlin-specific semantic checks:
    /// 1. When expression exhaustiveness on sealed classes
    /// 2. Null safety violations (unsafe member access on nullable types)
    /// 3. Extension function scope validation
    ///
    /// # Arguments
    /// * `root` - The root syntax node of the AST
    /// * `file_id` - The file being type-checked
    /// * `workspace_index` - The workspace index for class lookups
    ///
    /// # Returns
    /// A vector of semantic errors found during type checking
    pub fn check_with_context(
        &mut self,
        root: &SyntaxNode,
        file_id: FileId,
        workspace_index: Arc<WorkspaceIndex>,
    ) -> Vec<SemanticError> {
        let symbol_table = Arc::new(KotlinResolverBuilder::new(file_id).build(root));
        self.check_with_context_and_symbols(root, file_id, workspace_index, symbol_table)
    }

    /// Check using the Salsa-cached symbol table used by other semantic consumers.
    pub fn check_with_context_and_symbols(
        &mut self,
        root: &SyntaxNode,
        file_id: FileId,
        workspace_index: Arc<WorkspaceIndex>,
        symbol_table: Arc<SymbolTable>,
    ) -> Vec<SemanticError> {
        // Clear state from previous checks
        self.variable_types_stack.clear();
        self.variable_types_stack.push(HashMap::with_capacity(15));
        self.smart_cast_context = SmartCastContext::new();
        self.errors.clear();

        // Extract imports directly from the AST (more reliable than workspace index
        // which may not be populated in standalone type-checking scenarios)
        let imports = Self::extract_imports_from_ast(root);

        // Extract local extension function names
        let local_extensions = Self::extract_local_extensions(root);

        // Extract member methods for locally-declared classes (best-effort).
        let local_class_members = Self::extract_local_class_members(root);
        let initialized_vals = Self::collect_initialized_val_locations(root);

        // Store workspace context
        self.workspace_context = Some(WorkspaceContext {
            _file_id: file_id,
            imports,
            workspace_index,
            local_extensions,
            local_class_members,
            symbol_table,
            initialized_vals,
        });

        debug!("Kotlin type checking file {:?}", file_id);

        // Walk the AST and check for Kotlin-specific errors
        self.walk_node(root);

        // Errors that hold whatever the rest of the workspace looks like.
        let certain_index = self.workspace_context.as_ref().map(|ctx| {
            (
                Arc::clone(&ctx.workspace_index),
                Arc::clone(&ctx.symbol_table),
            )
        });
        if let Some((index, table)) = certain_index {
            let certain = crate::kotlin_certain_errors::check(root, file_id, &index, &table);
            self.errors.extend(certain);
        }

        // Log error summary
        if !self.errors.is_empty() {
            debug!(
                "Kotlin type checking file {:?} found {} semantic errors",
                file_id,
                self.errors.len()
            );
        }

        // Clear context and return errors
        self.workspace_context = None;
        std::mem::take(&mut self.errors)
    }

    /// Extract imports and package info directly from the AST
    fn extract_imports_from_ast(root: &SyntaxNode) -> ImportMap {
        use hyperion_kotlin::SyntaxKind;

        let mut imports = ImportMap::new();

        for child in root.children() {
            match child.kind() {
                SyntaxKind::PACKAGE_DECL => {
                    imports.current_package = Self::extract_package_name(&child);
                }
                SyntaxKind::IMPORT_DECL => {
                    Self::process_import_decl(&child, &mut imports);
                }
                _ => {}
            }
        }

        imports
    }

    /// Extract package name from PACKAGE_DECL node
    fn extract_package_name(node: &SyntaxNode) -> Option<String> {
        use hyperion_kotlin::SyntaxKind;

        let mut parts = Vec::new();
        for token in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(t) = token {
                if t.kind() == SyntaxKind::IDENT {
                    parts.push(t.text().to_string());
                }
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("."))
        }
    }

    /// Process a single import declaration
    fn process_import_decl(node: &SyntaxNode, imports: &mut ImportMap) {
        use hyperion_kotlin::SyntaxKind;

        let mut is_wildcard = false;
        let mut path_parts = Vec::new();
        let mut alias: Option<String> = None;
        let mut collecting_alias = false;

        for token in node.descendants_with_tokens() {
            if let rowan::NodeOrToken::Token(t) = token {
                match t.kind() {
                    SyntaxKind::STAR => is_wildcard = true,
                    SyntaxKind::IDENT => {
                        if collecting_alias {
                            alias = Some(t.text().to_string());
                            collecting_alias = false;
                        } else {
                            path_parts.push(t.text().to_string());
                        }
                    }
                    SyntaxKind::AS_KW => {
                        collecting_alias = true;
                    }
                    _ => {}
                }
            }
        }

        if path_parts.is_empty() {
            return;
        }

        let full_path = path_parts.join(".");

        if is_wildcard {
            // import com.example.*
            imports.wildcards.push(full_path);
        } else {
            // import com.example.MyClass (or import com.example.MyClass as Alias)
            if let Some(simple_name) = path_parts.last() {
                let key = alias.unwrap_or_else(|| simple_name.clone());
                imports.explicit.insert(key, full_path);
            }
        }
    }

    /// Extract local extension function names from the AST
    ///
    /// Extension functions are declared like: `fun ReceiverType.functionName() = ...`
    /// The AST structure is: FUN_KW, TYPE_REF (receiver), DOT, IDENT (function name), ...
    fn extract_local_extensions(root: &SyntaxNode) -> Vec<String> {
        use hyperion_kotlin::SyntaxKind;

        let mut extensions = Vec::new();

        for child in root.children() {
            if child.kind() == SyntaxKind::FUNCTION_DECL {
                // Look for extension function pattern: fun Type.name(...)
                // Pattern: FUN_KW TYPE_REF DOT IDENT ...
                let mut after_fun = false;
                let mut has_receiver_type = false;
                let mut seen_dot = false;

                for elem in child.children_with_tokens() {
                    match elem {
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() == SyntaxKind::FUN_KW {
                                after_fun = true;
                            } else if t.kind() == SyntaxKind::DOT && after_fun && has_receiver_type
                            {
                                seen_dot = true;
                            } else if t.kind() == SyntaxKind::IDENT && seen_dot {
                                // This is the function name - it's an extension function
                                extensions.push(t.text().to_string());
                                break;
                            } else if t.kind() == SyntaxKind::L_PAREN {
                                // Hit parameter list before seeing DOT - not an extension function
                                break;
                            }
                        }
                        rowan::NodeOrToken::Node(n) => {
                            if n.kind() == SyntaxKind::TYPE_REF && after_fun && !seen_dot {
                                has_receiver_type = true;
                            } else if n.kind() == SyntaxKind::PARAMETER_LIST {
                                // Hit parameter list - done checking
                                break;
                            }
                        }
                    }
                }
            }
        }

        extensions
    }

    fn extract_local_class_members(root: &SyntaxNode) -> HashMap<String, HashSet<String>> {
        let mut members: HashMap<String, HashSet<String>> = HashMap::new();

        for class_node in root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::CLASS_DECL)
        {
            let class_name = class_node
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .find(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string());

            let Some(class_name) = class_name else {
                continue;
            };

            let mut methods = HashSet::new();
            for fn_node in class_node
                .descendants()
                .filter(|n| n.kind() == SyntaxKind::FUNCTION_DECL)
            {
                let fn_name = fn_node
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|t| t.kind() == SyntaxKind::IDENT)
                    .map(|t| t.text().to_string());

                if let Some(fn_name) = fn_name {
                    methods.insert(fn_name);
                }
            }

            if !methods.is_empty() {
                members.insert(class_name, methods);
            }
        }

        members
    }

    /// Walk the AST node and check for Kotlin-specific semantic errors
    fn walk_node(&mut self, node: &SyntaxNode) {
        use hyperion_kotlin::SyntaxKind;

        match node.kind() {
            // Function declarations - extract parameters and walk body
            SyntaxKind::FUNCTION_DECL => {
                self.enter_function_scope(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.exit_function_scope();
                return; // Don't recurse again
            }

            SyntaxKind::WHEN_EXPR => {
                self.check_when_exhaustiveness(node);
            }

            // Member access (MEMBER_ACCESS_EXPR or CALL_EXPR with receiver)
            SyntaxKind::MEMBER_ACCESS_EXPR => {
                self.check_nullable_member_access(node);
            }

            // Note: SAFE_ACCESS_EXPR intentionally not handled — safe call operator (?.) needs no check

            // Call expressions - check extension function scope
            SyntaxKind::CALL_EXPR => {
                self.check_extension_function_scope(node);
            }

            SyntaxKind::BINARY_EXPR => {
                self.check_val_reassignment(node);
            }

            // Block scope management
            SyntaxKind::BLOCK => {
                self.smart_cast_context.enter_scope();
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.smart_cast_context.exit_scope();
                return; // Don't recurse again
            }

            // If expressions - manage smart cast scopes
            SyntaxKind::IF_EXPR => {
                self.check_if_expression(node);
                return; // We handle recursion manually
            }

            _ => {}
        }

        // Recursively walk children
        for child in node.children() {
            self.walk_node(&child);
        }
    }

    /// Report a bare-name assignment when scope resolution proves the target is
    /// a parameter or immutable declaration. Member/index assignments are
    /// intentionally left to later receiver-aware checking.
    fn check_val_reassignment(&mut self, node: &SyntaxNode) {
        use rowan::NodeOrToken;

        // `f(name = value)` is a named argument, never an assignment.
        if node
            .parent()
            .is_some_and(|parent| parent.kind() == SyntaxKind::CALL_EXPR)
        {
            return;
        }

        let mut lhs = None;
        let mut saw_nontrivia_node_before_eq = false;
        let mut is_assignment = false;

        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Node(_) if !is_assignment => {
                    saw_nontrivia_node_before_eq = true;
                }
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::EQ => {
                    is_assignment = true;
                    break;
                }
                NodeOrToken::Token(token) if !is_assignment && is_identifier_token(&token) => {
                    lhs = Some(token);
                }
                _ => {}
            }
        }

        let Some(lhs) = lhs.filter(|_| is_assignment && !saw_nontrivia_node_before_eq) else {
            return;
        };
        let name = lhs.text().trim_matches('`');
        let offset = lhs.text_range().start().into();
        let Some(context) = &self.workspace_context else {
            return;
        };
        let resolver = SymbolResolver::with_cached_table(Arc::clone(&context.symbol_table));
        let Some(scope) = resolver.find_scope_at_offset(offset) else {
            return;
        };
        let Some(resolved) = resolver.resolve_in_scope_at_offset(name, scope, offset) else {
            return;
        };
        let Some(symbol) = resolver.symbol_table().get_symbol(resolved.symbol_id) else {
            return;
        };
        let was_initialized = context
            .initialized_vals
            .contains(&(symbol.location.range.start, symbol.location.range.end));
        let immutable = match &symbol.kind {
            // Primary-constructor parameters are not visible in member bodies, so
            // a same-named assignment there targets a (possibly inherited) member.
            // Only treat the parameter as immutable inside the callable declaring it.
            SemanticSymbolKind::Parameter { .. } => lhs.parent_ancestors().any(|ancestor| {
                matches!(
                    ancestor.kind(),
                    SyntaxKind::FUNCTION_DECL
                        | SyntaxKind::LAMBDA_EXPR
                        | SyntaxKind::CONSTRUCTOR_DECL
                        | SyntaxKind::PROPERTY_ACCESSOR
                ) && u32::from(ancestor.text_range().start()) <= symbol.location.range.start
                    && symbol.location.range.end <= u32::from(ancestor.text_range().end())
            }),
            SemanticSymbolKind::LocalVariable { is_final, .. } => *is_final && was_initialized,
            SemanticSymbolKind::Field { .. } => symbol.modifiers.is_final() && was_initialized,
            _ => false,
        };

        if immutable {
            self.errors.push(SemanticError::new(
                lhs.text_range(),
                SemanticErrorKind::ValReassignment {
                    variable_name: name.to_string(),
                },
            ));
        }
    }

    fn collect_initialized_val_locations(root: &SyntaxNode) -> HashSet<(u32, u32)> {
        root.descendants()
            .filter(|node| node.kind() == SyntaxKind::PROPERTY_DECL)
            .filter_map(|property| {
                let mut saw_val = false;
                let mut name_range = None;
                let mut initialized = false;
                for element in property.children_with_tokens() {
                    let rowan::NodeOrToken::Token(token) = element else {
                        continue;
                    };
                    if token.kind() == SyntaxKind::VAL_KW {
                        saw_val = true;
                    } else if saw_val && name_range.is_none() && is_identifier_token(&token) {
                        name_range = Some(token.text_range());
                    } else if matches!(token.kind(), SyntaxKind::EQ | SyntaxKind::BY_KW) {
                        initialized = true;
                    }
                }
                let range = name_range.filter(|_| initialized)?;
                Some((range.start().into(), range.end().into()))
            })
            .collect()
    }

    /// Enter a function scope and extract parameter types
    fn enter_function_scope(&mut self, func_node: &SyntaxNode) {
        use hyperion_kotlin::SyntaxKind;

        // Push a new scope for function parameters (pre-allocate for typical function size)
        self.variable_types_stack.push(HashMap::with_capacity(8));

        // Find PARAMETER_LIST and extract parameters
        for child in func_node.children() {
            if child.kind() == SyntaxKind::PARAMETER_LIST {
                self.extract_parameters(&child);
            }
        }
    }

    /// Exit a function scope
    fn exit_function_scope(&mut self) {
        self.variable_types_stack.pop();
    }

    /// Extract parameters from a PARAMETER_LIST node
    fn extract_parameters(&mut self, param_list: &SyntaxNode) {
        use hyperion_kotlin::SyntaxKind;

        for child in param_list.children() {
            if child.kind() == SyntaxKind::PARAMETER {
                if let Some((name, typ)) = self.extract_parameter(&child) {
                    if let Some(scope) = self.variable_types_stack.last_mut() {
                        scope.insert(name, typ);
                    }
                }
            }
        }
    }

    /// Extract a single parameter's name and type
    #[allow(clippy::unused_self)]
    fn extract_parameter(&self, param: &SyntaxNode) -> Option<(String, KotlinType)> {
        use hyperion_kotlin::SyntaxKind;

        let mut name: Option<String> = None;
        let mut param_type = KotlinType::Unknown;

        // `vararg xs: String?` binds `xs` to an *array*; its declared type is
        // the element type.
        if param
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .any(|token| token.kind() == SyntaxKind::VARARG_KW)
        {
            return None;
        }

        for child in param.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::IDENT && name.is_none() {
                        name = Some(token.text().to_string());
                    }
                }
                rowan::NodeOrToken::Node(node) => {
                    if matches!(
                        node.kind(),
                        SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE
                    ) {
                        param_type = KotlinType::from_ast(&node);
                    }
                }
            }
        }

        name.map(|n| (n, param_type))
    }

    /// Check when expression for exhaustiveness on sealed classes
    fn check_when_exhaustiveness(&mut self, when_node: &SyntaxNode) {
        use hyperion_kotlin::when_exhaustiveness::{check_exhaustiveness, extract_when_info};

        // Extract when expression info
        let when_info = match extract_when_info(when_node) {
            Some(info) => info,
            None => return, // Failed to extract, skip check
        };

        // If there's an else branch, it's always exhaustive
        if when_info.has_else_branch {
            return;
        }

        // Try to determine the subject type
        // For now, we need to get the variable's type from context
        let subject_type = if let Some(ref var_name) = when_info.subject_name {
            self.get_variable_type(var_name)
        } else {
            return; // Can't check without subject
        };

        // Check if it's a sealed class
        let sealed_type_fqn = match &subject_type {
            KotlinType::Class { fqn, .. } => fqn.clone(),
            _ => return, // Not a class type, can't be sealed
        };

        // Get sealed subclasses from workspace index
        let workspace_ctx = match &self.workspace_context {
            Some(ctx) => ctx,
            None => return,
        };

        let sealed_subclasses = workspace_ctx
            .workspace_index
            .get_sealed_subclasses(&sealed_type_fqn);

        if sealed_subclasses.is_empty() {
            // Not a sealed class, or we don't have info
            return;
        }

        // Extract simple name for display
        let sealed_type_simple = sealed_type_fqn
            .split('.')
            .next_back()
            .unwrap_or(&sealed_type_fqn);

        // Check exhaustiveness
        let result = check_exhaustiveness(&when_info, &sealed_subclasses, sealed_type_simple);

        if !result.is_exhaustive {
            self.errors.push(SemanticError::new(
                result.when_range,
                SemanticErrorKind::NonExhaustiveWhen {
                    sealed_type: sealed_type_simple.to_string(),
                    missing_branches: result.missing_branches,
                },
            ));
        }
    }

    /// Check member access on nullable types
    fn check_nullable_member_access(&mut self, node: &SyntaxNode) {
        // Extract receiver and member from member access expression
        // MEMBER_ACCESS_EXPR: receiver.member
        let (receiver_node, member_name) = match self.extract_member_access_parts(node) {
            Some(parts) => parts,
            None => return, // Failed to parse structure
        };

        // A chained access is represented by nested MEMBER_ACCESS_EXPR nodes.
        // Check the inner access when it is visited instead of attributing its
        // receiver type to every outer segment and emitting duplicate errors.
        // Member-result type inference can add a distinct outer diagnostic once
        // it can resolve the nested expression's actual type.
        if receiver_node.kind() == SyntaxKind::MEMBER_ACCESS_EXPR
            && receiver_node.text_range() != node.text_range()
        {
            return;
        }

        // Extensions declared on a nullable receiver: `x.isNullOrBlank()`,
        // `x.let { .. }`, `x.toString()` are legal on `T?` and often *are* the
        // null check.
        if matches!(
            member_name.as_str(),
            "let"
                | "also"
                | "apply"
                | "run"
                | "takeIf"
                | "takeUnless"
                | "to"
                | "toString"
                | "hashCode"
                | "equals"
                | "isNullOrEmpty"
                | "isNullOrBlank"
                | "orEmpty"
                | "contentToString"
                | "contentEquals"
                | "contentHashCode"
                | "contentDeepToString"
                | "plus"
                | "ifEmpty"
                | "ifBlank"
                | "printStackTrace"
                | "use"
        ) {
            return;
        }

        // Infer the type of the receiver
        let receiver_type = match self.infer_receiver_type(&receiver_node, node) {
            Some(typ) => typ,
            None => return, // Can't infer type, skip check
        };

        // Check if receiver is nullable
        if !receiver_type.is_nullable() {
            return; // Type is non-nullable, safe to access
        }

        // Check if smart cast makes this safe
        if let Some(var_name) = self.extract_variable_name(&receiver_node) {
            if let Some(narrowed) = self.smart_cast_context.get_narrowed_type(&var_name) {
                if !narrowed.is_nullable() {
                    return; // Smart cast makes this safe
                }
            }
        }

        // Any earlier null-handling of the variable in this function (`x != null`,
        // `x is T`, `x!!`, `x ?: ..`, `x as T`, `requireNotNull(x)`, `when (x)`)
        // may narrow it in ways the smart-cast tracker does not model
        // (`x is T && x.f`, early returns, contracts). Only a variable with no
        // such evidence is provably nullable here.
        if let Some(var_name) = self.extract_variable_name(&receiver_node) {
            if Self::has_null_handling_evidence(node, &var_name) {
                return;
            }
        }

        // A workspace extension of that name may be declared on a nullable
        // receiver (`fun String?.isNotNullEmpty()`).
        if let Some(ctx) = &self.workspace_context {
            let is_workspace_extension = ctx
                .workspace_index
                .find_by_simple_name(&member_name)
                .iter()
                .filter_map(|location| ctx.workspace_index.get_symbol(&location.fqn))
                .any(|symbol| symbol.receiver_type.is_some());
            if is_workspace_extension {
                return;
            }
        }

        // Get the expression text for the error message
        let expression = receiver_node.text().to_string();

        // Report error: unsafe member access on nullable type
        self.errors.push(SemanticError::new(
            node.text_range(),
            SemanticErrorKind::NullableTypeMismatch {
                nullable_type: receiver_type.to_string(),
                non_nullable_required: true,
                expression,
            },
        ));
    }

    /// Whether `name` is null-checked, type-tested, asserted or cast anywhere in
    /// the enclosing function before `access`.
    fn has_null_handling_evidence(access: &SyntaxNode, name: &str) -> bool {
        // The outermost enclosing function: a local `fun` sees the null checks
        // made by the function it is nested in.
        let scope = access
            .ancestors()
            .filter(|ancestor| matches!(ancestor.kind(), SyntaxKind::FUNCTION_DECL))
            .last()
            .or_else(|| access.ancestors().last());
        let Some(scope) = scope else {
            return true;
        };
        let limit = access.text_range().start();
        // A local declaration, lambda/loop/catch binding or assignment that
        // reuses the name means the parameter type no longer describes it.
        let redeclared_or_assigned = scope.descendants_with_tokens().any(|element| {
            let Some(token) = element.as_token() else {
                return false;
            };
            if token.kind() != SyntaxKind::IDENT || token.text() != name {
                return false;
            }
            let Some(parent) = token.parent() else {
                return false;
            };
            let binds = matches!(
                parent.kind(),
                SyntaxKind::PROPERTY_DECL
                    | SyntaxKind::LAMBDA_PARAMETERS
                    | SyntaxKind::FOR_LOOP_VAR
                    | SyntaxKind::DESTRUCTURING_DECL
            ) || (parent.kind() == SyntaxKind::PARAMETER
                && parent
                    .parent()
                    .and_then(|list| list.parent())
                    .is_some_and(|owner| owner != scope));
            let assigned = parent.kind() == SyntaxKind::BINARY_EXPR
                && std::iter::successors(token.next_sibling_or_token(), |sibling| {
                    sibling.next_sibling_or_token()
                })
                .find(|sibling| !sibling.kind().is_trivia())
                .is_some_and(|sibling| sibling.kind() == SyntaxKind::EQ);
            binds || assigned
        });
        if redeclared_or_assigned {
            return true;
        }
        let tokens: Vec<_> = scope
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| !token.kind().is_trivia())
            .collect();
        tokens.iter().enumerate().any(|(index, token)| {
            if token.kind() != SyntaxKind::IDENT
                || token.text() != name
                || token.text_range().start() >= limit
                // the declaration itself is not evidence of anything
                || token
                    .parent()
                    .is_some_and(|parent| parent.kind() == SyntaxKind::PARAMETER)
            {
                return false;
            }
            let next = tokens.get(index + 1).map(|next| next.kind());
            let previous = index
                .checked_sub(1)
                .and_then(|previous| tokens.get(previous));
            let before_previous = index.checked_sub(2).and_then(|i| tokens.get(i));
            matches!(
                next,
                Some(
                    SyntaxKind::BANGEQ
                        | SyntaxKind::EQEQ
                        | SyntaxKind::IS_KW
                        | SyntaxKind::NOT_IS
                        | SyntaxKind::NOT_NULL
                        | SyntaxKind::ELVIS
                        | SyntaxKind::SAFE_CALL
                        | SyntaxKind::AS_KW
                        | SyntaxKind::SAFE_CAST
                )
            ) || (next == Some(SyntaxKind::DOT)
                && tokens.get(index + 2).is_some_and(|member| {
                    // `x.isNullOrBlank()` / `x.isNullOrEmpty()` are contracts that
                    // narrow `x` when they guard an early exit.
                    matches!(member.text(), "isNullOrBlank" | "isNullOrEmpty")
                }))
                || previous.is_some_and(|p| {
                    // `null != x`, `null == x`, `requireNotNull(x)`, `when (x)`, `if (x)`
                    matches!(p.kind(), SyntaxKind::BANGEQ | SyntaxKind::EQEQ)
                        || (p.kind() == SyntaxKind::L_PAREN
                            && before_previous.is_some_and(|b| {
                                matches!(b.kind(), SyntaxKind::WHEN_KW | SyntaxKind::IDENT)
                            }))
                })
        })
    }

    /// Extract receiver and member name from a member access expression
    /// Returns (receiver_node, member_name)
    ///
    /// For simple identifiers (e.g., `str.method()`), we return the parent MEMBER_ACCESS_EXPR
    /// as the "receiver node" since we need a node to extract the variable name from.
    #[allow(clippy::unused_self)]
    fn extract_member_access_parts(&self, node: &SyntaxNode) -> Option<(SyntaxNode, String)> {
        // MEMBER_ACCESS_EXPR has structure: receiver DOT member
        // receiver can be:
        // - A node (e.g., another MEMBER_ACCESS_EXPR, CALL_EXPR)
        // - A simple IDENT token (e.g., `str` in `str.method()`)
        let mut receiver: Option<SyntaxNode> = None;
        let mut receiver_ident: Option<String> = None;
        let mut member_name = String::new();
        let mut found_dot = false;

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(n) => {
                    if !found_dot {
                        receiver = Some(n);
                    }
                }
                rowan::NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::DOT {
                        found_dot = true;
                    } else if t.kind() == SyntaxKind::IDENT {
                        if found_dot {
                            member_name = t.text().to_string();
                        } else if receiver.is_none() {
                            // Simple identifier before dot
                            receiver_ident = Some(t.text().to_string());
                        }
                    }
                }
            }
        }

        // If we have a receiver node, use it
        if let Some(r) = receiver {
            return Some((r, member_name));
        }

        // If receiver is a simple identifier, we need to return the member access node itself
        // so the caller can extract the variable name from the first IDENT token
        if receiver_ident.is_some() {
            // Return the member access node - the caller will extract the var name from its first IDENT
            return Some((node.clone(), member_name));
        }

        None
    }

    /// Infer the type of a receiver expression. `access` is the member access the receiver
    /// was taken from. When the receiver is a bare identifier, `extract_member_access_parts`
    /// hands back the access itself, so the receiver is `access` and its first identifier is
    /// the variable.
    fn infer_receiver_type(
        &self,
        receiver: &SyntaxNode,
        access: &SyntaxNode,
    ) -> Option<KotlinType> {
        // Only a bare identifier receiver is a variable. `x!!.f()`, `f(x).g()`,
        // `(x as T).f()` etc. carry their own (non-)nullability; reading the
        // first identifier out of them types the wrong expression.
        if !matches!(
            receiver.kind(),
            SyntaxKind::MEMBER_ACCESS_EXPR | SyntaxKind::SAFE_ACCESS_EXPR
        ) {
            return None;
        }
        // In `a.b.ext()` the receiver is the access `a.b`, a member of `a` that needs a
        // member lookup, which this checker does not do. Its first identifier `a`, or the
        // `c` in `a.b.c`, is not the receiver.
        if receiver.kind() == SyntaxKind::MEMBER_ACCESS_EXPR && receiver != access {
            return None;
        }
        // `x?.y` starts with the variable. `a.b?.y` starts with another access.
        let starts_with_identifier = receiver
            .children_with_tokens()
            .find(|child| !child.kind().is_trivia())
            .is_some_and(|first| first.kind() == SyntaxKind::IDENT);
        if !starts_with_identifier {
            return None;
        }
        // Try to extract variable name and look up its type
        if let Some(var_name) = self.extract_variable_name(receiver) {
            return Some(self.get_variable_type(&var_name));
        }

        // For other expressions, we'd need more sophisticated type inference
        // For now, return Unknown
        None
    }

    /// Extract variable name from a simple reference expression
    #[allow(clippy::unused_self)]
    fn extract_variable_name(&self, node: &SyntaxNode) -> Option<String> {
        // Simple case: REFERENCE_EXPR or IDENT token
        for child in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(t) = child {
                if t.kind() == SyntaxKind::IDENT {
                    return Some(t.text().to_string());
                }
            }
        }

        None
    }

    /// Check if expression for smart casts and null safety
    fn check_if_expression(&mut self, if_node: &SyntaxNode) {
        use hyperion_kotlin::smart_cast::analyze_condition;

        // Extract condition expression (first child expression after IF_KW and L_PAREN)
        // Extract then/else blocks
        let mut condition_node = None;
        let mut blocks = Vec::new();
        let mut found_if = false;
        let mut found_lparen = false;
        let mut found_rparen = false;

        for elem in if_node.children_with_tokens() {
            match elem {
                rowan::NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::IF_KW {
                        found_if = true;
                    } else if t.kind() == SyntaxKind::L_PAREN && found_if {
                        found_lparen = true;
                    } else if t.kind() == SyntaxKind::R_PAREN && found_lparen {
                        found_rparen = true;
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    if found_lparen && !found_rparen && condition_node.is_none() {
                        // This is the condition expression
                        condition_node = Some(n);
                    } else if found_rparen
                        && (n.kind() == SyntaxKind::BLOCK || n.kind() == SyntaxKind::IF_EXPR)
                    {
                        // This is a then/else block
                        blocks.push(n);
                    }
                }
            }
        }

        // Analyze condition for smart casts
        let analysis = if let Some(cond) = condition_node {
            analyze_condition(&cond)
        } else {
            // Just walk normally if we can't parse the condition
            for child in if_node.children() {
                self.walk_node(&child);
            }
            return;
        };

        // Process then branch (first block) with true-branch smart casts
        if let Some(then) = blocks.first() {
            self.smart_cast_context.enter_scope();
            for cast in &analysis.true_branch_casts {
                self.smart_cast_context.add_smart_cast(cast.clone());
            }
            self.walk_node(then);
            self.smart_cast_context.exit_scope();
        }

        // Process else branch (second block) with false-branch smart casts
        if let Some(els) = blocks.get(1) {
            self.smart_cast_context.enter_scope();
            for cast in &analysis.false_branch_casts {
                self.smart_cast_context.add_smart_cast(cast.clone());
            }
            self.walk_node(els);
            self.smart_cast_context.exit_scope();
        }
    }

    /// Check extension function scope validation
    fn check_extension_function_scope(&mut self, call_node: &SyntaxNode) {
        // Extension functions look like: receiver.functionName()
        // We need to check if it's a member call (has a receiver)

        // Try to extract receiver and function name
        let (receiver_node, function_name, access) = match self.extract_call_parts(call_node) {
            Some(parts) => parts,
            None => return, // Not a member call, skip
        };

        // Infer receiver type
        let receiver_type = match self.infer_receiver_type(&receiver_node, &access) {
            Some(typ) => typ,
            None => return, // Can't infer type, skip check
        };

        // Get receiver type FQN for error reporting
        let receiver_fqn = match &receiver_type {
            KotlinType::Class { fqn, .. } => fqn.clone(),
            _ => return, // Not a class type, skip
        };

        // Get imports for the current file
        let imports = match &self.workspace_context {
            Some(ctx) => &ctx.imports,
            None => return, // No workspace context, skip
        };

        // Avoid false positives: if this is a member method declared on a local class within
        // the same file, it is not an extension function.
        if let Some(ctx) = &self.workspace_context {
            let receiver_simple = receiver_fqn.rsplit('.').next().unwrap_or(&receiver_fqn);
            if let Some(methods) = ctx.local_class_members.get(receiver_simple) {
                if methods.contains(&function_name) {
                    return;
                }
            }
        }

        // Check if function is in scope:
        // 1. Explicitly imported (e.g., import com.utils.customExtension)
        // 2. Wildcard imported (e.g., import com.utils.*)
        // 3. Defined in the current package (as a local extension function)
        //
        // Note: We check if the function NAME is available, not a specific FQN,
        // because we don't know where the extension is defined from the call site.

        // A call is only provably missing an import when the workspace itself
        // declares an extension with this name for this receiver and nothing
        // that could make it visible applies. A name the workspace has never
        // heard of may come from a jar, the stdlib default imports, an
        // inherited member or a DSL receiver, so it stays silent.
        if hyperion_kotlin::is_kotlin_stdlib_name(&function_name) {
            return;
        }
        let Some(ctx) = &self.workspace_context else {
            return;
        };
        if ctx.local_extensions.contains(&function_name) {
            return;
        }
        let simple_receiver = receiver_fqn.rsplit('.').next().unwrap_or(&receiver_fqn);
        let mut candidates = ctx
            .workspace_index
            .find_extension_by_name(&receiver_fqn, &function_name);
        candidates.extend(
            ctx.workspace_index
                .find_extension_by_name(simple_receiver, &function_name),
        );
        candidates.sort_by(|left, right| left.fqn.cmp(&right.fqn));
        candidates.dedup_by(|left, right| left.fqn == right.fqn);
        if candidates.is_empty() {
            return;
        }
        // Any same-named declaration that is not one of those extensions may
        // be a member of the receiver (or a supertype); stay silent.
        // The receiver must be a type whose members we can enumerate: an indexed
        // class (a member of that name means the extension is never consulted),
        // or a Kotlin builtin covered by the stdlib table. An unindexed library
        // class (`android.content.Context`) may well have the member.
        let simple_receiver_name = receiver_fqn.rsplit('.').next().unwrap_or(&receiver_fqn);
        if ctx.workspace_index.has_class(&receiver_fqn) {
            if !ctx
                .workspace_index
                .find_effective_methods_by_name(&receiver_fqn, &function_name)
                .is_empty()
            {
                return;
            }
        } else if receiver_fqn.starts_with("kotlin.") {
            let has_member =
                hyperion_kotlin::stdlib_members::entries_for(simple_receiver_name, &[], &[])
                    .iter()
                    .any(|entry| entry.is_member && entry.name == function_name);
            if has_member {
                return;
            }
        } else {
            return;
        }

        // (JVM-view aliases of those same extensions do not count: `ExtKt.shout`
        // is the Java-visible form of the extension itself.)
        let same_named = ctx
            .workspace_index
            .find_by_simple_name(&function_name)
            .iter()
            .filter(|location| {
                ctx.workspace_index
                    .get_symbol(&location.fqn)
                    .is_none_or(|symbol| {
                        symbol
                            .kotlin
                            .as_ref()
                            .is_none_or(|kotlin| kotlin.jvm_origin.is_none())
                    })
            })
            .count();
        if same_named != candidates.len() {
            return;
        }
        let current_package = imports.current_package.as_deref();
        let visible = candidates.iter().any(|candidate| {
            let container = candidate
                .fqn
                .rsplit_once('.')
                .map_or("", |(container, _)| container);
            // A member extension (declared inside a class/companion) is visible
            // through that class's scope, which we do not model.
            candidate.container.is_some()
                || current_package == Some(container)
                || imports.explicit.contains_key(function_name.as_str())
                || imports
                    .explicit
                    .values()
                    .any(|fqn| fqn.as_str() == candidate.fqn.as_ref())
                || imports
                    .wildcards
                    .iter()
                    .any(|wildcard| wildcard == container)
                || container.starts_with("kotlin")
        });
        if visible {
            return;
        }

        // Extension function is not in scope - report error
        self.errors.push(SemanticError::new(
            call_node.text_range(),
            SemanticErrorKind::ExtensionFunctionNotInScope {
                function_name,
                receiver_type: receiver_fqn,
            },
        ));
    }

    /// Extract receiver and function name from a call expression
    /// Returns (receiver_node, function_name, member_access) if it's a member call
    fn extract_call_parts(
        &self,
        call_node: &SyntaxNode,
    ) -> Option<(SyntaxNode, String, SyntaxNode)> {
        // CALL_EXPR can be:
        // 1. Simple call: functionName()
        // 2. Member call: receiver.functionName()

        // Look for member access pattern in children
        for child in call_node.children() {
            if child.kind() == SyntaxKind::MEMBER_ACCESS_EXPR {
                // Extract receiver and member name from member access
                return self
                    .extract_member_access_parts(&child)
                    .map(|(receiver, name)| (receiver, name, child));
            }
        }

        // Not a member call
        None
    }

    /// Get the type of a variable from the current scope or smart cast context
    fn get_variable_type(&self, var_name: &str) -> KotlinType {
        // First check smart casts
        if let Some(narrowed) = self.smart_cast_context.get_narrowed_type(var_name) {
            return narrowed.clone();
        }

        // Then check variable type stack
        for scope in self.variable_types_stack.iter().rev() {
            if let Some(typ) = scope.get(var_name) {
                return typ.clone();
            }
        }

        KotlinType::Unknown
    }
}

impl Default for KotlinTypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kotlin_type_checker_creation() {
        let checker = KotlinTypeChecker::new();
        assert_eq!(checker.errors.len(), 0);
        assert_eq!(checker.variable_types_stack.len(), 1);
    }

    #[test]
    fn test_extension_not_imported_reports_error() {
        use hyperion_kotlin::{parse, SyntaxNode};

        let source = r#"
        package com.example

        fun test(str: String) {
            str.customExtension()
        }
        "#;

        let parse_result = parse(source);
        let root = SyntaxNode::new_root(parse_result.green_node);

        use crate::db::FileId;
        use hyperion_workspace::WorkspaceIndex;
        use std::path::PathBuf;
        use std::sync::Arc;

        // The extension exists in another package of the workspace, so the
        // missing import is provable.
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace_index = Arc::new(WorkspaceIndex::new(PathBuf::from(temp.path())));
        let ext_path = temp.path().join("com/utils/Ext.kt");
        std::fs::create_dir_all(ext_path.parent().unwrap()).unwrap();
        workspace_index
            .index_file_with_source(
                &ext_path,
                "package com.utils\n\nfun String.customExtension() = 1\n",
            )
            .expect("index extension");
        let file_id = FileId(0);

        let mut checker = KotlinTypeChecker::new();
        let errors = checker.check_with_context(&root, file_id, workspace_index);

        // Should have error about extension not in scope
        assert!(
            errors.iter().any(|e| matches!(
                e.kind,
                SemanticErrorKind::ExtensionFunctionNotInScope { .. }
            )),
            "Expected extension not in scope error, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_local_extension_extraction() {
        use hyperion_kotlin::{parse, SyntaxNode};

        let source = r#"
        package com.example

        // Extension function defined in same package
        fun String.customExtension() = "extended"

        fun test(str: String) {
            str.customExtension()
        }
        "#;

        let parse_result = parse(source);
        let root = SyntaxNode::new_root(parse_result.green_node);

        let extensions = KotlinTypeChecker::extract_local_extensions(&root);

        assert!(
            extensions.contains(&"customExtension".to_string()),
            "Expected to find 'customExtension' in local extensions, got: {:?}",
            extensions
        );

        // Regular function should not be in extensions list
        assert!(
            !extensions.contains(&"test".to_string()),
            "Regular function 'test' should not be in extensions list"
        );
    }
}
