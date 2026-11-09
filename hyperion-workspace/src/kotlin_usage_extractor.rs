// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Usage extractor for Kotlin source: finds symbol references in Kotlin files
//! and feeds them into the same `symbol_usages`/`file_usage_counts` tables that
//! the Java usage extractor (`crate::usage_extractor`) populates.
//!
//! Kotlin's syntax tree shape differs from Java's in a few ways that matter
//! here:
//! - There is no `new` keyword: constructor calls are syntactically
//!   indistinguishable from function calls (`User("Alice")` vs `foo()`).
//! - Top-level functions/properties have no containing class, so their FQN
//!   is `package.name` (see `hyperion_kotlin::SymbolExtractor`).
//! - Kotlin function/property FQNs never carry a parameter-count/type
//!   descriptor suffix (unlike the Java extractor's `(_,_)` disambiguation),
//!   so usage FQNs built here must match that plain `container.name` shape.
//! - Member access (`.`) and safe access (`?.`) are both represented as
//!   binary-precedence nodes (`MEMBER_ACCESS_EXPR` / `SAFE_ACCESS_EXPR`)
//!   wrapping `receiver DOT|SAFE_CALL member`, and calls are a separate
//!   `CALL_EXPR` wrapping `callee ( args )`.
//!
//! Accuracy policy: every usage emitted here is verified against the
//! workspace index (`WorkspaceIndex::get_symbol`) before being recorded, so a
//! reference is only reported when it plausibly resolves to an indexed
//! symbol shape. This intentionally under-reports rather than emitting
//! guesses (e.g. references to un-indexed stdlib symbols, or to generic type
//! parameters, are silently skipped).
//!
//! Known, deliberate gaps (documented rather than guessed at):
//! - Named infix function calls (`a shl b`, `x zip y`) are not extracted:
//!   the Kotlin parser does not currently build a coherent AST node for
//!   identifier-based infix calls (only `.`/`?.` are wired into the Pratt
//!   parser's infix table), so there is no reliable tree shape to walk.
//!   Member-style infix calls and safe-call chains (`a?.b()?.c()`) ARE
//!   covered, since those go through `MEMBER_ACCESS_EXPR`/`SAFE_ACCESS_EXPR`.
//! - Bare (unqualified) implicit-`this` property usages are recovered through
//!   Kotlin's lexical scope graph and then joined to an exact, currently
//!   elected declaration range. An unresolved or shadow-source binding is
//!   skipped rather than guessed.

use crate::{
    import_resolver::{ImportResolver, RevisionSymbolOverlay},
    usage_extractor::Usage,
    usage_resolution::{is_callable_declaration, resolve_field, resolve_method},
    FileId, ImportMap, Symbol, SymbolLocation, TextRange, WorkspaceIndex,
};
use anyhow::Result;
use hyperion_kotlin::{Lexer, Parser, SyntaxKind, SyntaxNode, SyntaxToken};
use lsp_types::SymbolKind;
use rowan::NodeOrToken;
use std::collections::HashMap;
use std::sync::Arc;

type Elem = NodeOrToken<SyntaxNode, SyntaxToken>;

/// Context threaded through the recursive walk: tracks the enclosing
/// class/object/interface FQN and known variable/property types so member
/// access and calls can be resolved to FQNs.
#[derive(Clone, Default)]
struct KtContext {
    /// FQN of the nearest enclosing class/object/interface, if any.
    enclosing_container_fqn: Option<String>,
    /// Current file's package, if any.
    package: Option<String>,
    /// Known variable/parameter/property names -> resolved type FQN.
    /// Only populated when the declared type itself resolves to an indexed
    /// symbol (used for direct/companion member lookups).
    variable_types: HashMap<String, String>,
    /// Known variable/parameter/property names -> declared type name, exactly
    /// as written (e.g. "String", "Animal"), regardless of whether that type
    /// is itself indexed. Used only to match extension-function receiver
    /// types (which are commonly un-indexed stdlib types like `String`).
    variable_raw_types: HashMap<String, String>,
}

pub struct KotlinUsageExtractor;

impl KotlinUsageExtractor {
    /// Extract all symbol usages from Kotlin source code.
    pub fn extract(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
    ) -> Result<Vec<Usage>> {
        let resolver =
            ImportResolver::for_language(imports, workspace_index, crate::Language::Kotlin);
        Ok(Self::extract_with_resolver(
            source, file_id, imports, &resolver,
        ))
    }

    pub(crate) fn extract_for_revision(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
        revision: &RevisionSymbolOverlay,
    ) -> Vec<Usage> {
        let resolver = ImportResolver::for_revision(
            imports,
            workspace_index,
            revision,
            crate::Language::Kotlin,
        );
        Self::extract_with_resolver(source, file_id, imports, &resolver)
    }

    fn extract_with_resolver(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        resolver: &ImportResolver<'_>,
    ) -> Vec<Usage> {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let mut usages = Vec::new();

        let mut ctx = KtContext {
            package: imports.current_package.clone(),
            ..Default::default()
        };
        collect_scope_locals(&root, resolver, &mut ctx);

        walk(&root, file_id, resolver, imports, &mut usages, &ctx);
        extract_bare_value_usages(&root, file_id, resolver, &mut usages);
        extract_template_usages(&root, file_id, resolver, &mut usages);

        usages
    }
}

/// Recover bare property/field/enum-value reads which Kotlin represents as
/// identifier tokens rather than a dedicated name-expression node. The scope
/// graph supplies the lexical declaration range; the resolver then requires
/// that range to identify one currently elected value declaration in this
/// exact file. Locals, parameters, declarations, qualified accesses, and
/// duplicate-FQN shadow sources therefore cannot be rebound by spelling.
fn extract_bare_value_usages(
    root: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    usages: &mut Vec<Usage>,
) {
    let scope_graph = hyperion_kotlin::build_scope_graph(root);
    for token in root
        .descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
    {
        if !matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT)
            || kotlin_token_is_qualified(&token)
        {
            continue;
        }
        let name = token.text().trim_matches('`');
        let Some(binding) =
            scope_graph.resolve_symbol(name, usize::from(token.text_range().start()))
        else {
            continue;
        };
        if binding.definition_range == token.text_range() {
            continue;
        }
        let Some(symbol) = resolver.value_definition_at(
            file_id,
            name,
            u32::from(binding.definition_range.start()),
            u32::from(binding.definition_range.end()),
        ) else {
            continue;
        };
        let fqn = symbol.fqn.to_string();
        let range = token.text_range();
        if usages.iter().any(|usage| {
            usage.fqn == fqn
                && usage.location.range.start == u32::from(range.start())
                && usage.location.range.end == u32::from(range.end())
        }) {
            continue;
        }
        emit(usages, file_id, range, symbol.kind, name.to_string(), fqn);
    }
}

fn kotlin_token_is_qualified(token: &SyntaxToken) -> bool {
    let mut previous = token.prev_token();
    while previous.as_ref().is_some_and(|token| {
        matches!(
            token.kind(),
            SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        )
    }) {
        previous = previous.and_then(|token| token.prev_token());
    }
    previous.is_some_and(|token| matches!(token.kind(), SyntaxKind::DOT | SyntaxKind::SAFE_CALL))
}

/// Extract simple `$name` and `${name}` references from Kotlin strings.
///
/// String templates are single lexer tokens, so the normal CST node walk
/// cannot see their identifiers. Scope resolution prevents a local binding
/// from being misreported as a same-named indexed top-level property.
fn extract_template_usages(
    root: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    usages: &mut Vec<Usage>,
) {
    let scope_graph = hyperion_kotlin::build_scope_graph(root);

    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        for (name, range) in template_identifiers(&token) {
            let Some(symbol) = resolver
                .resolve_unqualified_symbol(&name, crate::DeclarationNamespace::Value)
                .filter(|symbol| symbol.kind == SymbolKind::PROPERTY)
            else {
                continue;
            };
            let fqn = symbol.fqn.to_string();

            if let Some(local) = scope_graph.resolve_symbol(&name, usize::from(range.start())) {
                let indexed_definition = resolver.definition_matches(
                    &fqn,
                    file_id,
                    u32::from(local.definition_range.start()),
                    u32::from(local.definition_range.end()),
                );
                if !indexed_definition {
                    continue;
                }
            }

            emit(usages, file_id, range, symbol.kind, name, fqn);
        }
    }
}

fn template_identifiers(token: &SyntaxToken) -> Vec<(String, rowan::TextRange)> {
    if !matches!(
        token.kind(),
        SyntaxKind::STRING_LITERAL | SyntaxKind::RAW_STRING_LITERAL
    ) {
        return Vec::new();
    }

    let text = token.text().as_bytes();
    let token_start = u32::from(token.text_range().start());
    let mut identifiers = Vec::new();
    let mut index = 0;

    while index < text.len() {
        if text[index] != b'$' {
            index += 1;
            continue;
        }
        if token.kind() == SyntaxKind::STRING_LITERAL
            && index > 0
            && text[..index]
                .iter()
                .rev()
                .take_while(|byte| **byte == b'\\')
                .count()
                % 2
                == 1
        {
            index += 1;
            continue;
        }

        let braced = text.get(index + 1) == Some(&b'{');
        let mut start = index + if braced { 2 } else { 1 };
        while braced && text.get(start).is_some_and(u8::is_ascii_whitespace) {
            start += 1;
        }
        if !text
            .get(start)
            .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
        {
            index += 1;
            continue;
        }

        let mut end = start + 1;
        while text
            .get(end)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            end += 1;
        }
        identifiers.push((
            String::from_utf8_lossy(&text[start..end]).into_owned(),
            rowan::TextRange::new(
                rowan::TextSize::from(token_start + start as u32),
                rowan::TextSize::from(token_start + end as u32),
            ),
        ));
        index = end;
    }

    identifiers
}

fn emit(
    usages: &mut Vec<Usage>,
    file_id: FileId,
    range: rowan::TextRange,
    kind: SymbolKind,
    name: String,
    fqn: String,
) {
    let location = SymbolLocation {
        file_id,
        source_revision_id: None,
        range: TextRange::new(range.start().into(), range.end().into()),
        kind,
        name: name.into(),
        fqn: fqn.clone().into(),
        container_name: None,
        documentation: None,
    };
    usages.push(Usage { fqn, location });
}

/// Look up a symbol by FQN, verifying it plausibly exists in the index
/// before we ever emit a usage referencing it.
fn lookup(resolver: &ImportResolver, fqn: &str) -> Option<Arc<Symbol>> {
    resolver.get_symbol(fqn)
}

// ============================================================================
// Scope-local variable/parameter/property type collection
// ============================================================================

/// Collect declared (explicitly typed) variable/parameter/property types
/// visible in `scope_node`, without descending into nested
/// function/class/object/interface bodies (those get their own scope when
/// the walker reaches them).
fn collect_scope_locals(scope_node: &SyntaxNode, resolver: &ImportResolver, ctx: &mut KtContext) {
    for child in scope_node.children() {
        match child.kind() {
            SyntaxKind::PARAMETER => {
                if let Some((name, raw_ty)) = extract_param_name_and_raw_type(&child) {
                    if let Some(fqn) = resolve_declared_type_to_fqn(&raw_ty, resolver) {
                        ctx.variable_types.insert(name.clone(), fqn);
                    }
                    ctx.variable_raw_types.insert(name, raw_ty);
                }
            }
            SyntaxKind::PROPERTY_DECL => {
                if let Some((name, raw_ty)) = extract_property_name_and_raw_type(&child) {
                    if let Some(fqn) = resolve_declared_type_to_fqn(&raw_ty, resolver) {
                        ctx.variable_types.insert(name.clone(), fqn);
                    }
                    ctx.variable_raw_types.insert(name, raw_ty);
                }
            }
            SyntaxKind::FUNCTION_DECL
            | SyntaxKind::CLASS_DECL
            | SyntaxKind::OBJECT_DECL
            | SyntaxKind::INTERFACE_DECL => {
                // Boundary: these get their own scope when the walker reaches them.
            }
            _ => {
                collect_scope_locals(&child, resolver, ctx);
            }
        }
    }
}

/// Extract `(name, declared_type_name)` from a PARAMETER node, if it has an
/// explicit type. The type name is returned exactly as written (no
/// existence check) so extension-function receiver matching can use it even
/// when the type itself isn't an indexed workspace symbol (e.g. `String`).
fn extract_param_name_and_raw_type(node: &SyntaxNode) -> Option<(String, String)> {
    let mut name: Option<String> = None;
    let mut seen_colon = false;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t)
                if matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT)
                    && !seen_colon
                    && name.is_none() =>
            {
                name = Some(t.text().to_string());
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::COLON => {
                seen_colon = true;
            }
            NodeOrToken::Node(n) if seen_colon && n.kind() == SyntaxKind::TYPE_REF => {
                let (parts, _, _) = type_ref_name_parts(&n);
                if parts.is_empty() {
                    return None;
                }
                return Some((name?, parts.join(".")));
            }
            _ => {}
        }
    }
    None
}

/// Extract `(name, declared_type_name)` from a PROPERTY_DECL node, if it has
/// an explicit type. See `extract_param_name_and_raw_type` for why this
/// returns the unverified, as-written type name.
fn extract_property_name_and_raw_type(node: &SyntaxNode) -> Option<(String, String)> {
    let mut seen_val_var = false;
    let mut name: Option<String> = None;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t)
                if matches!(t.kind(), SyntaxKind::VAL_KW | SyntaxKind::VAR_KW) =>
            {
                seen_val_var = true;
            }
            NodeOrToken::Token(t)
                if seen_val_var
                    && matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT)
                    && name.is_none() =>
            {
                name = Some(t.text().to_string());
            }
            NodeOrToken::Node(n) if name.is_some() && n.kind() == SyntaxKind::TYPE_REF => {
                let (parts, _, _) = type_ref_name_parts(&n);
                if parts.is_empty() {
                    return None;
                }
                return Some((name?, parts.join(".")));
            }
            _ => {}
        }
    }
    None
}

// ============================================================================
// Type name parsing/resolution
// ============================================================================

/// Collect the dotted simple-name parts of a TYPE_REF node (e.g. `["List"]`
/// for `List<User>`, or `["Outer", "Inner"]` for `Outer.Inner`), stopping at
/// generics (`TYPE_ARGUMENT_LIST`) or nullability/non-null markers.
fn type_ref_name_parts(
    node: &SyntaxNode,
) -> (
    Vec<String>,
    Option<rowan::TextRange>,
    Option<rowan::TextRange>,
) {
    let mut parts = Vec::new();
    let mut first_range = None;
    let mut last_range = None;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(t)
                if matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT) =>
            {
                if first_range.is_none() {
                    first_range = Some(t.text_range());
                }
                last_range = Some(t.text_range());
                parts.push(t.text().to_string());
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::DOT => {}
            _ => break,
        }
    }

    (parts, first_range, last_range)
}

/// Resolve a declared type name (already stripped of surrounding syntax, but
/// possibly still carrying `?`/generics if it came from a raw symbol
/// `return_type` string) to an indexed FQN. Only returns `Some` when the
/// resolved FQN actually exists in the workspace index.
fn resolve_declared_type_to_fqn(raw: &str, resolver: &ImportResolver) -> Option<String> {
    let base = raw
        .split('<')
        .next()
        .unwrap_or(raw)
        .trim()
        .trim_end_matches('?')
        .trim_end_matches('!')
        .trim();

    if base.is_empty() {
        return None;
    }

    let crate::TypeNameResolution::Exact(fqn) = resolver.resolve_type_identity(base) else {
        return None;
    };
    resolver.get_symbol(&fqn).map(|_| fqn)
}

/// Emit a type-reference usage for a TYPE_REF node (declarations, supertypes,
/// parameter types, generics all flow through this since they're all plain
/// TYPE_REF nodes in the Kotlin tree).
fn extract_type_ref_usage(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    usages: &mut Vec<Usage>,
) {
    let (parts, first_range, last_range) = type_ref_name_parts(node);
    if parts.is_empty() {
        return;
    }

    let qualified = parts.join(".");
    let crate::TypeNameResolution::Exact(fqn) = resolver.resolve_type_identity(&qualified) else {
        return;
    };
    let Some(symbol) = lookup(resolver, &fqn) else {
        return;
    };
    let range = if parts.len() > 1 {
        last_range
    } else {
        first_range
    }
    .unwrap_or(node.text_range());
    let name = parts.last().cloned().unwrap_or_default();
    emit(usages, file_id, range, symbol.kind, name, fqn);
}

// ============================================================================
// Container (class/object/interface) FQN naming
// ============================================================================

/// Mirrors `hyperion_kotlin::SymbolExtractor`'s naming conventions so usages
/// join up with indexed symbols: nested class/object FQN is always
/// `{outerFqn}.{simpleName}`, and an unnamed `companion object` is named
/// `Companion`.
fn build_container_context(node: &SyntaxNode, ctx: &KtContext) -> KtContext {
    let is_companion = node
        .parent()
        .into_iter()
        .flat_map(|p| p.children())
        .take_while(|sibling| sibling != node)
        .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIERS)
        .any(|modifiers| {
            modifiers
                .descendants_with_tokens()
                .any(|e| matches!(e, NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMPANION_KW))
        });

    let kw = match node.kind() {
        SyntaxKind::CLASS_DECL => SyntaxKind::CLASS_KW,
        SyntaxKind::OBJECT_DECL => SyntaxKind::OBJECT_KW,
        SyntaxKind::INTERFACE_DECL => SyntaxKind::INTERFACE_KW,
        _ => SyntaxKind::CLASS_KW,
    };

    let mut seen_kw = false;
    let mut simple_name = None;
    for elem in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = elem {
            if t.kind() == kw {
                seen_kw = true;
            } else if seen_kw && t.kind() == SyntaxKind::IDENT {
                simple_name = Some(t.text().to_string());
                break;
            }
        }
    }

    let simple_name = simple_name.unwrap_or_else(|| {
        if is_companion {
            "Companion".to_string()
        } else {
            String::new()
        }
    });

    let fqn = if simple_name.is_empty() {
        // Anonymous object literal or otherwise unnamed - keep the outer
        // container so nested members at least resolve against it, rather
        // than producing a malformed FQN.
        ctx.enclosing_container_fqn
            .clone()
            .or_else(|| ctx.package.clone())
    } else if let Some(outer) = &ctx.enclosing_container_fqn {
        Some(format!("{outer}.{simple_name}"))
    } else if let Some(pkg) = &ctx.package {
        Some(format!("{pkg}.{simple_name}"))
    } else {
        Some(simple_name)
    };

    let mut child_ctx = ctx.clone();
    child_ctx.enclosing_container_fqn = fqn;
    child_ctx
}

// ============================================================================
// Call/member resolution
// ============================================================================

/// Resolve a bare (unqualified) call target: either a constructor call
/// (Capitalized name resolving to a class), a member function of the
/// enclosing container, or a top-level function in the current package.
fn resolve_bare_call(
    name: &str,
    ctx: &KtContext,
    resolver: &ImportResolver,
) -> Option<(String, SymbolKind)> {
    let starts_upper = name.chars().next().is_some_and(|c| c.is_uppercase());

    if starts_upper {
        let crate::TypeNameResolution::Exact(fqn) = resolver.resolve_type_identity(name) else {
            return None;
        };
        let symbol = lookup(resolver, &fqn)?;
        return matches!(symbol.kind, SymbolKind::CLASS | SymbolKind::STRUCT)
            .then_some((fqn, symbol.kind));
    }

    if let Some(container) = &ctx.enclosing_container_fqn {
        if let Some(symbol) = resolve_method(container, name, resolver) {
            return Some((symbol.fqn.to_string(), symbol.kind));
        }
    }

    resolver
        .resolve_unqualified_symbol(name, crate::DeclarationNamespace::Callable)
        .filter(|symbol| is_callable_declaration(symbol.kind))
        .map(|symbol| (symbol.fqn.to_string(), symbol.kind))
}

/// Resolve `{base_type}.{member}`, falling back to `{base_type}.Companion.{member}`
/// so members invoked through the enclosing class name (`MyClass.member()`)
/// still join up with the companion object's indexed FQN.
fn resolve_callable_member(
    base_type_fqn: &str,
    member_name: &str,
    resolver: &ImportResolver,
) -> Option<(String, SymbolKind)> {
    let direct = format!("{base_type_fqn}.{member_name}");
    if let Some(sym) = lookup(resolver, &direct).filter(|sym| is_callable_declaration(sym.kind)) {
        return Some((direct, sym.kind));
    }

    let via_companion = format!("{base_type_fqn}.Companion.{member_name}");
    if let Some(sym) =
        lookup(resolver, &via_companion).filter(|sym| is_callable_declaration(sym.kind))
    {
        return Some((via_companion, sym.kind));
    }

    resolve_method(base_type_fqn, member_name, resolver)
        .map(|symbol| (symbol.fqn.to_string(), symbol.kind))
}

fn resolve_property_member(
    base_type_fqn: &str,
    member_name: &str,
    resolver: &ImportResolver,
) -> Option<(String, SymbolKind)> {
    let direct = format!("{base_type_fqn}.{member_name}");
    if let Some(sym) = lookup(resolver, &direct).filter(|sym| !is_callable_declaration(sym.kind)) {
        return Some((direct, sym.kind));
    }

    let via_companion = format!("{base_type_fqn}.Companion.{member_name}");
    if let Some(sym) =
        lookup(resolver, &via_companion).filter(|sym| !is_callable_declaration(sym.kind))
    {
        return Some((via_companion, sym.kind));
    }

    resolve_field(base_type_fqn, member_name, resolver)
        .map(|symbol| (symbol.fqn.to_string(), symbol.kind))
}

/// Resolve an extension-function call: the receiver's static type doesn't
/// declare `member_name`, so check whether an in-scope extension function
/// with a matching receiver type exists (explicit import, wildcard import,
/// or same-package declaration).
fn resolve_extension_fqn(
    member_name: &str,
    base_type_fqn: &str,
    resolver: &ImportResolver,
) -> Option<String> {
    let base_simple = base_type_fqn.rsplit('.').next().unwrap_or(base_type_fqn);

    let symbol = resolver
        .resolve_unqualified_symbol(member_name, crate::DeclarationNamespace::Callable)
        .filter(|symbol| symbol.kind == SymbolKind::FUNCTION)?;
    let receiver = symbol.receiver_type.as_ref()?;
    let recv_simple = receiver
        .split(['<', '?'])
        .next()
        .unwrap_or(receiver)
        .trim()
        .rsplit('.')
        .next()
        .unwrap_or(receiver);
    (recv_simple == base_simple).then(|| symbol.fqn.to_string())
}

/// Split a CALL_EXPR's direct children into `(callee, argument/trailing nodes)`.
/// Skips a leading `TYPE_ARGUMENT_LIST` (explicit generic call args, e.g.
/// `foo<String>()`) when locating the callee.
fn split_call_children(node: &SyntaxNode) -> (Option<Elem>, Vec<SyntaxNode>) {
    let mut callee: Option<Elem> = None;
    let mut rest = Vec::new();
    let mut past_callee = false;

    for elem in node.children_with_tokens() {
        if !past_callee {
            match &elem {
                NodeOrToken::Node(n) if n.kind() == SyntaxKind::TYPE_ARGUMENT_LIST => continue,
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::L_PAREN => {
                    past_callee = true;
                }
                NodeOrToken::Node(n) if n.kind() == SyntaxKind::LAMBDA_EXPR => {
                    // Trailing-lambda-only call, e.g. `run { ... }` (no parens at all).
                    past_callee = true;
                    rest.push(n.clone());
                }
                _ => {
                    if callee.is_none() {
                        callee = Some(elem.clone());
                    }
                }
            }
        } else if let NodeOrToken::Node(n) = &elem {
            rest.push(n.clone());
        }
    }

    (callee, rest)
}

/// Split a MEMBER_ACCESS_EXPR/SAFE_ACCESS_EXPR's children into
/// `(receiver, member_ident)`.
fn split_member_access(node: &SyntaxNode) -> Option<(Elem, SyntaxToken)> {
    let mut receiver: Option<Elem> = None;
    let mut seen_dot = false;
    let mut member: Option<SyntaxToken> = None;

    for elem in node.children_with_tokens() {
        match &elem {
            NodeOrToken::Token(t)
                if matches!(t.kind(), SyntaxKind::DOT | SyntaxKind::SAFE_CALL) =>
            {
                seen_dot = true;
            }
            NodeOrToken::Token(t)
                if seen_dot
                    && matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT) =>
            {
                member = Some(t.clone());
            }
            _ => {
                if !seen_dot && receiver.is_none() {
                    receiver = Some(elem.clone());
                }
            }
        }
    }

    Some((receiver?, member?))
}

/// Resolve the static type FQN of a receiver element (bare `this`, a bare
/// identifier, or a nested expression).
fn resolve_receiver_type(
    elem: &Elem,
    ctx: &KtContext,
    resolver: &ImportResolver,
) -> Option<String> {
    match elem {
        NodeOrToken::Token(t) if t.kind() == SyntaxKind::THIS_KW => {
            ctx.enclosing_container_fqn.clone()
        }
        NodeOrToken::Token(t)
            if matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT) =>
        {
            let name = t.text();
            if let Some(ty) = ctx.variable_types.get(name) {
                return Some(ty.clone());
            }
            if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                if let crate::TypeNameResolution::Exact(fqn) = resolver.resolve_type_identity(name)
                {
                    return Some(fqn);
                }
            }
            None
        }
        NodeOrToken::Node(n) => resolve_node_expr_type(n, ctx, resolver),
        _ => None,
    }
}

/// Resolve the declared (as-written) type name of a bare-identifier receiver,
/// regardless of whether that type is itself indexed. Used only as a
/// fallback for extension-function receiver matching (see module docs).
fn resolve_receiver_raw_type(elem: &Elem, ctx: &KtContext) -> Option<String> {
    match elem {
        NodeOrToken::Token(t)
            if matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::BACKTICK_IDENT) =>
        {
            ctx.variable_raw_types.get(t.text()).cloned()
        }
        _ => None,
    }
}

/// Resolve the static type FQN of a sub-expression node, used to walk
/// chained calls/accesses (`a.b().c`) one hop at a time via declared return
/// types.
fn resolve_node_expr_type(
    node: &SyntaxNode,
    ctx: &KtContext,
    resolver: &ImportResolver,
) -> Option<String> {
    match node.kind() {
        SyntaxKind::PAREN_EXPR => {
            let inner = node.children().next()?;
            resolve_node_expr_type(&inner, ctx, resolver)
        }
        SyntaxKind::CALL_EXPR => {
            let (callee, _rest) = split_call_children(node);
            match callee? {
                Elem::Token(t) if t.kind() == SyntaxKind::IDENT => {
                    let (fqn, kind) = resolve_bare_call(t.text(), ctx, resolver)?;
                    if kind == SymbolKind::CLASS {
                        Some(fqn)
                    } else {
                        return_type_fqn(&fqn, resolver)
                    }
                }
                Elem::Node(member_node)
                    if matches!(
                        member_node.kind(),
                        SyntaxKind::MEMBER_ACCESS_EXPR | SyntaxKind::SAFE_ACCESS_EXPR
                    ) =>
                {
                    let (receiver, member) = split_member_access(&member_node)?;
                    let base = resolve_receiver_type(&receiver, ctx, resolver)?;
                    let (fqn, _kind) = resolve_callable_member(&base, member.text(), resolver)?;
                    return_type_fqn(&fqn, resolver)
                }
                _ => None,
            }
        }
        SyntaxKind::MEMBER_ACCESS_EXPR | SyntaxKind::SAFE_ACCESS_EXPR => {
            let (receiver, member) = split_member_access(node)?;
            let base = resolve_receiver_type(&receiver, ctx, resolver)?;
            let (fqn, _kind) = resolve_property_member(&base, member.text(), resolver)?;
            return_type_fqn(&fqn, resolver)
        }
        _ => None,
    }
}

fn return_type_fqn(member_fqn: &str, resolver: &ImportResolver) -> Option<String> {
    let sym = lookup(resolver, member_fqn)?;
    let raw = sym.return_type.as_deref()?;
    resolve_declared_type_to_fqn(raw, resolver)
}

// ============================================================================
// Main recursive walk
// ============================================================================

fn walk(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    imports: &ImportMap,
    usages: &mut Vec<Usage>,
    ctx: &KtContext,
) {
    match node.kind() {
        SyntaxKind::TYPE_REF => {
            extract_type_ref_usage(node, file_id, resolver, usages);
            for child in node.children() {
                walk(&child, file_id, resolver, imports, usages, ctx);
            }
            return;
        }

        SyntaxKind::CLASS_DECL | SyntaxKind::OBJECT_DECL | SyntaxKind::INTERFACE_DECL => {
            let mut child_ctx = build_container_context(node, ctx);
            collect_scope_locals(node, resolver, &mut child_ctx);
            for child in node.children() {
                walk(&child, file_id, resolver, imports, usages, &child_ctx);
            }
            return;
        }

        SyntaxKind::FUNCTION_DECL => {
            let mut child_ctx = ctx.clone();
            collect_scope_locals(node, resolver, &mut child_ctx);
            for child in node.children() {
                walk(&child, file_id, resolver, imports, usages, &child_ctx);
            }
            return;
        }

        SyntaxKind::CALL_EXPR => {
            extract_call_expr(node, file_id, resolver, imports, usages, ctx);
            return;
        }

        SyntaxKind::MEMBER_ACCESS_EXPR | SyntaxKind::SAFE_ACCESS_EXPR => {
            extract_member_access_property(node, file_id, resolver, imports, usages, ctx);
            return;
        }

        _ => {}
    }

    for child in node.children() {
        walk(&child, file_id, resolver, imports, usages, ctx);
    }
}

/// Handle a CALL_EXPR: resolve the callee (bare call or qualified
/// member/extension call) and emit a usage, then manually recurse into the
/// receiver sub-expression and argument/trailing-lambda nodes.
fn extract_call_expr(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    imports: &ImportMap,
    usages: &mut Vec<Usage>,
    ctx: &KtContext,
) {
    let (callee, rest) = split_call_children(node);

    match callee {
        Some(Elem::Token(ident)) if ident.kind() == SyntaxKind::IDENT => {
            let name = ident.text().to_string();
            if let Some((fqn, kind)) = resolve_bare_call(&name, ctx, resolver) {
                emit(usages, file_id, ident.text_range(), kind, name, fqn);
            }
        }
        Some(Elem::Node(member_node))
            if matches!(
                member_node.kind(),
                SyntaxKind::MEMBER_ACCESS_EXPR | SyntaxKind::SAFE_ACCESS_EXPR
            ) =>
        {
            if let Some((receiver, member)) = split_member_access(&member_node) {
                let member_name = member.text().to_string();
                let mut resolved =
                    resolve_receiver_type(&receiver, ctx, resolver).and_then(|base_fqn| {
                        resolve_callable_member(&base_fqn, &member_name, resolver).or_else(|| {
                            resolve_extension_fqn(&member_name, &base_fqn, resolver)
                                .map(|fqn| (fqn, SymbolKind::FUNCTION))
                        })
                    });
                if resolved.is_none() {
                    resolved = resolve_receiver_raw_type(&receiver, ctx).and_then(|raw_ty| {
                        resolve_extension_fqn(&member_name, &raw_ty, resolver)
                            .map(|fqn| (fqn, SymbolKind::FUNCTION))
                    });
                }
                if let Some((fqn, kind)) = resolved {
                    emit(usages, file_id, member.text_range(), kind, member_name, fqn);
                }
                if let NodeOrToken::Node(n) = &receiver {
                    walk(n, file_id, resolver, imports, usages, ctx);
                }
            }
        }
        Some(Elem::Node(other)) => {
            walk(&other, file_id, resolver, imports, usages, ctx);
        }
        _ => {}
    }

    for arg in rest {
        walk(&arg, file_id, resolver, imports, usages, ctx);
    }
}

/// Handle a MEMBER_ACCESS_EXPR/SAFE_ACCESS_EXPR encountered outside of a
/// call position: a property/field read (`user.name`, `obj?.name`).
fn extract_member_access_property(
    node: &SyntaxNode,
    file_id: FileId,
    resolver: &ImportResolver,
    imports: &ImportMap,
    usages: &mut Vec<Usage>,
    ctx: &KtContext,
) {
    if let Some((receiver, member)) = split_member_access(node) {
        let member_name = member.text().to_string();
        let mut resolved = resolve_receiver_type(&receiver, ctx, resolver)
            .and_then(|base_fqn| resolve_property_member(&base_fqn, &member_name, resolver));
        if resolved.is_none() {
            resolved = resolve_receiver_raw_type(&receiver, ctx).and_then(|raw_ty| {
                resolve_extension_fqn(&member_name, &raw_ty, resolver)
                    .map(|fqn| (fqn, SymbolKind::FUNCTION))
            });
        }
        if let Some((fqn, kind)) = resolved {
            emit(usages, file_id, member.text_range(), kind, member_name, fqn);
        }
        if let NodeOrToken::Node(n) = &receiver {
            walk(n, file_id, resolver, imports, usages, ctx);
        }
    }
}
