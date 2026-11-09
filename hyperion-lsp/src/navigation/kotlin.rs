// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin-specific navigation helpers
//!
//! This module contains goto-definition and type navigation logic for Kotlin files.
//! Extracted from provider.rs to reduce file size and improve organization.

use hyperion_semantics::db::{FileId, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_workspace::{DeclarationNamespace, SymbolTarget, TypeNameResolution, WorkspaceIndex};
use regex::Regex;
use rowan::TextSize;
use std::sync::Arc;
use std::sync::LazyLock;
use tower_lsp::lsp_types::{Location, Position, Range, Url};
use tracing::{debug, info};

use crate::semantic_host::ReadTxn;

use crate::navigation_util::{offset_to_position, position_to_offset};

static KOTLIN_VALVAR_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:val|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([A-Za-z0-9_.]+)")
        .expect("valid kotlin val/var annotation regex")
});

static KOTLIN_VALVAR_CTOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:val|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([A-Za-z0-9_.]+)\s*\(")
        .expect("valid kotlin val/var constructor regex")
});

fn is_kotlin_identifier_token(token: &hyperion_kotlin::SyntaxToken) -> bool {
    if token.kind() == hyperion_kotlin::SyntaxKind::IDENT {
        return true;
    }

    let text = token.text();
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }

    chars.all(|c| c == '_' || c.is_alphanumeric())
}

fn token_is_call_callee(token: &hyperion_kotlin::SyntaxToken) -> bool {
    token
        .parent_ancestors()
        .find(|node| node.kind() == hyperion_kotlin::SyntaxKind::CALL_EXPR)
        .and_then(|call| {
            call.children_with_tokens()
                .filter_map(|element| element.into_token())
                .find(|token| token.kind() == hyperion_kotlin::SyntaxKind::L_PAREN)
        })
        .is_some_and(|lparen| token.text_range().end() <= lparen.text_range().start())
}

/// Helper struct for Kotlin navigation operations
///
/// Holds references to shared resources needed for Kotlin symbol resolution.
pub(crate) struct KotlinNavigationHelper<'a> {
    pub txn: &'a ReadTxn,
    pub workspace_index: &'a Arc<WorkspaceIndex>,
    pub source_providers: &'a Arc<crate::source_provider::SourceProviderChain>,
}

impl<'a> KotlinNavigationHelper<'a> {
    /// Create a new Kotlin navigation helper
    pub const fn new(
        txn: &'a ReadTxn,
        workspace_index: &'a Arc<WorkspaceIndex>,
        source_providers: &'a Arc<crate::source_provider::SourceProviderChain>,
    ) -> Self {
        Self {
            txn,
            workspace_index,
            source_providers,
        }
    }

    /// Kotlin-specific goto-definition implementation
    ///
    /// This implementation uses a three-tier resolution strategy:
    /// 1. **Local scope** - Check for local variables, parameters, and function-local symbols
    /// 2. **Workspace index** - Check for cross-file symbols (classes, functions, properties)
    /// 3. **Fallback** - Return None if not found
    ///
    /// This order ensures that local symbols shadow global symbols correctly.
    pub fn goto_definition(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        debug!(
            "NAV: Kotlin goto-definition for {:?} at {:?}",
            file_id, position
        );

        // Parse the Kotlin file. try_lock_for: degrade to no result rather
        // than freeze if did_change is currently holding the DB mutex.
        let parse_tree = {
            let snapshot = self.txn.snapshot();
            snapshot.parse_file(file_id)
        };

        // Convert position to offset
        let offset = position_to_offset(source_text, position)?;

        // Create Kotlin SyntaxNode from the green tree
        let kotlin_root: hyperion_kotlin::SyntaxNode =
            hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());

        // Find token at position
        let token_offset = kotlin_root.token_at_offset(TextSize::from(offset));
        let token = token_offset.right_biased().ok_or("No token at position")?;

        debug!(
            "NAV: Kotlin token: {:?} (kind: {:?})",
            token.text(),
            token.kind()
        );

        // Check if cursor is in a string template ($identifier pattern)
        // For Kotlin strings like "Hello, $name", we want to resolve $name
        use hyperion_kotlin::SyntaxKind as KtKind;
        if matches!(
            token.kind(),
            KtKind::STRING_LITERAL | KtKind::RAW_STRING_LITERAL
        ) {
            // Try to extract identifier from string template
            if let Some(template_identifier) =
                self.extract_string_template_identifier(&token, offset)
            {
                debug!(
                    "NAV: Found string template identifier: ${}",
                    template_identifier
                );
                // Use this identifier for local scope resolution
                return self.resolve_identifier(
                    file_id,
                    &template_identifier,
                    source_text,
                    uri,
                    offset,
                );
            }
            // Not on a template identifier, skip
            debug!("NAV: Cursor in string literal (not on template), skipping");
            return Ok(None);
        }

        // Check if cursor is in comment - return None early
        if matches!(token.kind(), KtKind::LINE_COMMENT | KtKind::BLOCK_COMMENT) {
            debug!("NAV: Cursor in comment, skipping Kotlin goto-definition");
            return Ok(None);
        }

        // Check if cursor is on a type reference

        // Check if this token is part of a TYPE_REF node
        if let Some(parent) = token.parent() {
            let mut current = Some(parent);
            while let Some(node) = current {
                if node.kind() == KtKind::TYPE_REF {
                    debug!("NAV: Cursor is on TYPE_REF - handling type navigation");
                    return self.goto_type_definition(file_id, &token);
                }
                current = node.parent();
            }
        }

        // Only handle identifier-like tokens (IDENT + soft-keyword identifiers like `where`)
        if !is_kotlin_identifier_token(&token) {
            debug!("NAV: Not an identifier, skipping");
            return Ok(None);
        }

        let identifier = token.text().to_string();
        debug!("NAV: Looking up identifier: {}", identifier);

        // PHASE -1: Check if cursor is on a member access (receiver.member)
        // Use type inference to resolve the member
        if let Some(parent) = token.parent() {
            let mut current = Some(parent.clone());
            while let Some(node) = current {
                if matches!(
                    node.kind(),
                    KtKind::MEMBER_ACCESS_EXPR | KtKind::SAFE_ACCESS_EXPR
                ) {
                    debug!("NAV: Cursor is on member access - using type inference");
                    if let Some(location) = self.resolve_member_access(file_id, &node, &token) {
                        return Ok(Some(location));
                    }
                }
                current = node.parent();
            }

            // Fallback: Kotlin parser sometimes flattens `Receiver.member()` into flat
            // tokens under PROPERTY_DECL/EXPR_STMT instead of creating MEMBER_ACCESS_EXPR.
            // Detect the pattern: ... IDENT("Receiver") DOT IDENT("member") ...
            if let Some(dot) = token.prev_sibling_or_token() {
                if dot.kind() == KtKind::DOT {
                    // Fast path: IDENT DOT IDENT
                    let mut receiver_name = dot.prev_sibling_or_token().and_then(|receiver_tok| {
                        (receiver_tok.kind() == KtKind::IDENT)
                            .then(|| receiver_tok.as_token().map(|t| t.text().to_string()))
                            .flatten()
                    });

                    // Extended fallback: chained/flattened access where token before DOT is `)`.
                    // Walk left to find the chain root identifier that is not itself preceded by DOT.
                    if receiver_name.is_none() {
                        let mut cursor = dot.prev_sibling_or_token();
                        while let Some(elem) = cursor.clone() {
                            if elem.kind() == KtKind::IDENT {
                                let mut prev_non_ws = elem.prev_sibling_or_token();
                                while let Some(prev) = prev_non_ws.clone() {
                                    if prev.kind() == KtKind::WHITESPACE {
                                        prev_non_ws = prev.prev_sibling_or_token();
                                    } else {
                                        break;
                                    }
                                }

                                if prev_non_ws
                                    .as_ref()
                                    .is_none_or(|prev| prev.kind() != KtKind::DOT)
                                {
                                    receiver_name = elem.as_token().map(|t| t.text().to_string());
                                    break;
                                }
                            }
                            cursor = elem.prev_sibling_or_token();
                        }
                    }

                    // Last-resort for flattened call chains split into ERROR nodes:
                    // infer root receiver from source assignment RHS, e.g.
                    // `val query = Query()` followed by `.where(...)`.
                    if receiver_name.is_none() {
                        receiver_name =
                            self.infer_flat_chain_root_receiver_from_source(source_text, offset);
                    }

                    if let Some(receiver_name) = receiver_name {
                        debug!(
                            "NAV: Detected flat member access: {}.{}",
                            receiver_name, identifier
                        );
                        if let Some(loc) = self.resolve_flat_member_access(
                            file_id,
                            &receiver_name,
                            &identifier,
                            token_is_call_callee(&token),
                            offset as usize,
                        ) {
                            return Ok(Some(loc));
                        }
                    }
                }
            }
        }

        // PHASE 0: Check if cursor is on an import statement
        // Navigate from import to the imported symbol
        if let Some(parent) = token.parent() {
            let mut current = Some(parent);
            while let Some(node) = current {
                if node.kind() == KtKind::IMPORT_DECL {
                    debug!("NAV: Cursor is on an import statement");

                    // Extract the full import path (excluding alias)
                    let mut path_parts = Vec::new();
                    let mut collecting_alias = false;

                    for child_token in node.descendants_with_tokens() {
                        if let Some(t) = child_token.as_token() {
                            match t.kind() {
                                KtKind::IDENT => {
                                    if collecting_alias {
                                        // Skip the alias - we want the FQN being imported
                                        collecting_alias = false;
                                    } else {
                                        path_parts.push(t.text().to_string());
                                    }
                                }
                                KtKind::AS_KW => {
                                    collecting_alias = true;
                                }
                                _ => {}
                            }
                        }
                    }

                    if !path_parts.is_empty() {
                        let fqn = path_parts.join(".");
                        debug!("NAV: Import FQN: {}", fqn);

                        return self.unique_typed_location(
                            &fqn,
                            &[
                                DeclarationNamespace::Type,
                                DeclarationNamespace::Callable,
                                DeclarationNamespace::Value,
                            ],
                        );
                    }

                    // Import statement handled, don't continue with other resolution
                    return Ok(None);
                }
                current = node.parent();
            }
        }

        // PHASE 1: Check local scope first (local variables, parameters, etc.)
        let snapshot = self.txn.snapshot();

        let scope_graph = snapshot.scope_graph(file_id);
        debug!("NAV: Checking local scope for '{}'", identifier);
        if let Some(symbol) = scope_graph.resolve_symbol(&identifier, offset as usize) {
            info!(
                "NAV: Resolved '{}' via LOCAL SCOPE (kind: {:?})",
                identifier, symbol.kind
            );

            // Convert the ScopeSymbol definition_range to an LSP Location
            let start_pos = offset_to_position(source_text, symbol.definition_range.start().into());
            let end_pos = offset_to_position(source_text, symbol.definition_range.end().into());

            return Ok(Some(Location {
                uri: uri.clone(),
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
            }));
        }

        debug!("NAV: Not found in local scope, checking workspace index");

        let namespaces: &[DeclarationNamespace] = if token_is_call_callee(&token) {
            &[
                DeclarationNamespace::Callable,
                DeclarationNamespace::Type,
                DeclarationNamespace::Value,
            ]
        } else {
            &[DeclarationNamespace::Value, DeclarationNamespace::Type]
        };
        self.resolve_workspace_identifier(file_id, &identifier, namespaces)
    }

    /// Navigate from type reference to type definition (Kotlin)
    ///
    /// Handles navigation from type annotations like:
    /// - `val user: User` -> navigate to User class
    /// - `val items: List<User>` -> navigate to List or User depending on cursor position
    /// - `fun test(x: String): Int` -> navigate to String or Int
    pub fn goto_type_definition(
        &self,
        file_id: FileId,
        token: &hyperion_kotlin::SyntaxToken,
    ) -> Result<Option<Location>, String> {
        debug!(
            "NAV: goto_type_definition_kotlin for token: {}",
            token.text()
        );

        // Extract the type name at cursor position
        // For `List<User>`, if cursor is on "List", we want "List"
        // If cursor is on "User", we want "User"
        let type_name = token.text().to_string();
        debug!("NAV: Type name at cursor: {}", type_name);

        let TypeNameResolution::Exact(fqn) =
            self.workspace_index
                .resolve_type_name(file_id, &[], &type_name)
        else {
            debug!("NAV: Type '{}' is missing or ambiguous", type_name);
            return Ok(None);
        };

        // Kotlin built-ins have a source-language declaration name and a
        // canonical JVM identity. Semantic consumers need the latter, but
        // navigation should still prefer an indexed source declaration such
        // as `kotlin.String` over the mapped `java.lang.String`. Keep this as
        // a presentation step after the ambiguity-aware canonical binding has
        // succeeded so aliases cannot weaken name-resolution correctness.
        if let Some(definition) = self.kotlin_builtin_source_definition(file_id, &type_name, &fqn) {
            return self.symbol_location_to_lsp_location(&definition.location);
        }

        let Some(definition) = self
            .workspace_index
            .find_symbol_definition(&SymbolTarget::new(fqn, DeclarationNamespace::Type))
        else {
            return Ok(None);
        };
        self.symbol_location_to_lsp_location(&definition.location)
    }

    /// Find the source-facing declaration behind one canonical Kotlin built-in.
    ///
    /// Kotlin's `String`, collection interfaces, and several other types map
    /// to JVM declarations for identity and member lookup. When Kotlin source
    /// for the language declaration is indexed, go-to-definition should land
    /// there. Explicit and star imports retain their normal precedence; the
    /// language defaults are consulted only when neither source tier binds the
    /// written name.
    fn kotlin_builtin_source_definition(
        &self,
        file_id: FileId,
        written: &str,
        canonical_fqn: &str,
    ) -> Option<hyperion_workspace::ResolvedDefinition> {
        let imports = self.workspace_index.get_imports(file_id)?;
        let find_alias = |candidate: &str| {
            (hyperion_workspace::kotlin_builtin_jvm_fqn(candidate) == Some(canonical_fqn))
                .then(|| {
                    self.workspace_index
                        .find_symbol_definition(&SymbolTarget::new(
                            candidate,
                            DeclarationNamespace::Type,
                        ))
                })
                .flatten()
        };

        if let Some(explicit) = imports.explicit.get(written) {
            return find_alias(explicit);
        }

        let source_imports = imports
            .wildcards
            .iter()
            .map(|package| format!("{package}.{written}"))
            .collect::<Vec<_>>();
        if source_imports.iter().any(|candidate| {
            hyperion_workspace::kotlin_builtin_jvm_fqn(candidate) == Some(canonical_fqn)
        }) {
            return source_imports
                .iter()
                .find_map(|candidate| find_alias(candidate));
        }

        hyperion_workspace::implicit_type_import_packages(Some(
            hyperion_workspace::Language::Kotlin,
        ))
        .iter()
        .map(|package| format!("{package}.{written}"))
        .find_map(|candidate| find_alias(&candidate))
    }

    /// Resolve member access using type inference (receiver.member)
    ///
    /// This uses the Kotlin type inference engine to determine the receiver's type,
    /// then looks up the member in that type's class.
    pub fn resolve_member_access(
        &self,
        file_id: FileId,
        member_access_node: &hyperion_kotlin::SyntaxNode,
        cursor_token: &hyperion_kotlin::SyntaxToken,
    ) -> Option<Location> {
        use hyperion_kotlin::SyntaxKind as KtKind;
        use rowan::NodeOrToken;

        debug!("NAV: Resolving Kotlin member access with type inference");

        if std::env::var("HYPERION_DEBUG_KOTLIN_MEMBER_ACCESS").is_ok() {
            eprintln!(
                "DEBUG kotlin member access: cursor_token kind={:?} text={:?} range={:?}",
                cursor_token.kind(),
                cursor_token.text(),
                cursor_token.text_range()
            );
            for child in member_access_node.children_with_tokens() {
                match child {
                    NodeOrToken::Node(n) => eprintln!(
                        "DEBUG kotlin member access: child node kind={:?} range={:?} text={:?}",
                        n.kind(),
                        n.text_range(),
                        n.text().to_string()
                    ),
                    NodeOrToken::Token(t) => eprintln!(
                        "DEBUG kotlin member access: child token kind={:?} range={:?} text={:?}",
                        t.kind(),
                        t.text_range(),
                        t.text()
                    ),
                }
            }
        }

        // Extract receiver and member from the member access expression
        let mut receiver_node = None;
        let mut receiver_token_text: Option<String> = None;
        let mut member_name = None;
        let mut seen_dot = false;

        for child in member_access_node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => {
                    // First expression node is the receiver
                    if receiver_node.is_none() {
                        receiver_node = Some(n);
                    }
                }
                NodeOrToken::Token(t) => {
                    if t.kind() == KtKind::DOT {
                        seen_dot = true;
                        continue;
                    }

                    // Identifier token after DOT is the member
                    if is_kotlin_identifier_token(&t) && !seen_dot && receiver_token_text.is_none()
                    {
                        receiver_token_text = Some(t.text().to_string());
                    }

                    if is_kotlin_identifier_token(&t) && t.text_range() == cursor_token.text_range()
                    {
                        member_name = Some(t.text().to_string());
                    }
                }
            }
        }

        let member_name = member_name?;

        debug!("NAV: Member access - member name: {}", member_name);

        // Build a symbol table for this file to enable type resolution.
        // try_lock_for: degrade to no result rather than freeze if did_change
        // is currently holding the DB mutex.
        let symbol_table = {
            let snapshot = self.txn.snapshot();
            snapshot.file_symbol_table(file_id)
        };

        // Create a symbol resolver for local scope lookups
        let mut resolver = hyperion_semantics::SymbolResolver::with_cached_table(symbol_table);

        // Create the Kotlin type inference engine
        let inference_engine = hyperion_semantics::KotlinTypeInferenceEngine::new(
            self.workspace_index.clone(),
            file_id,
        );

        // Infer the type of the receiver
        let mut receiver_type = match receiver_node.as_ref() {
            Some(node) => inference_engine.infer_expression(node, &mut resolver),
            None => hyperion_semantics::Type::Unknown,
        };
        debug!("NAV: Inferred receiver type: {:?}", receiver_type);

        let receiver_name = receiver_node
            .as_ref()
            .and_then(|node| {
                node.descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .find(|t| t.kind() == KtKind::IDENT)
                    .map(|t| t.text().to_string())
            })
            .or_else(|| receiver_token_text.clone());

        if std::env::var("HYPERION_DEBUG_KOTLIN_MEMBER_ACCESS").is_ok() {
            eprintln!(
                "DEBUG kotlin member access: receiver_name={:?} receiver_type_initial={:?} member_name={}",
                receiver_name,
                receiver_type,
                member_name
            );
        }

        if matches!(receiver_type, hyperion_semantics::Type::Unknown) {
            // Kotlin symbol tables don't always store inferred types for local vals/vars.
            // Fall back to the Kotlin scope graph, which tracks type annotations and some
            // initializer call information.
            if let Some(receiver_name) = receiver_name.as_deref() {
                let snapshot = self.txn.snapshot();
                let scope_graph = snapshot.scope_graph(file_id);
                let pos: usize = member_access_node.text_range().start().into();
                if let Some(symbol) = scope_graph.resolve_symbol(receiver_name, pos) {
                    if let Some(ty) = &symbol.type_annotation {
                        if let hyperion_kotlin::types::KotlinType::Class { fqn, .. } = ty {
                            let (name, package) = fqn.rsplit_once('.').map_or_else(
                                || (fqn.clone(), None),
                                |(p, n)| (n.to_string(), Some(p.to_string())),
                            );
                            receiver_type = hyperion_semantics::Type::Class(
                                hyperion_semantics::symbol::ClassType {
                                    name,
                                    package,
                                    type_arguments: vec![],
                                    is_diamond: false,
                                },
                            );
                        }
                    } else if let Some(call) = &symbol.method_call_info {
                        // Common case: `val user = JavaUser()` (constructor call).
                        if call.receiver_name.is_none() && !call.method_name.is_empty() {
                            let method = call.method_name.as_str();
                            if method
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_ascii_uppercase())
                            {
                                receiver_type = hyperion_semantics::Type::Class(
                                    hyperion_semantics::symbol::ClassType {
                                        name: method.to_string(),
                                        package: None,
                                        type_arguments: vec![],
                                        is_diamond: false,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        if matches!(receiver_type, hyperion_semantics::Type::Unknown) {
            // Last-resort heuristic: infer local val/var type directly from source text.
            // This keeps Kotlin→Java navigation working even when the cached symbol table
            // doesn't include local variable inference.
            if let Some(receiver_name) = receiver_name.as_deref() {
                let snapshot = self.txn.snapshot();
                let source = snapshot.file_text(file_id);
                let limit: usize = member_access_node.text_range().start().into();
                let source_prefix = source
                    .get(..limit.min(source.len()))
                    .unwrap_or(source.as_str());
                let type_from_annotation = KOTLIN_VALVAR_TYPE_RE
                    .captures_iter(source_prefix)
                    .filter(|c| c.get(1).is_some_and(|m| m.as_str() == receiver_name))
                    .filter_map(|c| c.get(2))
                    .last()
                    .map(|m| m.as_str().to_string());

                let type_from_ctor = KOTLIN_VALVAR_CTOR_RE
                    .captures_iter(source_prefix)
                    .filter(|c| c.get(1).is_some_and(|m| m.as_str() == receiver_name))
                    .filter_map(|c| c.get(2))
                    .last()
                    .map(|m| m.as_str().to_string());

                if let Some(fqn) = type_from_annotation.or(type_from_ctor) {
                    let (name, package) = match fqn.rsplit_once('.') {
                        Some((p, n)) => (n.to_string(), Some(p.to_string())),
                        None => (fqn, None),
                    };
                    receiver_type =
                        hyperion_semantics::Type::Class(hyperion_semantics::symbol::ClassType {
                            name,
                            package,
                            type_arguments: vec![],
                            is_diamond: false,
                        });
                }
            }
        }

        // Look up the member in the receiver's type
        if let hyperion_semantics::Type::Class(class_type) = receiver_type {
            let namespace = if member_name.chars().next().is_some_and(char::is_uppercase) {
                hyperion_workspace::DeclarationNamespace::Type
            } else if token_is_call_callee(cursor_token) {
                hyperion_workspace::DeclarationNamespace::Callable
            } else {
                hyperion_workspace::DeclarationNamespace::Value
            };
            let mut class_fqn = class_type.qualified_name().into_owned();
            if !class_fqn.contains('.') || !self.workspace_index.has_class(&class_fqn) {
                if let hyperion_workspace::TypeNameResolution::Exact(resolved) = self
                    .workspace_index
                    .resolve_type_name(file_id, &[], &class_fqn)
                {
                    class_fqn = resolved;
                }

                if !self.workspace_index.has_class(&class_fqn) {
                    return None;
                }
            }
            let receiver_simple_name = class_fqn.rsplit('.').next().unwrap_or(&class_fqn);
            let member_fqn = format!("{class_fqn}.{member_name}");

            debug!("NAV: Looking up member FQN: {}", member_fqn);

            // `c.radius` on a Java class means `getRadius()`; a private field of
            // the same name is not what Kotlin sees.
            if namespace == hyperion_workspace::DeclarationNamespace::Value {
                let direct_is_usable = self
                    .workspace_index
                    .get_symbol_target(&hyperion_workspace::SymbolTarget::new(
                        member_fqn.as_str(),
                        namespace,
                    ))
                    .is_some_and(|symbol| {
                        !(symbol.kind == lsp_types::SymbolKind::FIELD
                            && symbol.modifiers.is_private)
                    });
                if !direct_is_usable {
                    for getter in hyperion_types::jvm_names::java_getter_names(&member_name) {
                        let target = hyperion_workspace::SymbolTarget::new(
                            format!("{class_fqn}.{getter}"),
                            hyperion_workspace::DeclarationNamespace::Callable,
                        );
                        if let Some(definition) =
                            self.workspace_index.find_symbol_definition(&target)
                        {
                            return self
                                .symbol_location_to_lsp_location(&definition.location)
                                .ok()?;
                        }
                    }
                }
            }

            // First, try to find the member in workspace index (regular method)
            if let Some(symbol_loc) = self
                .workspace_index
                .find_symbol_definition(&hyperion_workspace::SymbolTarget::new(
                    member_fqn.as_str(),
                    namespace,
                ))
                .map(|definition| definition.location)
            {
                info!("NAV: Resolved member '{}' via TYPE INFERENCE", member_name);
                let location = self.symbol_location_to_lsp_location(&symbol_loc);
                if let Err(ref e) = location {
                    debug!(
                        "Failed to convert symbol location to LSP location for member '{}': {}",
                        member_name, e
                    );
                }
                return location.ok()?;
            }

            // Kotlin companion object: `Factory.create()` indexes as `Factory.Companion.create`
            let companion_fqn = format!("{class_fqn}.Companion.{member_name}");
            debug!("NAV: Trying companion object FQN: {}", companion_fqn);
            if let Some(symbol_loc) = self
                .workspace_index
                .find_symbol_definition(&hyperion_workspace::SymbolTarget::new(
                    companion_fqn.as_str(),
                    namespace,
                ))
                .map(|definition| definition.location)
            {
                info!(
                    "NAV: Resolved '{}' as companion object member of '{}'",
                    member_name, class_fqn
                );
                let location = self.symbol_location_to_lsp_location(&symbol_loc);
                if let Err(ref e) = location {
                    debug!(
                        "Failed to convert companion symbol location for '{}': {}",
                        companion_fqn, e
                    );
                }
                return location.ok()?;
            }

            // Member not found as regular method - try extension functions
            debug!(
                "NAV: Member '{}' not found as method, checking extension functions for '{}'",
                member_name, receiver_simple_name
            );

            // Get imports for scope checking
            if let Some(imports) = self.workspace_index.get_imports(file_id) {
                // Find extension functions matching receiver type and method name.
                // Extension precedence is tiered; declaration/index order must
                // never decide between two star-imported functions.
                let candidates = self
                    .workspace_index
                    .find_extension_by_name(receiver_simple_name, &member_name);

                let explicit = imports
                    .explicit
                    .get(&member_name)
                    .into_iter()
                    .filter_map(|imported| {
                        let target =
                            SymbolTarget::new(imported.as_str(), DeclarationNamespace::Callable);
                        self.workspace_index
                            .get_symbol_target(&target)
                            .and_then(|symbol| {
                                symbol.receiver_type.as_ref().and_then(|receiver| {
                                    let receiver = receiver.split('<').next().unwrap_or(receiver);
                                    (receiver == receiver_simple_name
                                        || receiver
                                            .rsplit('.')
                                            .next()
                                            .is_some_and(|simple| simple == receiver_simple_name))
                                    .then(|| imported.clone())
                                })
                            })
                    })
                    .collect::<Vec<_>>();
                let same_package = imports
                    .current_package
                    .as_deref()
                    .into_iter()
                    .flat_map(|package| {
                        candidates
                            .iter()
                            .filter(move |candidate| {
                                candidate
                                    .fqn
                                    .rsplit_once('.')
                                    .is_some_and(|(owner_package, _)| owner_package == package)
                            })
                            .map(|candidate| candidate.fqn.to_string())
                    })
                    .collect::<Vec<_>>();
                let star_imported = candidates
                    .iter()
                    .filter(|candidate| {
                        candidate.fqn.rsplit_once('.').is_some_and(|(package, _)| {
                            imports.wildcards.iter().any(|wildcard| wildcard == package)
                        })
                    })
                    .map(|candidate| candidate.fqn.to_string())
                    .collect::<Vec<_>>();

                for mut tier in [explicit, same_package, star_imported] {
                    tier.sort_unstable();
                    tier.dedup();
                    if tier.is_empty() {
                        continue;
                    }
                    let [extension_fqn] = tier.as_slice() else {
                        debug!(
                            "NAV: Extension '{}' is ambiguous for receiver '{}'",
                            member_name, receiver_simple_name
                        );
                        return None;
                    };
                    let definition =
                        self.workspace_index
                            .find_symbol_definition(&SymbolTarget::new(
                                extension_fqn.as_str(),
                                DeclarationNamespace::Callable,
                            ))?;
                    info!(
                        "NAV: Resolved '{}' as extension function on '{}'",
                        member_name, receiver_simple_name
                    );
                    return self
                        .symbol_location_to_lsp_location(&definition.location)
                        .ok()?;
                }

                debug!(
                    "NAV: No extension function '{}' found in scope for type '{}'",
                    member_name, receiver_simple_name
                );
            }

            None
        } else {
            debug!(
                "NAV: Cannot resolve member on non-class type: {:?}",
                receiver_type
            );
            None
        }
    }

    /// Resolve a flat `Receiver.member` pattern when the parser hasn't created
    /// a `MEMBER_ACCESS_EXPR` node. Resolves the receiver to a class FQN via
    /// imports, then looks up the member (including companion object fallback).
    fn resolve_flat_member_access(
        &self,
        file_id: FileId,
        receiver_name: &str,
        member_name: &str,
        is_call: bool,
        cursor_offset: usize,
    ) -> Option<Location> {
        let mut class_fqn = self.resolve_simple_name_to_fqn(file_id, receiver_name);
        if class_fqn.is_none() {
            class_fqn = self.resolve_local_receiver_type_fqn(file_id, receiver_name);
        }
        if class_fqn.is_none() && receiver_name == "it" {
            class_fqn = self.resolve_implicit_it_receiver_type_fqn(file_id, cursor_offset);
        }
        let class_fqn = class_fqn?;
        debug!(
            "NAV: Flat member access: resolved '{}' to FQN '{}'",
            receiver_name, class_fqn
        );

        // Try direct member lookup
        let member_fqn = format!("{class_fqn}.{member_name}");
        let namespace = if member_name.chars().next().is_some_and(char::is_uppercase) {
            hyperion_workspace::DeclarationNamespace::Type
        } else if is_call {
            hyperion_workspace::DeclarationNamespace::Callable
        } else {
            hyperion_workspace::DeclarationNamespace::Value
        };
        if let Some(symbol_loc) = self
            .workspace_index
            .find_symbol_definition(&hyperion_workspace::SymbolTarget::new(
                member_fqn.as_str(),
                namespace,
            ))
            .map(|definition| definition.location)
        {
            info!(
                "NAV: Resolved flat member '{}' via direct lookup",
                member_fqn
            );
            return self.symbol_location_to_lsp_location(&symbol_loc).ok()?;
        }

        // Kotlin companion object: `Factory.create()` indexes as `Factory.Companion.create`
        let companion_fqn = format!("{class_fqn}.Companion.{member_name}");
        if let Some(symbol_loc) = self
            .workspace_index
            .find_symbol_definition(&hyperion_workspace::SymbolTarget::new(
                companion_fqn.as_str(),
                namespace,
            ))
            .map(|definition| definition.location)
        {
            info!(
                "NAV: Resolved flat member '{}' as companion object member",
                companion_fqn
            );
            return self.symbol_location_to_lsp_location(&symbol_loc).ok()?;
        }

        None
    }

    fn resolve_local_receiver_type_fqn(
        &self,
        file_id: FileId,
        receiver_name: &str,
    ) -> Option<String> {
        let snapshot = self.txn.snapshot();
        let source = snapshot.file_text(file_id);

        let from_annotation = KOTLIN_VALVAR_TYPE_RE
            .captures_iter(&source)
            .filter(|c| c.get(1).is_some_and(|m| m.as_str() == receiver_name))
            .filter_map(|c| c.get(2))
            .last()
            .map(|m| m.as_str().to_string());

        let from_ctor = KOTLIN_VALVAR_CTOR_RE
            .captures_iter(&source)
            .filter(|c| c.get(1).is_some_and(|m| m.as_str() == receiver_name))
            .filter_map(|c| c.get(2))
            .last()
            .map(|m| m.as_str().to_string());

        let mut type_name = from_annotation.or(from_ctor)?;
        if let Some(base) = type_name.split('<').next() {
            type_name = base.trim().to_string();
        }

        if type_name.contains('.') && self.workspace_index.has_class(&type_name) {
            return Some(type_name);
        }

        self.resolve_simple_name_to_fqn(file_id, &type_name)
    }

    fn resolve_implicit_it_receiver_type_fqn(
        &self,
        file_id: FileId,
        cursor_offset: usize,
    ) -> Option<String> {
        let snapshot = self.txn.snapshot();
        let scope_graph = snapshot.scope_graph(file_id);
        let mut symbol = scope_graph.resolve_symbol("it", cursor_offset);
        if symbol.is_none() {
            // Jitter: small deltas (1–4) cover token-boundary misalignment;
            // larger deltas (8–64) reach past multi-token expressions in lambda
            // bodies where `resolve_symbol` can't find implicit `it` at the
            // exact cursor offset.
            for delta in [1usize, 2, 3, 4, 8, 16, 32, 64] {
                symbol = scope_graph.resolve_symbol("it", cursor_offset.saturating_sub(delta));
                if symbol.is_some() {
                    break;
                }
            }
        }

        if let Some(symbol) = symbol {
            if let Some(hyperion_kotlin::types::KotlinType::Class { fqn, .. }) =
                symbol.type_annotation.as_ref()
            {
                if fqn != "kotlin.Any" {
                    return Some(fqn.clone());
                }
            }
        }

        self.resolve_implicit_it_from_source(&snapshot.file_text(file_id), file_id, cursor_offset)
    }

    fn resolve_implicit_it_from_source(
        &self,
        source: &str,
        file_id: FileId,
        cursor_offset: usize,
    ) -> Option<String> {
        let prefix = source.get(..cursor_offset.min(source.len()))?;

        let methods = [
            "map",
            "filter",
            "filterNot",
            "forEach",
            "any",
            "all",
            "none",
            "count",
            "find",
            "first",
            "firstOrNull",
            "last",
            "lastOrNull",
        ];

        let mut best_idx = None;
        let mut best_method = None;
        for method in methods {
            let needle = format!(".{method}");
            if let Some(idx) = prefix.rfind(&needle) {
                if best_idx.is_none_or(|b| idx > b) {
                    best_idx = Some(idx);
                    best_method = Some(method);
                }
            }
        }
        let idx = best_idx?;
        let _method = best_method?;

        // Extract receiver identifier before ".method"
        let bytes = prefix.as_bytes();
        let mut end = idx;
        while end > 0 && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        let mut start = end;
        while start > 0 {
            let c = bytes[start - 1] as char;
            if c.is_ascii_alphanumeric() || c == '_' {
                start -= 1;
            } else {
                break;
            }
        }
        if start == end {
            return None;
        }
        let receiver_name = prefix[start..end].trim();

        self.resolve_local_collection_element_type_fqn(
            source,
            file_id,
            receiver_name,
            cursor_offset,
        )
    }

    fn resolve_local_collection_element_type_fqn(
        &self,
        source: &str,
        file_id: FileId,
        receiver_name: &str,
        cursor_offset: usize,
    ) -> Option<String> {
        let prefix = source.get(..cursor_offset.min(source.len()))?;

        for line in prefix.lines().rev() {
            let trimmed = line.trim();
            if !(trimmed.starts_with("val ") || trimmed.starts_with("var ")) {
                continue;
            }
            let Some(eq_idx) = trimmed.find('=') else {
                continue;
            };
            let lhs = trimmed[..eq_idx].trim();
            let rhs = trimmed[eq_idx + 1..].trim();

            let lhs_body = lhs
                .trim_start_matches("val ")
                .trim_start_matches("var ")
                .trim();
            let decl_name = lhs_body
                .split(|c: char| c == ':' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if decl_name != receiver_name {
                continue;
            }

            // Explicit type annotation: val users: List<User> = ...
            if let Some(colon_idx) = lhs_body.find(':') {
                let annotated = lhs_body[colon_idx + 1..].trim();
                if let Some(lt) = annotated.find('<') {
                    if let Some(gt_rel) = annotated[lt + 1..].find('>') {
                        let arg = annotated[lt + 1..lt + 1 + gt_rel]
                            .split(',')
                            .next()
                            .unwrap_or("")
                            .trim();
                        if !arg.is_empty() {
                            return self.resolve_simple_name_to_fqn(file_id, arg);
                        }
                    }
                }
            }

            // Collection initializer with explicit generic: listOf<User>(...)
            for ctor in [
                "listOf",
                "mutableListOf",
                "setOf",
                "mutableSetOf",
                "sequenceOf",
            ] {
                let generic_prefix = format!("{ctor}<");
                if rhs.starts_with(&generic_prefix) {
                    let inner = &rhs[generic_prefix.len()..];
                    if let Some(gt) = inner.find('>') {
                        let arg = inner[..gt].split(',').next().unwrap_or("").trim();
                        if !arg.is_empty() {
                            return self.resolve_simple_name_to_fqn(file_id, arg);
                        }
                    }
                }
            }

            // Collection initializer from first constructor arg: listOf(User(...))
            for ctor in [
                "listOf(",
                "mutableListOf(",
                "setOf(",
                "mutableSetOf(",
                "sequenceOf(",
            ] {
                if rhs.starts_with(ctor) {
                    if let Some(lparen) = rhs.find('(') {
                        let rest = rhs[lparen + 1..].trim_start();
                        if let Some(ctor_lparen) = rest.find('(') {
                            let first_ctor = rest[..ctor_lparen].trim();
                            if !first_ctor.is_empty() {
                                return self.resolve_simple_name_to_fqn(file_id, first_ctor);
                            }
                        }
                    }
                }
            }

            break;
        }

        None
    }

    fn infer_flat_chain_root_receiver_from_source(
        &self,
        source_text: &str,
        offset: u32,
    ) -> Option<String> {
        let off = offset as usize;
        let prefix = source_text.get(..off.min(source_text.len()))?;
        let eq_idx = find_plain_assignment(prefix)?;
        let rhs = prefix.get(eq_idx + 1..)?.trim_start();

        let ident_len = rhs
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .count();
        if ident_len == 0 {
            return None;
        }

        Some(rhs[..ident_len].to_string())
    }

    /// Resolve a simple name (like `Factory`) to its FQN using imports and workspace index.
    fn resolve_simple_name_to_fqn(&self, file_id: FileId, simple_name: &str) -> Option<String> {
        let hyperion_workspace::TypeNameResolution::Exact(fqn) = self
            .workspace_index
            .resolve_type_name(file_id, &[], simple_name)
        else {
            return None;
        };
        Some(fqn)
    }

    /// Resolve one non-local Kotlin spelling without turning import order into
    /// semantic precedence. Explicit imports and current-package declarations
    /// are definitive tiers. Star imports form one ambiguity set, while type
    /// candidates use the workspace's canonical Kotlin type resolver so
    /// implicit imports and aliases stay consistent with indexing.
    fn resolve_workspace_identifier(
        &self,
        file_id: FileId,
        identifier: &str,
        namespaces: &[DeclarationNamespace],
    ) -> Result<Option<Location>, String> {
        let Some(imports) = self.workspace_index.get_imports(file_id) else {
            return Ok(None);
        };

        if let Some(imported_fqn) = imports.explicit.get(identifier) {
            return self.unique_typed_location(imported_fqn, namespaces);
        }

        let same_package_fqn = imports.current_package.as_ref().map_or_else(
            || identifier.to_owned(),
            |package| format!("{package}.{identifier}"),
        );
        let same_package = self.typed_locations(&same_package_fqn, namespaces);
        if !same_package.is_empty() {
            return self.unique_location(same_package);
        }

        let mut candidates = Vec::new();
        for package in &imports.wildcards {
            let candidate_fqn = format!("{package}.{identifier}");
            candidates.extend(self.typed_locations(&candidate_fqn, namespaces));
        }

        if namespaces.contains(&DeclarationNamespace::Type) {
            if let TypeNameResolution::Exact(type_fqn) =
                self.workspace_index
                    .resolve_type_name(file_id, &[], identifier)
            {
                candidates.extend(self.typed_locations(&type_fqn, &[DeclarationNamespace::Type]));
            }
        }

        self.unique_location(candidates)
    }

    fn unique_typed_location(
        &self,
        fqn: &str,
        namespaces: &[DeclarationNamespace],
    ) -> Result<Option<Location>, String> {
        self.unique_location(self.typed_locations(fqn, namespaces))
    }

    fn typed_locations(
        &self,
        fqn: &str,
        namespaces: &[DeclarationNamespace],
    ) -> Vec<(SymbolTarget, hyperion_workspace::SymbolLocation)> {
        namespaces
            .iter()
            .filter_map(|namespace| {
                let target = SymbolTarget::new(fqn, *namespace);
                self.workspace_index
                    .find_symbol_definition(&target)
                    .map(|definition| (target, definition.location))
            })
            .collect()
    }

    fn unique_location(
        &self,
        mut candidates: Vec<(SymbolTarget, hyperion_workspace::SymbolLocation)>,
    ) -> Result<Option<Location>, String> {
        candidates.sort_unstable_by(|(left, _), (right, _)| {
            left.fqn
                .cmp(&right.fqn)
                .then_with(|| (left.namespace as u8).cmp(&(right.namespace as u8)))
        });
        candidates.dedup_by(|(left, _), (right, _)| left == right);
        let [(_, location)] = candidates.as_slice() else {
            return Ok(None);
        };
        self.symbol_location_to_lsp_location(location)
    }

    /// Extract identifier from a Kotlin string template ($identifier pattern)
    ///
    /// Given a STRING_LITERAL token and cursor offset, returns the identifier
    /// if the cursor is positioned within a $identifier pattern.
    ///
    /// Example: "Hello, $name" with cursor at 'n' returns Some("name")
    pub fn extract_string_template_identifier(
        &self,
        token: &hyperion_kotlin::SyntaxToken,
        cursor_offset: u32,
    ) -> Option<String> {
        let text = token.text();
        let token_start = u32::from(token.text_range().start());

        let relative_offset = cursor_offset.checked_sub(token_start)? as usize;
        string_template_identifier_at_offset(text, relative_offset).map(str::to_owned)
    }

    /// Resolve a Kotlin identifier to its definition
    ///
    /// This is a helper method that encapsulates the identifier resolution logic,
    /// used by both regular identifier resolution and string template resolution.
    pub fn resolve_identifier(
        &self,
        file_id: FileId,
        identifier: &str,
        source_text: &str,
        uri: &Url,
        offset: u32,
    ) -> Result<Option<Location>, String> {
        debug!("NAV: Resolving Kotlin identifier: {}", identifier);

        // PHASE 1: Check local scope first (local variables, parameters, etc.)
        let snapshot = self.txn.snapshot();

        let scope_graph = snapshot.scope_graph(file_id);
        debug!("NAV: Checking local scope for '{}'", identifier);
        if let Some(symbol) = scope_graph.resolve_symbol(identifier, offset as usize) {
            info!(
                "NAV: Resolved '{}' via LOCAL SCOPE (kind: {:?})",
                identifier, symbol.kind
            );

            // Convert the ScopeSymbol definition_range to an LSP Location
            let start_pos = offset_to_position(source_text, symbol.definition_range.start().into());
            let end_pos = offset_to_position(source_text, symbol.definition_range.end().into());

            return Ok(Some(Location {
                uri: uri.clone(),
                range: Range {
                    start: start_pos,
                    end: end_pos,
                },
            }));
        }

        debug!("NAV: Not found in local scope, checking workspace index");
        self.resolve_workspace_identifier(file_id, identifier, &[DeclarationNamespace::Value])
    }

    /// Convert a SymbolLocation to an LSP Location
    ///
    /// External source is parsed according to the actual source attachment
    /// language, not the language of the document containing the reference.
    /// This matters for the common Kotlin-to-Java navigation path: dependency
    /// and JDK providers currently materialize Java source.
    fn symbol_location_to_lsp_location(
        &self,
        symbol_loc: &hyperion_workspace::SymbolLocation,
    ) -> Result<Option<Location>, String> {
        crate::navigation_util::symbol_location_to_lsp_location(
            self.workspace_index,
            symbol_loc,
            Some(self.source_providers),
        )
    }
}

/// Return the template identifier under a byte offset relative to a Kotlin
/// string token. Tree ranges and LSP conversion both produce UTF-8 byte
/// offsets, so this scanner deliberately keeps every boundary in bytes.
fn string_template_identifier_at_offset(text: &str, cursor: usize) -> Option<&str> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }

    let raw_string = text.starts_with("\"\"\"");
    let mut offset = 0;
    while offset < text.len() {
        let character = text[offset..].chars().next()?;
        if character != '$' || (!raw_string && dollar_is_escaped(text, offset)) {
            offset += character.len_utf8();
            continue;
        }

        let dollar = offset;
        offset += character.len_utf8();
        let next = text[offset..].chars().next()?;
        if next == '{' {
            let expression_start = offset + next.len_utf8();
            let (expression_end, template_end) = braced_template_end(text, expression_start);
            if (dollar..template_end).contains(&cursor) {
                let mut candidate = expression_start;
                while candidate < expression_end {
                    let ch = text[candidate..].chars().next()?;
                    if is_kotlin_identifier_start(ch) {
                        let start = candidate;
                        candidate += ch.len_utf8();
                        while candidate < expression_end {
                            let ch = text[candidate..].chars().next()?;
                            if !is_kotlin_identifier_continue(ch) {
                                break;
                            }
                            candidate += ch.len_utf8();
                        }
                        if (start..candidate).contains(&cursor) {
                            return Some(&text[start..candidate]);
                        }
                    } else {
                        candidate += ch.len_utf8();
                    }
                }
            }
            offset = template_end;
            continue;
        }

        if is_kotlin_identifier_start(next) {
            let identifier_start = offset;
            offset += next.len_utf8();
            while offset < text.len() {
                let ch = text[offset..].chars().next()?;
                if !is_kotlin_identifier_continue(ch) {
                    break;
                }
                offset += ch.len_utf8();
            }
            if (dollar..offset).contains(&cursor) {
                return Some(&text[identifier_start..offset]);
            }
            continue;
        }
    }

    None
}

fn braced_template_end(text: &str, expression_start: usize) -> (usize, usize) {
    let mut offset = expression_start;
    let mut depth = 1_u32;
    while offset < text.len() {
        let Some(ch) = text[offset..].chars().next() else {
            break;
        };
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return (offset, offset + ch.len_utf8());
                }
            }
            _ => {}
        }
        offset += ch.len_utf8();
    }
    (text.len(), text.len())
}

fn dollar_is_escaped(text: &str, dollar: usize) -> bool {
    let mut backslashes = 0;
    for byte in text.as_bytes()[..dollar].iter().rev() {
        if *byte != b'\\' {
            break;
        }
        backslashes += 1;
    }
    backslashes % 2 == 1
}

fn is_kotlin_identifier_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn is_kotlin_identifier_continue(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Find the position of a plain `=` assignment operator, skipping comparison and
/// compound operators (`==`, `!=`, `<=`, `>=`, `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`).
fn find_plain_assignment(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    for (idx, _) in text.rmatch_indices('=') {
        if idx == 0 {
            return Some(idx);
        }
        let prev = bytes[idx - 1];
        if matches!(
            prev,
            b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'&' | b'|' | b'^'
        ) {
            continue;
        }
        // Skip the first `=` of a `==` token (prev char was fine but next is `=`)
        if bytes.get(idx + 1) == Some(&b'=') {
            continue;
        }
        return Some(idx);
    }
    None
}

#[cfg(test)]
mod string_template_tests {
    use super::{string_template_identifier_at_offset, KotlinNavigationHelper};

    use crate::source_provider::{
        ExternalSourceRequest, SourceContent, SourceFile, SourceProvider, SourceProviderChain,
    };
    use hyperion_workspace::{
        ExternalSourceIdentity, Modifiers, Symbol, TextRange, WorkspaceIndex,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use tower_lsp::lsp_types::{Position, SymbolKind};

    #[derive(Clone)]
    struct KotlinIdentitySourceProvider {
        source_identity: ExternalSourceIdentity,
        fqn: &'static str,
        content: SourceContent,
    }

    impl SourceProvider for KotlinIdentitySourceProvider {
        fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
            (request.source_identity() == &self.source_identity && request.fqn() == self.fqn)
                .then(|| self.content.clone())
        }

        fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
            request.source_identity() == &self.source_identity && request.fqn() == self.fqn
        }

        fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
            source_identity == &self.source_identity
        }

        fn snapshot_for_sources(
            &self,
            source_identities: &std::collections::HashSet<ExternalSourceIdentity>,
        ) -> Option<Arc<dyn SourceProvider>> {
            source_identities
                .contains(&self.source_identity)
                .then(|| Arc::new(self.clone()) as Arc<dyn SourceProvider>)
        }

        fn priority(&self) -> u32 {
            0
        }

        fn name(&self) -> &'static str {
            "kotlin-identity-test"
        }
    }

    fn publish_revisioned_external_symbol(
        index: &WorkspaceIndex,
        providers: &Arc<SourceProviderChain>,
        owner: &str,
        symbol: Symbol,
        provider: KotlinIdentitySourceProvider,
    ) {
        let source_identity = provider.source_identity.clone();
        let ticket = index.begin_external_source_refresh(owner);
        assert!(providers.observe_group_refresh(&ticket));
        let publication = providers
            .begin_group_publication(
                &ticket,
                std::slice::from_ref(&source_identity),
                Some(Arc::new(provider)),
            )
            .expect("provider publication");
        let source = PathBuf::from(crate::source_provider::jdk::JDK_SOURCE_ID);
        let refresh = index
            .replace_external_sources_for_refresh(&ticket, std::slice::from_ref(&source))
            .expect("workspace publication");
        let revision = refresh
            .revision_for_identity(&source_identity)
            .expect("external revision");
        assert!(index.add_external_symbols_responsive_for_revision(vec![symbol], revision));
        assert!(providers.record_group_publication_success_for_revision(
            &publication,
            &source_identity,
            revision,
        ));
        assert!(providers.finish_group_publication(&publication));
    }

    #[test]
    fn retained_external_kotlin_location_is_rejected_after_workspace_promotion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let symbol = Symbol {
            fqn: "dep.Duplicate".into(),
            simple_name: "Duplicate".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        };
        let source_path = temp.path().join("external/Duplicate.kt");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let source = "package dep\nclass Duplicate";
        std::fs::write(&source_path, source).unwrap();
        let providers = Arc::new(SourceProviderChain::new());
        publish_revisioned_external_symbol(
            &index,
            &providers,
            "kotlin-workspace-promotion",
            symbol,
            KotlinIdentitySourceProvider {
                source_identity: ExternalSourceIdentity::for_virtual(
                    crate::source_provider::jdk::JDK_SOURCE_ID,
                ),
                fqn: "dep.Duplicate",
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(source))),
                    temp_file: source_path,
                },
            },
        );
        let retained = index
            .find_symbol("dep.Duplicate")
            .expect("R1 external definition");
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let helper = KotlinNavigationHelper::new(&txn, &index, &providers);
        assert!(helper
            .symbol_location_to_lsp_location(&retained)
            .expect("R1 conversion")
            .is_some());

        index
            .index_file_with_source(&temp.path().join("src/dep/Duplicate.kt"), source)
            .expect("workspace definition");

        assert!(
            helper
                .symbol_location_to_lsp_location(&retained)
                .expect("stale conversion")
                .is_none(),
            "Kotlin navigation must enforce the same typed election as Java"
        );
    }

    #[test]
    fn external_java_method_from_kotlin_uses_java_source_symbols() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let symbol = Symbol {
            fqn: "dep.Api.ping()".into(),
            simple_name: "ping".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::METHOD,
            modifiers: Modifiers::default(),
            container: Some("dep.Api".into()),
            return_type: Some("void".into()),
            parameters: Vec::new(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        };
        let source_path = temp.path().join("external/Api.java");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let source = "package dep; class Api { void ping() {} }";
        std::fs::write(&source_path, source).unwrap();
        let providers = Arc::new(SourceProviderChain::new());
        publish_revisioned_external_symbol(
            &index,
            &providers,
            "kotlin-java-method",
            symbol,
            KotlinIdentitySourceProvider {
                source_identity: ExternalSourceIdentity::for_virtual(
                    crate::source_provider::jdk::JDK_SOURCE_ID,
                ),
                fqn: "dep.Api.ping()",
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(source))),
                    temp_file: source_path,
                },
            },
        );
        let method = index
            .find_symbol("dep.Api.ping()")
            .expect("external method");
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let helper = KotlinNavigationHelper::new(&txn, &index, &providers);

        let location = helper
            .symbol_location_to_lsp_location(&method)
            .expect("conversion")
            .expect("Java method source location");
        assert_eq!(location.range.start, Position::new(0, 30));
    }

    #[test]
    fn finds_simple_and_braced_template_identifiers_by_byte_offset() {
        let simple = "\"Hello, $name\"";
        let name = simple.find("name").unwrap();
        assert_eq!(
            string_template_identifier_at_offset(simple, name + 2),
            Some("name")
        );
        assert_eq!(
            string_template_identifier_at_offset(simple, name - 1),
            Some("name")
        );

        let braced = "\"${person.displayName}\"";
        let display_name = braced.find("displayName").unwrap();
        assert_eq!(
            string_template_identifier_at_offset(braced, display_name + 4),
            Some("displayName")
        );
    }

    #[test]
    fn template_offsets_remain_correct_after_unicode() {
        let text = "\"Grüße, $naïve\"";
        let identifier = text.find("naïve").unwrap();
        let cursor = identifier + "naï".len();

        assert_eq!(
            string_template_identifier_at_offset(text, cursor),
            Some("naïve")
        );
    }

    #[test]
    fn regular_strings_ignore_escaped_dollars_but_raw_strings_do_not() {
        let escaped = "\"literal \\$name\"";
        let name = escaped.find("name").unwrap();
        assert_eq!(string_template_identifier_at_offset(escaped, name), None);

        let raw = "\"\"\"literal \\$name\"\"\"";
        let name = raw.find("name").unwrap();
        assert_eq!(
            string_template_identifier_at_offset(raw, name),
            Some("name")
        );
    }

    #[test]
    fn rejects_non_boundary_and_out_of_range_offsets() {
        let text = "\"é $name\"";
        assert_eq!(string_template_identifier_at_offset(text, 2), None);
        assert_eq!(
            string_template_identifier_at_offset(text, text.len() + 1),
            None
        );
    }
}
