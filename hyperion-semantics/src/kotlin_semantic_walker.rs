// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin semantic walker that pre-computes expression types for every expression in a file.
//!
//! This mirrors the Java semantic walker's approach but for Kotlin syntax. Results are
//! stored in a `FileSemantics` and cached via Salsa.

use crate::db::FileId;
use crate::file_semantics::FileSemantics;
use crate::kotlin_inference::KotlinTypeInferenceEngine;
use crate::kotlin_resolver::is_identifier_token;
use crate::resolved_name::ResolvedName;
use crate::resolver::SymbolResolver;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::symbol::{SymbolTable, Type};
use crate::unified_resolver::{NameContext, UnifiedTypeResolver};
use hyperion_kotlin::{
    SyntaxKind as KtKind, SyntaxNode as KtSyntaxNode, SyntaxToken as KtSyntaxToken,
};
use hyperion_workspace::{DeclarationNamespace, ImportMap, Language, WorkspaceIndex};
use rowan::{NodeOrToken, TextRange};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Compute file semantics for a Kotlin file by walking the AST and inferring expression types.
///
/// This is the entry point called by the Salsa `file_semantics` query for Kotlin files.
pub fn compute_kotlin_file_semantics(
    root: &KtSyntaxNode,
    file_id: FileId,
    symbol_table: Arc<SymbolTable>,
    workspace_index: &WorkspaceIndex,
) -> FileSemantics {
    let workspace_arc = Arc::new(workspace_index.clone());
    let inference_engine = KotlinTypeInferenceEngine::new(Arc::clone(&workspace_arc), file_id);
    let mut resolver = SymbolResolver::with_cached_table(symbol_table);
    let imports = extract_imports(root);
    let declared_names = collect_declared_names(root);
    let local_types = collect_local_types(root);
    let type_parameters = collect_type_parameters(root);

    let mut walker = KotlinSemanticWalker {
        resolved_names: HashMap::new(),
        expression_types: HashMap::new(),
        diagnostics: Vec::new(),
        inference_engine: &inference_engine,
        resolver: &mut resolver,
        workspace_index,
        imports: &imports,
        declared_names: &declared_names,
        local_types: &local_types,
        type_parameters: &type_parameters,
    };

    walker.walk_node(root);

    FileSemantics::new(
        walker.resolved_names,
        walker.expression_types,
        walker.diagnostics,
    )
}

/// Internal walker state for Kotlin files.
struct KotlinSemanticWalker<'a> {
    /// Identifier resolutions shared by navigation, references, hover, and diagnostics.
    resolved_names: HashMap<TextRange, ResolvedName>,
    /// Accumulated expression type inference results
    expression_types: HashMap<TextRange, Type>,
    /// Conservative unresolved-reference diagnostics.
    diagnostics: Vec<SemanticError>,
    /// Kotlin type inference engine
    inference_engine: &'a KotlinTypeInferenceEngine,
    /// Symbol resolver for variable lookups
    resolver: &'a mut SymbolResolver,
    workspace_index: &'a WorkspaceIndex,
    imports: &'a ImportMap,
    declared_names: &'a HashSet<String>,
    local_types: &'a HashSet<String>,
    type_parameters: &'a HashSet<String>,
}

impl KotlinSemanticWalker<'_> {
    fn infer_and_store(&mut self, node: &KtSyntaxNode) {
        let inferred = self.inference_engine.infer_expression(node, self.resolver);
        if !matches!(inferred, Type::Unknown) {
            self.expression_types.insert(node.text_range(), inferred);
        }
    }

    fn walk_node(&mut self, node: &KtSyntaxNode) {
        self.resolve_direct_identifier_tokens(node);

        match node.kind() {
            // Expression nodes — infer and store types, then recurse into children
            KtKind::CALL_EXPR
            | KtKind::MEMBER_ACCESS_EXPR
            | KtKind::SAFE_ACCESS_EXPR
            | KtKind::BINARY_EXPR
            | KtKind::UNARY_EXPR
            | KtKind::PAREN_EXPR
            | KtKind::LITERAL_EXPR
            | KtKind::WHEN_EXPR
            | KtKind::TRY_EXPR
            | KtKind::IF_EXPR
            | KtKind::LAMBDA_EXPR => {
                self.infer_and_store(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
            }

            // All other nodes — just recurse
            _ => {
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
        }
    }

    fn resolve_direct_identifier_tokens(&mut self, node: &KtSyntaxNode) {
        for element in node.children_with_tokens() {
            let NodeOrToken::Token(token) = element else {
                continue;
            };
            if !is_identifier_token(&token) {
                continue;
            }
            // `catch`/`finally`/`init` etc. are grammar keywords wherever they
            // head a clause; only true identifiers are references.
            if matches!(
                token.kind(),
                KtKind::CATCH_KW | KtKind::FINALLY_KW | KtKind::INIT_KW | KtKind::CONSTRUCTOR_KW
            ) && matches!(
                node.kind(),
                KtKind::CATCH_CLAUSE | KtKind::TRY_EXPR | KtKind::CLASS_DECL | KtKind::CLASS_BODY
            ) {
                continue;
            }
            if is_named_argument_label(node, &token)
                || is_infix_function_name(node, &token)
                || is_jump_label(&token)
                || is_type_operand(node, &token)
            {
                continue;
            }
            let Some(role) = reference_role(node, &token) else {
                continue;
            };
            self.resolve_reference(node, &token, role);
        }
    }

    fn resolve_reference(
        &mut self,
        parent: &KtSyntaxNode,
        token: &KtSyntaxToken,
        role: ReferenceRole,
    ) {
        let name = token.text().trim_matches('`');
        if name.is_empty() || name == "_" {
            return;
        }
        let offset = token.text_range().start().into();
        let context = NameContext {
            // Kotlin when labels are arbitrary expressions. Treating every
            // bare name as a Java-style enum constant fabricates an identity.
            in_switch_case: false,
            type_parameters: self.type_parameters,
            variable_scopes: &[],
            local_types: self.local_types,
            lexical_owners: &[],
            allow_external_type: role == ReferenceRole::Callee
                && name.chars().next().is_some_and(char::is_uppercase),
            expected_namespace: match role {
                ReferenceRole::Value => Some(DeclarationNamespace::Value),
                ReferenceRole::Callee
                | ReferenceRole::MemberReceiver
                | ReferenceRole::MemberName => None,
            },
        };
        let resolved = UnifiedTypeResolver::for_language(
            self.workspace_index,
            self.imports,
            Some(self.resolver),
            Language::Kotlin,
        )
        .resolve_name(name, offset, &context);

        if let Some(resolved) = resolved {
            self.resolved_names.insert(token.text_range(), resolved);
            return;
        }

        // An explicit import is an exact external identity even when its source
        // is unavailable. Wildcard imports, stdlib metadata, and same-file
        // declaration names can suppress a diagnostic, but cannot manufacture
        // an FQN for references or hover.
        if let Some(fqn) = self.imports.explicit.get(name) {
            self.resolved_names.insert(
                token.text_range(),
                ResolvedName::External { fqn: fqn.clone() },
            );
            return;
        }
        if hyperion_kotlin::STDLIB.lookup(name).is_some()
            || hyperion_kotlin::is_kotlin_stdlib_name(name)
            || self.declared_names.contains(name)
            || !self.imports.wildcards.is_empty()
        {
            return;
        }

        // A name declared *anywhere* the index knows about (other files and
        // modules, inherited members, jars, the JDK) is not a typo. Without
        // full scope/supertype resolution we cannot say which declaration a
        // bare name binds to, so its existence is enough to stay silent.
        if !self.workspace_index.find_by_simple_name(name).is_empty() {
            return;
        }

        // Member resolution is only sound with a known receiver. Both halves
        // of an unresolved qualified access are suppressed so an Unknown or
        // ambiguous receiver never creates speculative diagnostics.
        if matches!(
            role,
            ReferenceRole::MemberReceiver | ReferenceRole::MemberName
        ) {
            return;
        }

        // Inside an extension function/property, or a class with a supertype
        // the index cannot see, a bare name can be a member of the implicit
        // receiver (`fun Buffer.endsWith() = rangeEquals(..)`).
        if self.has_unknowable_implicit_receiver(parent) {
            return;
        }

        // A parameterless trailing lambda may have an implicit DSL receiver.
        // If receiver threading cannot prove a member, silence is safer than
        // claiming a valid DSL property/function is unresolved.
        if is_inside_implicit_receiver_lambda(parent) {
            return;
        }

        // A callee whose expression inference succeeds is resolved by the same
        // Kotlin call machinery even if it has no standalone workspace symbol.
        if role == ReferenceRole::Callee {
            let inferred = self
                .inference_engine
                .infer_expression(parent, self.resolver);
            if !matches!(inferred, Type::Unknown) {
                return;
            }
        }

        self.diagnostics.push(SemanticError::new(
            token.text_range(),
            SemanticErrorKind::UnresolvedReference {
                symbol_name: name.to_string(),
            },
        ));
    }
}

impl KotlinSemanticWalker<'_> {
    /// Whether some enclosing scope contributes an implicit `this` whose
    /// members the workspace index cannot enumerate: an extension receiver, or
    /// a class/object whose supertype is not declared in the workspace.
    fn has_unknowable_implicit_receiver(&self, node: &KtSyntaxNode) -> bool {
        node.ancestors().any(|ancestor| match ancestor.kind() {
            KtKind::FUNCTION_DECL | KtKind::PROPERTY_DECL => {
                ancestor
                    .children()
                    .any(|child| child.kind() == KtKind::TYPE_REF)
                    && has_receiver_dot(&ancestor)
            }
            KtKind::CLASS_DECL | KtKind::OBJECT_DECL | KtKind::INTERFACE_DECL => {
                self.has_external_supertype(&ancestor)
            }
            _ => false,
        })
    }

    /// Whether `name` and every supertype above it are declared in the index,
    /// so that all inherited members are enumerable.
    fn supertype_chain_is_indexed(&self, name: &str, visited: &mut HashSet<String>) -> bool {
        if !visited.insert(name.to_string()) || visited.len() > 64 {
            return true;
        }
        let declarations = self
            .workspace_index
            .find_type_declarations_by_simple_name(name);
        if declarations.is_empty() {
            return false;
        }
        declarations.iter().all(|declaration| {
            let Some(symbol) = self.workspace_index.get_symbol(&declaration.fqn) else {
                return false;
            };
            let declaring_imports = self.workspace_index.get_imports(declaration.file_id);
            symbol
                .parent_class
                .iter()
                .chain(symbol.interfaces.iter())
                .all(|written| {
                    let path = written.split('<').next().unwrap_or(written).trim();
                    // `RecyclerView.Adapter`: the leftmost segment says where the
                    // type comes from; a workspace `Adapter` elsewhere cannot vouch.
                    let simple = path.split('.').next().unwrap_or(path).trim();
                    if simple.starts_with(|c: char| c.is_lowercase())
                        && path.contains('.')
                        && self.workspace_index.find_symbol(path).is_none()
                    {
                        return false;
                    }
                    let simple = if simple.starts_with(|c: char| c.is_lowercase()) {
                        path.rsplit('.').next().unwrap_or(path)
                    } else {
                        simple
                    };
                    // An explicit import in the declaring file names the real
                    // supertype: a workspace class sharing only the simple name
                    // (a `Fragment` data class next to androidx's) cannot vouch.
                    if let Some(fqn) = declaring_imports
                        .as_ref()
                        .and_then(|imports| imports.explicit.get(simple))
                    {
                        if self.workspace_index.find_symbol(fqn).is_none() {
                            return false;
                        }
                    }
                    simple == "Any"
                        || simple == "Object"
                        || self.supertype_chain_is_indexed(simple, visited)
                })
        })
    }

    fn has_external_supertype(&self, class_like: &KtSyntaxNode) -> bool {
        let mut after_colon = false;
        for element in class_like.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) => match token.kind() {
                    KtKind::COLON => after_colon = true,
                    KtKind::L_BRACE => break,
                    _ => {}
                },
                NodeOrToken::Node(node) => {
                    if node.kind() == KtKind::CLASS_BODY {
                        break;
                    }
                    if after_colon && node.kind() == KtKind::TYPE_REF {
                        // `RecyclerView.Adapter<T>`: the leftmost segment decides
                        // where the type comes from.
                        let Some(name) = identifier_tokens(&node).into_iter().next() else {
                            return true;
                        };
                        // An explicitly imported supertype is identified by its
                        // import: an unrelated workspace class that merely shares
                        // the simple name must not vouch for it.
                        let known = match self.imports.explicit.get(&name) {
                            Some(fqn) => {
                                self.workspace_index.find_symbol(fqn).is_some()
                                    && self.supertype_chain_is_indexed(
                                        fqn.rsplit('.').next().unwrap_or(fqn),
                                        &mut HashSet::new(),
                                    )
                            }
                            None => self.supertype_chain_is_indexed(&name, &mut HashSet::new()),
                        };
                        if !known {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

/// A `fun Receiver.name()` / `val Receiver.name` declaration: a DOT directly
/// follows a TYPE_REF child.
fn has_receiver_dot(declaration: &KtSyntaxNode) -> bool {
    let mut previous_was_type_ref = false;
    for element in declaration.children_with_tokens() {
        match &element {
            NodeOrToken::Node(node) => previous_was_type_ref = node.kind() == KtKind::TYPE_REF,
            NodeOrToken::Token(token) => {
                if token.kind() == KtKind::DOT && previous_was_type_ref {
                    return true;
                }
                if !token.kind().is_trivia() {
                    previous_was_type_ref = false;
                }
            }
        }
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReferenceRole {
    Value,
    Callee,
    MemberReceiver,
    MemberName,
}

fn reference_role(node: &KtSyntaxNode, token: &KtSyntaxToken) -> Option<ReferenceRole> {
    if node.ancestors().any(|ancestor| {
        matches!(
            ancestor.kind(),
            KtKind::IMPORT_DECL
                | KtKind::PACKAGE_DECL
                | KtKind::TYPE_REF
                | KtKind::NULLABLE_TYPE
                | KtKind::TYPE_PARAMETER
                | KtKind::TYPE_PARAMETER_LIST
                | KtKind::TYPE_ARGUMENT_LIST
                | KtKind::ANNOTATION
        )
    }) {
        return None;
    }

    match node.kind() {
        KtKind::PROPERTY_DECL | KtKind::PARAMETER => {
            token_after_assignment_marker(node, token).then_some(ReferenceRole::Value)
        }
        KtKind::FUNCTION_DECL => {
            token_after_kind(node, token, KtKind::EQ).then_some(ReferenceRole::Value)
        }
        KtKind::CLASS_DECL
        | KtKind::INTERFACE_DECL
        | KtKind::OBJECT_DECL
        | KtKind::CONSTRUCTOR_DECL
        | KtKind::TYPEALIAS_DECL
        | KtKind::LAMBDA_PARAMETERS
        | KtKind::FOR_LOOP_VAR
        | KtKind::DESTRUCTURING_DECL
        | KtKind::PROPERTY_ACCESSOR => None,
        KtKind::MEMBER_ACCESS_EXPR | KtKind::SAFE_ACCESS_EXPR => {
            let separator = node
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .find(|candidate| matches!(candidate.kind(), KtKind::DOT | KtKind::SAFE_CALL))?;
            if token.text_range().end() <= separator.text_range().start() {
                Some(ReferenceRole::MemberReceiver)
            } else {
                Some(ReferenceRole::MemberName)
            }
        }
        KtKind::CALL_EXPR => {
            // `Refine.unsafeCast<T>(x)`: with explicit type arguments the call
            // node is the right operand of the member access, so its callee
            // is a member name, not a free function.
            if node.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    KtKind::MEMBER_ACCESS_EXPR | KtKind::SAFE_ACCESS_EXPR
                ) && parent
                    .children_with_tokens()
                    .take_while(|element| element.as_node() != Some(node))
                    .any(|element| matches!(element.kind(), KtKind::DOT | KtKind::SAFE_CALL))
            }) {
                return Some(ReferenceRole::MemberName);
            }
            let lparen = node
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .find(|candidate| candidate.kind() == KtKind::L_PAREN);
            if lparen.is_some_and(|lparen| token.text_range().end() <= lparen.text_range().start())
            {
                Some(ReferenceRole::Callee)
            } else {
                Some(ReferenceRole::Value)
            }
        }
        KtKind::BINARY_EXPR
        | KtKind::UNARY_EXPR
        | KtKind::PAREN_EXPR
        | KtKind::ARRAY_ACCESS_EXPR
        | KtKind::RETURN_EXPR
        | KtKind::THROW_EXPR
        | KtKind::EXPR_STMT
        | KtKind::IF_EXPR
        | KtKind::WHEN_EXPR
        | KtKind::WHEN_ENTRY
        | KtKind::WHILE_EXPR
        | KtKind::FOR_EXPR
        | KtKind::DO_WHILE_EXPR
        | KtKind::TRY_EXPR
        | KtKind::CATCH_CLAUSE
        | KtKind::LAMBDA_EXPR => Some(ReferenceRole::Value),
        _ => None,
    }
}

/// `return@label`, `break@loop`, `this@Outer`: the name after `@` is a label.
fn is_jump_label(token: &KtSyntaxToken) -> bool {
    std::iter::successors(token.prev_sibling_or_token(), |sibling| {
        sibling.prev_sibling_or_token()
    })
    .find(|sibling| !sibling.kind().is_trivia())
    .is_some_and(|previous| previous.kind() == KtKind::AT)
}

/// Identifiers of the type after `is` / `!is` / `as` inside one node
/// (`is androidx.compose.State<*> ->`) name a type, not a value.
fn is_type_operand(node: &KtSyntaxNode, token: &KtSyntaxToken) -> bool {
    let mut in_type = false;
    for element in node.children_with_tokens() {
        let NodeOrToken::Token(candidate) = element else {
            continue;
        };
        if candidate.text_range() == token.text_range() {
            return in_type;
        }
        match candidate.kind() {
            KtKind::IS_KW | KtKind::NOT_IS | KtKind::AS_KW | KtKind::SAFE_CAST => in_type = true,
            KtKind::ARROW
            | KtKind::COMMA
            | KtKind::AMPAMP
            | KtKind::PIPEPIPE
            | KtKind::L_PAREN
            | KtKind::R_PAREN => in_type = false,
            _ => {}
        }
    }
    false
}

/// `f(name = value)`: the label is not a reference to anything in scope.
fn is_named_argument_label(node: &KtSyntaxNode, token: &KtSyntaxToken) -> bool {
    if node.kind() != KtKind::BINARY_EXPR
        || node
            .parent()
            .is_none_or(|parent| parent.kind() != KtKind::CALL_EXPR)
    {
        return false;
    }
    let mut significant = node
        .children_with_tokens()
        .filter(|element| !element.kind().is_trivia());
    let first_is_token = significant
        .next()
        .and_then(NodeOrToken::into_token)
        .is_some_and(|first| first.text_range() == token.text_range());
    first_is_token
        && significant
            .next()
            .is_some_and(|second| second.kind() == KtKind::EQ)
}

/// `a and b`, `x shl 2`: the middle identifier of an infix call names a
/// function resolved against the left operand's type, not a value in scope.
fn is_infix_function_name(node: &KtSyntaxNode, token: &KtSyntaxToken) -> bool {
    if node.kind() != KtKind::BINARY_EXPR {
        return false;
    }
    let significant: Vec<_> = node
        .children_with_tokens()
        .filter(|element| !element.kind().is_trivia())
        .collect();
    significant.len() == 3
        && significant[1]
            .as_token()
            .is_some_and(|middle| middle.text_range() == token.text_range())
}

fn token_after_assignment_marker(node: &KtSyntaxNode, token: &KtSyntaxToken) -> bool {
    token_after_kind(node, token, KtKind::EQ) || token_after_kind(node, token, KtKind::BY_KW)
}

fn token_after_kind(node: &KtSyntaxNode, token: &KtSyntaxToken, kind: KtKind) -> bool {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|candidate| candidate.kind() == kind)
        .is_some_and(|marker| token.text_range().start() >= marker.text_range().end())
}

fn is_inside_implicit_receiver_lambda(node: &KtSyntaxNode) -> bool {
    node.ancestors().any(|ancestor| {
        ancestor.kind() == KtKind::LAMBDA_EXPR
            && ancestor
                .children()
                .all(|child| child.kind() != KtKind::LAMBDA_PARAMETERS)
    })
}

fn extract_imports(root: &KtSyntaxNode) -> ImportMap {
    let mut imports = ImportMap::new();
    for node in root.children() {
        match node.kind() {
            KtKind::PACKAGE_DECL => {
                let parts = identifier_tokens(&node);
                if !parts.is_empty() {
                    imports.current_package = Some(parts.join("."));
                }
            }
            KtKind::IMPORT_DECL => {
                let mut parts = Vec::new();
                let mut alias = None;
                let mut collecting_alias = false;
                let mut wildcard = false;
                for element in node.descendants_with_tokens() {
                    let NodeOrToken::Token(token) = element else {
                        continue;
                    };
                    match token.kind() {
                        KtKind::AS_KW => collecting_alias = true,
                        KtKind::STAR => wildcard = true,
                        _ if is_identifier_token(&token) => {
                            let text = token.text().trim_matches('`').to_string();
                            if collecting_alias {
                                alias = Some(text);
                                collecting_alias = false;
                            } else {
                                parts.push(text);
                            }
                        }
                        _ => {}
                    }
                }
                if parts.is_empty() {
                    continue;
                }
                let fqn = parts.join(".");
                if wildcard {
                    imports.add_wildcard(fqn);
                } else if let Some(simple) = alias.or_else(|| parts.last().cloned()) {
                    imports.add_import(simple, fqn);
                }
            }
            _ => {}
        }
    }
    imports
}

fn collect_declared_names(root: &KtSyntaxNode) -> HashSet<String> {
    root.descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                KtKind::PROPERTY_DECL
                    | KtKind::PARAMETER
                    | KtKind::FUNCTION_DECL
                    | KtKind::CLASS_DECL
                    | KtKind::INTERFACE_DECL
                    | KtKind::OBJECT_DECL
                    | KtKind::LAMBDA_PARAMETERS
                    | KtKind::FOR_LOOP_VAR
                    | KtKind::DESTRUCTURING_DECL
            )
        })
        .flat_map(|node| declaration_names(&node))
        .collect()
}

fn collect_local_types(root: &KtSyntaxNode) -> HashSet<String> {
    root.descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                KtKind::CLASS_DECL | KtKind::INTERFACE_DECL | KtKind::OBJECT_DECL
            )
        })
        .flat_map(|node| identifier_tokens(&node).into_iter().take(1))
        .collect()
}

fn collect_type_parameters(root: &KtSyntaxNode) -> HashSet<String> {
    root.descendants()
        .filter(|node| node.kind() == KtKind::TYPE_PARAMETER)
        .flat_map(|node| identifier_tokens(&node).into_iter().take(1))
        .collect()
}

fn identifier_tokens(node: &KtSyntaxNode) -> Vec<String> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(is_identifier_token)
        .map(|token| token.text().trim_matches('`').to_string())
        .collect()
}

fn declaration_names(node: &KtSyntaxNode) -> Vec<String> {
    let stop_kind = match node.kind() {
        KtKind::PROPERTY_DECL
        | KtKind::PARAMETER
        | KtKind::FUNCTION_DECL
        | KtKind::DESTRUCTURING_DECL => Some(KtKind::EQ),
        KtKind::FOR_LOOP_VAR => Some(KtKind::IN_KW),
        _ => None,
    };
    if node.kind() == KtKind::PARAMETER {
        return crate::kotlin_resolver::parameter_name_token(node)
            .map(|token| vec![token.text().trim_matches('`').to_string()])
            .unwrap_or_default();
    }
    // Destructuring: `val (a, b) = pair` / `for ((k, v) in map)` declare every
    // name inside the parentheses.
    if matches!(node.kind(), KtKind::PROPERTY_DECL | KtKind::FOR_LOOP_VAR)
        && node
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .take_while(|token| !matches!(token.kind(), KtKind::EQ | KtKind::IN_KW))
            .any(|token| token.kind() == KtKind::L_PAREN)
    {
        return node
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .take_while(|token| !matches!(token.kind(), KtKind::EQ | KtKind::IN_KW))
            .filter(is_identifier_token)
            .map(|token| token.text().trim_matches('`').to_string())
            .collect();
    }
    let mut names = Vec::new();
    for element in node.children_with_tokens() {
        let NodeOrToken::Token(token) = element else {
            continue;
        };
        if stop_kind.is_some_and(|kind| token.kind() == kind) {
            break;
        }
        if is_identifier_token(&token) {
            names.push(token.text().trim_matches('`').to_string());
            if !matches!(
                node.kind(),
                KtKind::LAMBDA_PARAMETERS | KtKind::DESTRUCTURING_DECL
            ) {
                break;
            }
        }
    }
    names
}
