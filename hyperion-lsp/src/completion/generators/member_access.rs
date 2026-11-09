// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Member access completion generator
//!
//! Generates completions for member access expressions (obj.█)
//! Handles fields, methods, and inherited members with visibility rules.

mod access;

use self::access::MemberAccessContext;
use super::context::{CompletionGenerator, CompletionGeneratorContext};
use crate::completion::chain_snippets::{
    completable_future_snippets, is_completable_future_type, is_optional_type, is_stream_type,
    optional_chain_snippets, stream_chain_snippets,
};
use crate::completion::utils;
use crate::navigation::import_resolver::extract_package_from_root;
use crate::position_utils::position_to_offset;
use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{ParseDatabase, SemanticDatabase};
use hyperion_semantics::symbol::{ClassType, Type};
use hyperion_semantics::TypeInferenceEngine;
use hyperion_workspace::FileId;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionItemTag,
    Documentation, InsertTextFormat, MarkupContent, MarkupKind, SymbolKind,
};
use tracing::debug;

/// Resolved receiver type for member access completion
pub enum ReceiverType {
    /// Instance access on an object with the given FQN
    Instance(String, Vec<String>),
    /// Static access on a class with the given FQN
    Static(String, Vec<String>),
    /// Array type (e.g., `int[]`, `String[]`)
    Array,
    /// Primitive type (no members available)
    Primitive,
}

impl ReceiverType {
    /// Extract the FQN if this is an Instance or Static receiver
    pub fn to_fqn(self) -> Option<String> {
        match self {
            Self::Instance(fqn, _) | Self::Static(fqn, _) => Some(fqn),
            Self::Array | Self::Primitive => None,
        }
    }
}

/// Count arguments in a method call expression.
///
/// `"foo()"` → `Some(0)`, `"foo(a, b)"` → `Some(2)`, `"foo(bar(x), y)"` → `Some(2)`.
/// Returns `None` if no parentheses are found.
fn count_method_arguments(receiver: &str) -> Option<usize> {
    let open = receiver.find('(')?;
    let inner = &receiver[open + 1..];

    // Find matching close paren (respecting strings)
    let mut depth: u32 = 1;
    let mut in_string = false;
    let mut prev_was_escape = false;
    let mut close_offset = None;
    for (i, ch) in inner.char_indices() {
        if in_string {
            if ch == '"' && !prev_was_escape {
                in_string = false;
            }
            prev_was_escape = ch == '\\' && !prev_was_escape;
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close_offset = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let args_str = &inner[..close_offset?];
    let trimmed = args_str.trim();
    if trimmed.is_empty() {
        return Some(0);
    }

    // Count top-level commas (not inside parens, generics, brackets, or strings)
    let mut count = 1usize;
    let mut d: u32 = 0;
    let mut in_str = false;
    let mut prev_esc = false;
    for ch in trimmed.chars() {
        if in_str {
            if ch == '"' && !prev_esc {
                in_str = false;
            }
            prev_esc = ch == '\\' && !prev_esc;
            continue;
        }
        match ch {
            '"' => in_str = true,
            '(' | '<' | '[' => d += 1,
            ')' | '>' | ']' => d = d.saturating_sub(1),
            ',' if d == 0 => count += 1,
            _ => {}
        }
    }
    Some(count)
}

/// An expression's source text without the layout between its tokens
/// (`names\n    .stream()` -> `names.stream()`); string literals are kept verbatim.
fn compact_expression_text(text: &str) -> String {
    if text.contains('"') {
        return text.trim().to_string();
    }
    text.split_whitespace().collect()
}

/// `a`, `a.b`, `a.b.C`: identifiers joined by dots, nothing else.
fn is_dotted_name(text: &str) -> bool {
    !text.is_empty()
        && text.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
                && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        })
}

/// Generator for member access completions (fields, methods)
pub struct MemberAccessCompletionGenerator;

/// Resolve a type using the file's imports
///
/// When a symbol table stores a type with the wrong package (e.g., using the current
/// file's package instead of the imported class's package), this function resolves
/// the type name using the file's imports to find the correct FQN.
fn resolve_type_with_imports(
    typ: &Type,
    file_id: FileId,
    workspace_index: &Arc<hyperion_workspace::WorkspaceIndex>,
) -> Type {
    match typ {
        Type::Class(class_type) => {
            // If the type has a package, check if we should resolve it via imports
            // This handles the case where the cached symbol table incorrectly sets
            // the package to the current file's package instead of resolving imports
            let simple_name = &class_type.name;

            // Try to resolve via imports
            if let Some(import_map) = workspace_index.get_imports(file_id) {
                // Check explicit imports first
                if let Some(fqn) = import_map.explicit.get::<str>(simple_name.as_ref()) {
                    // Parse the FQN to get the correct package
                    if let Some(last_dot) = fqn.rfind('.') {
                        let package = &fqn[..last_dot];
                        debug!(
                            "Resolved type '{}' via import to package '{}'",
                            simple_name, package
                        );
                        return Type::Class(ClassType {
                            name: simple_name.to_string(),
                            package: Some(package.to_string()),
                            type_arguments: class_type.type_arguments.clone(),
                            is_diamond: class_type.is_diamond,
                        });
                    }
                }
            }

            // If not resolved via imports, check if the class exists in workspace
            // with its declared package
            if let Some(ref pkg) = class_type.package {
                let fqn = format!("{pkg}.{simple_name}");
                if workspace_index.has_class(&fqn) {
                    // Package is correct, return as-is
                    return typ.clone();
                }
            }

            // For default package (package: None), first check if the class exists with simple name
            // This prevents incorrectly resolving to JDK classes like sun.launcher.Helper
            if class_type.package.is_none() {
                // Check if the class exists in workspace by simple name (default package)
                if workspace_index.has_class(simple_name) {
                    return typ.clone();
                }
            }

            // Try to find the class in workspace by simple name (handles packaged classes)
            // This is a last-resort fallback after import resolution and direct lookups.
            // Unconditional: package: None types from find_method_in_hierarchy need this path.
            if let Some(fqn) = workspace_index.find_class_by_suffix(simple_name) {
                if let Some(last_dot) = fqn.rfind('.') {
                    let package = &fqn[..last_dot];
                    debug!(
                        "Resolved type '{}' from workspace to package '{}'",
                        simple_name, package
                    );
                    return Type::Class(ClassType {
                        name: simple_name.to_string(),
                        package: Some(package.to_string()),
                        type_arguments: class_type.type_arguments.clone(),
                        is_diamond: class_type.is_diamond,
                    });
                }
            }

            // Couldn't resolve, return original
            typ.clone()
        }
        // For non-class types, return as-is
        _ => typ.clone(),
    }
}

/// Shorten every qualified name in a rendered type: `java.util.List<java.lang.String>`
/// becomes `List<String>`.
pub(crate) fn shorten_type(rendered: &str) -> String {
    let chars: Vec<char> = rendered.chars().collect();
    let mut out = String::with_capacity(rendered.len());
    let mut segment = String::new();
    for (index, &c) in chars.iter().enumerate() {
        let qualifier_dot = c == '.'
            && !segment.is_empty()
            && chars
                .get(index + 1)
                .is_some_and(|next| next.is_alphabetic() || *next == '_');
        if c.is_alphanumeric() || c == '_' || c == '$' {
            segment.push(c);
        } else if qualifier_dot {
            // A package or outer-type qualifier: drop it, keep the last segment.
            segment.clear();
        } else {
            out.push_str(&segment);
            segment.clear();
            out.push(c);
        }
    }
    out.push_str(&segment);
    out
}

/// Member types reachable as `Outer.Inner` when completing on a type name.
fn nested_type_items(
    workspace_index: &hyperion_workspace::index::WorkspaceIndex,
    outer_fqn: &str,
) -> Vec<CompletionItem> {
    workspace_index
        .nested_type_symbols(outer_fqn)
        .into_iter()
        .filter(|symbol| {
            !symbol.modifiers.is_private
                && (symbol.modifiers.is_public
                    || symbol.modifiers.is_protected
                    || workspace_index
                        .find_symbol(&symbol.fqn)
                        .is_none_or(|location| !workspace_index.is_external_file(location.file_id)))
        })
        .map(|symbol| {
            let metadata =
                crate::completion::metadata::CompletionItemMetadata::new(&symbol.simple_name)
                    .with_fqn(symbol.fqn.to_string())
                    .with_group(crate::completion::metadata::rank_group::NESTED_TYPE);
            CompletionItem {
                label: symbol.simple_name.to_string(),
                label_details: Some(CompletionItemLabelDetails {
                    detail: None,
                    description: Some(outer_fqn.to_string()),
                }),
                kind: Some(crate::completion::type_search::completion_kind(&symbol)),
                detail: Some(symbol.fqn.to_string()),
                insert_text: Some(symbol.simple_name.to_string()),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                data: metadata.to_value(),
                tags: symbol
                    .is_deprecated
                    .then(|| vec![CompletionItemTag::DEPRECATED]),
                ..Default::default()
            }
        })
        .collect()
}

/// Render `Type name` parameter declarations for display.
fn render_parameters(declarations: &[String]) -> String {
    declarations
        .iter()
        .map(|declaration| shorten_type(declaration))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The completion item for one member of the receiver.
fn member_item(
    symbol: &hyperion_workspace::Symbol,
    projected_type: Option<String>,
    parameters: Option<Vec<String>>,
    snippets: bool,
) -> CompletionItem {
    let simple_name = symbol.simple_name.to_string();
    let is_method = symbol.kind == SymbolKind::METHOD;
    let kind = match symbol.kind {
        SymbolKind::METHOD => CompletionItemKind::METHOD,
        SymbolKind::FIELD => CompletionItemKind::FIELD,
        SymbolKind::PROPERTY => CompletionItemKind::PROPERTY,
        SymbolKind::ENUM_MEMBER => CompletionItemKind::ENUM_MEMBER,
        _ => CompletionItemKind::FIELD,
    };

    let mut metadata = crate::completion::metadata::CompletionItemMetadata::new(&simple_name)
        .with_fqn(symbol.fqn.to_string());
    if let Some(owner) = &symbol.container {
        metadata = metadata.with_declared_in(owner.to_string());
    }
    if let Some(return_type) = &projected_type {
        metadata = metadata.with_item_type(return_type.to_string());
    }

    let returns = projected_type.as_deref().map(shorten_type);
    let (label_details, detail, insert_text, format) = if is_method {
        let declarations = parameters.unwrap_or_default();
        let rendered = render_parameters(&declarations);
        let insert = if snippets && !declarations.is_empty() {
            let placeholders = declarations
                .iter()
                .enumerate()
                .map(|(index, declaration)| {
                    let name = declaration.rsplit(' ').next().unwrap_or("arg");
                    format!("${{{}:{}}}", index + 1, name.replace(['$', '}', '\\'], ""))
                })
                .collect::<Vec<_>>()
                .join(", ");
            (
                format!("{simple_name}({placeholders})$0"),
                InsertTextFormat::SNIPPET,
            )
        } else {
            (format!("{simple_name}()"), InsertTextFormat::PLAIN_TEXT)
        };
        (
            CompletionItemLabelDetails {
                detail: Some(format!("({rendered})")),
                description: returns.clone(),
            },
            Some(format!(
                "{} {simple_name}({rendered})",
                returns.as_deref().unwrap_or("void")
            )),
            insert.0,
            insert.1,
        )
    } else {
        (
            CompletionItemLabelDetails {
                detail: None,
                description: returns.clone(),
            },
            returns.as_ref().map(|ty| format!("{ty} {simple_name}")),
            simple_name.clone(),
            InsertTextFormat::PLAIN_TEXT,
        )
    };

    CompletionItem {
        label: simple_name.clone(),
        label_details: Some(label_details),
        kind: Some(kind),
        detail,
        documentation: symbol.documentation.as_ref().map(|doc| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc.to_string(),
            })
        }),
        preselect: None,
        sort_text: Some(format!("1_{simple_name}")),
        filter_text: None,
        insert_text: Some(insert_text),
        insert_text_format: Some(format),
        insert_text_mode: None,
        text_edit: None,
        additional_text_edits: None,
        command: None,
        commit_characters: None,
        data: metadata.to_value(),
        tags: symbol
            .is_deprecated
            .then(|| vec![CompletionItemTag::DEPRECATED]),
        deprecated: symbol.is_deprecated.then_some(true),
    }
}

impl Default for MemberAccessCompletionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl MemberAccessCompletionGenerator {
    pub const fn new() -> Self {
        Self
    }

    /// Extract the receiver text from the source before the dot.
    ///
    /// Handles paren-balancing for method calls like `getUser()` and finds
    /// the start of the identifier chain.
    fn extract_receiver_text<'a>(
        &self,
        source_text: &'a str,
        offset: usize,
    ) -> Result<&'a str, String> {
        let before_cursor = &source_text[..offset];
        let dot_pos = before_cursor
            .rfind('.')
            .ok_or("No dot found before cursor")?;
        let before_dot = &source_text[..dot_pos];

        debug!(
            "Text before dot: {:?}",
            before_dot.chars().rev().take(50).collect::<String>()
        );

        let trimmed = before_dot.trim_end();
        let bytes = trimmed.as_bytes();
        let mut scan_end = trimmed.len();

        // Skip balanced parentheses from right to left for method calls
        if scan_end > 0 && bytes[scan_end - 1] == b')' {
            let mut paren_depth: u32 = 0;
            let mut i = scan_end;
            while i > 0 {
                i -= 1;
                if bytes[i] == b')' {
                    paren_depth += 1;
                } else if bytes[i] == b'(' {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        scan_end = i;
                        break;
                    }
                }
            }
        }

        let chain_start = trimmed[..scan_end]
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_alphanumeric() && *c != '_' && *c != '.')
            .map_or(0, |(i, _)| i + 1);

        let receiver = trimmed[chain_start..].trim();

        debug!("Receiver: '{}'", receiver);

        if receiver.is_empty() {
            return Err("Empty receiver".to_string());
        }

        Ok(receiver)
    }

    /// Resolve the receiver type from an expression before the dot.
    ///
    /// Tries strategies in order:
    /// 1. TypeInferenceEngine for complex receivers (chains with `.` or `()`)
    /// 2. Symbol table lookup → method call or variable branch
    /// 3. Class name fallback (static access)
    pub fn resolve_receiver_type(
        &self,
        context: &CompletionGeneratorContext<'_>,
        receiver: &str,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
        symbol_table: &Arc<hyperion_semantics::symbol::SymbolTable>,
        scope: hyperion_semantics::symbol::ScopeId,
    ) -> Result<ReceiverType, String> {
        if matches!(receiver, "this" | "super") {
            let caller_package = extract_package_from_root(root);
            let caller_owner =
                MemberAccessContext::caller_owner_at(root, offset, caller_package.as_deref())
                    .ok_or_else(|| format!("Could not resolve enclosing type for '{receiver}'"))?;
            let caller_arguments = context
                .workspace_index
                .project_owner_type_arguments(&caller_owner, &[], &caller_owner)
                .map(|parameters| {
                    parameters
                        .into_iter()
                        .map(|(parameter, _)| parameter.name.to_string())
                        .collect()
                })
                .unwrap_or_default();
            if receiver == "this" {
                return Ok(ReceiverType::Instance(caller_owner, caller_arguments));
            }
            let parent = context
                .workspace_index
                .get_parent_class(&caller_owner)
                .ok_or_else(|| format!("Type '{caller_owner}' has no resolvable superclass"))?;
            let parent_arguments = context
                .workspace_index
                .project_owner_type_arguments(&caller_owner, &caller_arguments, &parent)
                .and_then(|arguments| {
                    arguments
                        .into_iter()
                        .map(|(_, argument)| argument)
                        .collect::<Option<Vec<_>>>()
                })
                .unwrap_or_default();
            return Ok(ReceiverType::Instance(parent, parent_arguments));
        }

        // Prefer flow-sensitive narrowed types (e.g., after instanceof) for simple receivers.
        if !receiver.contains('.') && !receiver.contains('(') {
            if let Some(narrowed) = symbol_table.lookup_narrowed_type(receiver, offset) {
                debug!(
                    "Using narrowed type for receiver '{}': {:?}",
                    receiver, narrowed
                );
                return Self::type_to_receiver_type(narrowed);
            }
        }

        // Most member completions are on a plainly declared local, parameter, or
        // field. Resolve those from the already-built symbol table before asking
        // FileSemantics to walk and infer every expression in a large source file.
        // Keep FileSemantics as the authoritative fallback for unknown/complex
        // cases, including `var` and flow-sensitive expressions.
        if !receiver.contains('.') && !receiver.contains('(') {
            if let Some(symbol) = symbol_table
                .lookup_symbol(receiver, scope)
                .and_then(|id| symbol_table.get_symbol(id))
            {
                let is_declared_value = matches!(
                    symbol.kind,
                    hyperion_semantics::symbol::SymbolKind::Field { .. }
                        | hyperion_semantics::symbol::SymbolKind::LocalVariable { .. }
                        | hyperion_semantics::symbol::SymbolKind::Parameter { .. }
                        | hyperion_semantics::symbol::SymbolKind::EnumConstant { .. }
                );
                if is_declared_value && !matches!(symbol.type_info, Type::Unknown) {
                    debug!(
                        "Symbol table fast path resolved receiver '{}': {:?}",
                        receiver, symbol.type_info
                    );
                    return self.resolve_variable_type(
                        context,
                        receiver,
                        offset,
                        root,
                        symbol_table,
                        symbol,
                    );
                }
            }
        }

        // A receiver spelled as a plain (dotted) name whose head is not a
        // variable is a type: `Collections.`, `Outer.Inner.`, `java.util.List.`.
        // The expression-type table cannot tell `Test` (the type) from a value
        // of type `Test`, so this has to be decided by name resolution first.
        if is_dotted_name(receiver) {
            let head = receiver.split('.').next().unwrap_or(receiver);
            let head_is_value = symbol_table
                .lookup_symbol(head, scope)
                .and_then(|id| symbol_table.get_symbol(id))
                .is_some_and(|symbol| {
                    matches!(
                        symbol.kind,
                        hyperion_semantics::symbol::SymbolKind::Field { .. }
                            | hyperion_semantics::symbol::SymbolKind::LocalVariable { .. }
                            | hyperion_semantics::symbol::SymbolKind::Parameter { .. }
                            | hyperion_semantics::symbol::SymbolKind::EnumConstant { .. }
                    )
                });
            if !head_is_value {
                if let Ok(fqn) = self.resolve_written_type(context, receiver, root, offset) {
                    return Ok(ReceiverType::Static(fqn, Vec::new()));
                }
            }
        }

        // Primary path: FileSemantics name resolution for simple receivers
        if !receiver.contains('.') && !receiver.contains('(') {
            if let Some(receiver_type) =
                self.resolve_receiver_via_file_semantics(context, receiver, root, offset)
            {
                debug!("FileSemantics resolved receiver type for '{}'", receiver);
                return Ok(receiver_type);
            }
        }

        // Fast path: receiver rooted at an implicitly-typed lambda parameter
        // (e.g. `entry.getValue()` from `.map(entry -> ...)`). ResolverBuilder
        // never assigns a type to such parameters (no type node to extract, and
        // nothing infers the SAM parameter type during scope construction), so
        // the symbol table -- which both FileSemantics' whole-file semantic
        // model and TypeInferenceEngine below ultimately consult -- always
        // reports `Type::Unknown` for it. Those two paths therefore cannot
        // structurally succeed here; detect the lambda-param-rooted case
        // cheaply (a syntax-tree walk, no workspace/Salsa work) and go
        // straight to the SAM-based inference already used for hover/
        // navigation, instead of paying for two whole-file/semantic attempts
        // that are guaranteed to fail first. This turns a multi-second
        // member completion at
        // `entry.getValue().<|>` into a fast path: see
        // `infer_receiver_type_via_lambda_binding`'s doc comment for the full
        // history.
        let receiver_node = self.find_receiver_expression_node(root, offset);
        if let Some(ref node) = receiver_node {
            if crate::navigation::lambda_inference::receiver_is_lambda_param_rooted(node) {
                if let Some(inferred) =
                    crate::navigation::lambda_inference::infer_receiver_type_via_lambda_binding(
                        node,
                        root,
                        &context.workspace_index,
                        context.file_id,
                    )
                {
                    debug!(
                        "Lambda-binding fast path resolved receiver type for '{}': {:?}",
                        receiver, inferred
                    );
                    return Self::type_to_receiver_type(&inferred);
                }
                debug!(
                    "Receiver '{}' is lambda-param-rooted but lambda-binding inference \
                     failed; skipping FileSemantics/TypeInferenceEngine (structurally \
                     cannot resolve it either) and falling back to symbol table",
                    receiver
                );
            }
        }

        // FileSemantics expression_type_at for all receivers (simple and complex)
        if let Some(receiver_node) = self.find_receiver_expression_node(root, offset) {
            let file_sem = {
                use hyperion_semantics::db::SemanticDatabase;
                context.snapshot.file_semantics(context.file_id)
            };
            if let Some(cached_type) = file_sem.expression_type_at(receiver_node.text_range()) {
                debug!(
                    "FileSemantics resolved receiver expression type for '{}'",
                    receiver
                );
                return Self::type_to_receiver_type(cached_type);
            }
        }

        // Fallback for complex receivers: TypeInferenceEngine
        if receiver.contains('.') || receiver.contains('(') {
            debug!("Complex receiver detected, trying TypeInferenceEngine");
            if let Some(inferred) =
                self.infer_receiver_type_with_engine(context, receiver, root, offset)
            {
                debug!("TypeInferenceEngine successfully resolved receiver type");
                return Self::type_to_receiver_type(&inferred);
            }
        }

        // Fall back to symbol table lookup
        debug!("Falling back to symbol table lookup");

        let (lookup_name, is_method_call) = if let Some(paren_pos) = receiver.find('(') {
            (&receiver[..paren_pos], true)
        } else {
            (receiver, false)
        };

        let arg_count = if is_method_call {
            count_method_arguments(receiver)
        } else {
            None
        };

        let resolved_opt = symbol_table
            .lookup_symbol(lookup_name, scope)
            .and_then(|id| symbol_table.get_symbol(id));

        debug!(
            "Resolved symbol for '{}' (method_call={}): {:?}",
            lookup_name,
            is_method_call,
            resolved_opt
                .as_ref()
                .map(|s| (&s.name, &s.kind, &s.type_info))
        );

        if let Some(symbol) = resolved_opt {
            if is_method_call {
                self.resolve_method_call_type(context, lookup_name, symbol, arg_count)
            } else {
                self.resolve_variable_type(context, receiver, offset, root, symbol_table, symbol)
            }
        } else if is_method_call {
            // Method call but symbol not found locally — try workspace hierarchy
            // This handles inherited methods where the local symbol table doesn't
            // contain the method definition (it's in a parent class)
            debug!(
                "Method '{}' not in local symbol table, trying workspace hierarchy",
                lookup_name
            );
            if let Some(fqn) =
                self.resolve_method_return_type_via_workspace(context, lookup_name, arg_count)
            {
                debug!(
                    "Resolved inherited method return type via workspace: {}",
                    fqn
                );
                hyperion_semantics::type_parser::parse_type_string(&fqn)
                    .as_ref()
                    .map(Self::type_to_receiver_type)
                    .transpose()?
                    .ok_or_else(|| format!("Could not parse return type '{fqn}'"))
            } else {
                Err(format!(
                    "Could not resolve return type for method '{}'",
                    lookup_name
                ))
            }
        } else {
            // Could not resolve as variable - try as class name (static access)
            debug!(
                "Could not resolve '{}' as variable, trying as class name",
                receiver
            );
            let class_fqn = self.resolve_written_type(context, receiver, root, offset)?;
            debug!("Resolved as class with FQN: {}", class_fqn);
            Ok(ReceiverType::Static(class_fqn, Vec::new()))
        }
    }

    /// Try resolving a simple receiver via FileSemantics.
    ///
    /// For single-token receivers like `user`, find the token in the AST and check
    /// if FileSemantics resolved it to a LocalVariable or Field with a known type.
    fn resolve_receiver_via_file_semantics(
        &self,
        context: &CompletionGeneratorContext<'_>,
        receiver: &str,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
    ) -> Option<ReceiverType> {
        use hyperion_semantics::resolved_name::ResolvedName;
        use rowan::TextSize;

        // Find the receiver token by scanning backward from the dot position.
        // offset points at/after the dot, so look just before it.
        let search_offset = offset.checked_sub(1)?;
        let token = root
            .token_at_offset(TextSize::from(search_offset))
            .left_biased()?;

        // Verify this token matches the receiver text
        if token.text() != receiver {
            return None;
        }

        let file_sem = context.snapshot.file_semantics(context.file_id);
        let resolved = file_sem.resolve_at(token.text_range())?;

        match resolved {
            ResolvedName::LocalVariable { var_type, .. }
            | ResolvedName::Field {
                field_type: var_type,
                ..
            } => {
                let resolved =
                    resolve_type_with_imports(var_type, context.file_id, &context.workspace_index);
                let receiver_type = Self::type_to_receiver_type(&resolved).ok()?;
                debug!(
                    "FileSemantics: receiver '{}' has type '{:?}'",
                    receiver, resolved
                );
                Some(receiver_type)
            }
            ResolvedName::Type { fqn, .. } | ResolvedName::External { fqn, .. } => {
                debug!(
                    "FileSemantics: receiver '{}' is static type '{}'",
                    receiver, fqn
                );
                Some(ReceiverType::Static(fqn.clone(), Vec::new()))
            }
            _ => None,
        }
    }

    /// Resolve the return type of a method call receiver.
    ///
    /// Handles two cases:
    /// - Unknown return type: falls back to workspace index
    /// - Known return type: resolves via imports
    fn resolve_method_call_type(
        &self,
        context: &CompletionGeneratorContext<'_>,
        method_name: &str,
        symbol: &hyperion_semantics::symbol::Symbol,
        arg_count: Option<usize>,
    ) -> Result<ReceiverType, String> {
        if matches!(symbol.type_info, Type::Unknown) {
            debug!(
                "Method call '{}' has Unknown return type, trying workspace index",
                method_name
            );
            if let Some(fqn) =
                self.resolve_method_return_type_via_workspace(context, method_name, arg_count)
            {
                debug!("Resolved method return type via workspace: {}", fqn);
                hyperion_semantics::type_parser::parse_type_string(&fqn)
                    .as_ref()
                    .map(Self::type_to_receiver_type)
                    .transpose()?
                    .ok_or_else(|| format!("Could not parse return type '{fqn}'"))
            } else {
                Err(format!(
                    "Could not resolve return type for method '{}'",
                    method_name
                ))
            }
        } else {
            let resolved = resolve_type_with_imports(
                &symbol.type_info,
                context.file_id,
                &context.workspace_index,
            );
            Self::type_to_receiver_type(&resolved)
        }
    }

    /// Resolve the type of a variable receiver.
    ///
    /// Checks narrowed types first, then falls back to declared type with
    /// var inference caching for `var` declarations.
    fn resolve_variable_type(
        &self,
        context: &CompletionGeneratorContext<'_>,
        receiver: &str,
        offset: u32,
        root: &hyperion_parser::syntax::SyntaxNode,
        symbol_table: &Arc<hyperion_semantics::symbol::SymbolTable>,
        symbol: &hyperion_semantics::symbol::Symbol,
    ) -> Result<ReceiverType, String> {
        // Check for narrowed type (e.g., after instanceof)
        if let Some(narrowed) = symbol_table.lookup_narrowed_type(receiver, offset) {
            debug!("Found narrowed type for '{}': {:?}", receiver, narrowed);
            return Self::type_to_receiver_type(narrowed);
        }

        // No narrowed type - use declared type with var inference caching
        let raw_type = if matches!(symbol.type_info, Type::Unknown) {
            debug!("Type is Unknown, checking cache before targeted var inference");
            let cache_key = (context.file_id, symbol.location.range.start);

            let mut cache = context.var_type_cache.lock();
            if let Some(cached_type) = cache.get(&cache_key) {
                debug!(
                    "Cache hit for var at offset {}",
                    symbol.location.range.start
                );
                cached_type.clone()
            } else {
                drop(cache); // Release lock before expensive inference

                debug!("Cache miss, doing targeted var inference");
                let inferred_type = self.infer_var_type_targeted(
                    context,
                    root,
                    symbol_table,
                    receiver,
                    symbol.location.range.start,
                );
                if inferred_type.is_none() {
                    debug!(
                        "Type::Unknown: targeted var inference failed for receiver '{}' at offset {}",
                        receiver,
                        symbol.location.range.start
                    );
                }
                let inferred_type = inferred_type.unwrap_or(Type::Unknown);

                let mut cache = context.var_type_cache.lock();
                cache.put(cache_key, inferred_type.clone());
                debug!(
                    "Cached var type at offset {}: {:?}",
                    symbol.location.range.start, inferred_type
                );

                inferred_type
            }
        } else {
            symbol.type_info.clone()
        };

        let resolved =
            resolve_type_with_imports(&raw_type, context.file_id, &context.workspace_index);
        debug!("Resolved variable type: {:?}", resolved);
        Self::type_to_receiver_type(&resolved)
    }

    /// Convert a semantic `Type` to a `ReceiverType`
    fn type_to_receiver_type(typ: &Type) -> Result<ReceiverType, String> {
        if matches!(typ, Type::Primitive(_)) {
            debug!("Receiver is primitive type, no members available");
            return Ok(ReceiverType::Primitive);
        }
        if matches!(typ, Type::Array { .. }) {
            return Ok(ReceiverType::Array);
        }
        let fqn = utils::type_to_fqn(typ)?;
        let arguments = match typ {
            Type::Class(class) => class
                .type_arguments
                .iter()
                .map(ToString::to_string)
                .collect(),
            _ => Vec::new(),
        };
        Ok(ReceiverType::Instance(fqn, arguments))
    }

    /// Append chain snippets for Stream, Optional, and CompletableFuture types
    fn add_chain_snippets(type_fqn: &str, members: &mut Vec<CompletionItem>) {
        if is_stream_type(type_fqn) {
            debug!("Adding Stream chain snippets for type: {}", type_fqn);
            members.extend(stream_chain_snippets());
        } else if is_optional_type(type_fqn) {
            debug!("Adding Optional chain snippets for type: {}", type_fqn);
            members.extend(optional_chain_snippets());
        } else if is_completable_future_type(type_fqn) {
            debug!(
                "Adding CompletableFuture chain snippets for type: {}",
                type_fqn
            );
            members.extend(completable_future_snippets());
        }
    }

    /// Perform targeted var type inference for a single variable
    fn infer_var_type_targeted(
        &self,
        context: &CompletionGeneratorContext<'_>,
        root: &hyperion_parser::syntax::SyntaxNode,
        symbol_table: &std::sync::Arc<hyperion_semantics::symbol::SymbolTable>,
        var_name: &str,
        expected_offset: u32,
    ) -> Option<Type> {
        use hyperion_semantics::resolver::ResolverBuilder;

        // Find the LOCAL_VAR_DECL node for this variable
        let var_decl = self.find_var_declaration_node(root, var_name, expected_offset)?;

        debug!("Found var declaration node for '{}'", var_name);

        // Create a ResolverBuilder with the cached symbol table
        let builder = ResolverBuilder::for_var_inference_cached(
            context.file_id,
            symbol_table.clone(),
            context.workspace_index.clone(),
        );

        // Infer the type from just this node
        let inferred_type = builder.infer_var_type(&var_decl);
        debug!("Inferred var type: {:?}", inferred_type);

        Some(inferred_type)
    }

    /// Find the LOCAL_VAR_DECL node for a variable at a specific offset
    fn find_var_declaration_node(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        var_name: &str,
        expected_offset: u32,
    ) -> Option<hyperion_parser::syntax::SyntaxNode> {
        // Walk the tree to find LOCAL_VAR_DECL nodes
        for node in root.descendants() {
            if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                let start = u32::from(node.text_range().start());
                // Check if this is approximately at the expected offset
                if start <= expected_offset && expected_offset < start + 100 {
                    // Verify this node declares the variable we're looking for
                    for child in node.children_with_tokens() {
                        if let Some(token) = child.as_token() {
                            if token.kind() == SyntaxKind::IDENT && token.text() == var_name {
                                return Some(node);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Infer receiver type using TypeInferenceEngine for complex expressions
    ///
    /// This handles method chains like `user.getProfile()` by using
    /// TypeInferenceEngine to track types through the chain.
    fn infer_receiver_type_with_engine(
        &self,
        context: &CompletionGeneratorContext,
        receiver: &str,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
    ) -> Option<Type> {
        // Check if receiver contains method calls or field access (indicating a chain)
        if !receiver.contains('.') && !receiver.contains('(') {
            // Simple identifier - fall back to symbol table lookup
            return None;
        }

        debug!(
            "Attempting TypeInferenceEngine for receiver: '{}'",
            receiver
        );

        // Create TypeInferenceEngine
        let engine = TypeInferenceEngine::new(context.workspace_index.clone(), context.file_id);

        // Use cached symbol table from Salsa for better performance
        // This avoids rebuilding the symbol table on every completion request
        let cached_table = context.snapshot.file_symbol_table(context.file_id);
        let resolver =
            hyperion_semantics::resolver::SymbolResolver::with_cached_table(cached_table);

        // Find the AST node for the receiver expression
        // We need to find the expression node just before the final dot
        let receiver_node = self.find_receiver_expression_node(root, offset)?;

        debug!("Found receiver expression node: {:?}", receiver_node.kind());

        // Use TypeInferenceEngine to infer the type
        let inferred_type = engine.infer_expression(&receiver_node, &resolver, None);

        debug!("TypeInferenceEngine inferred type: {:?}", inferred_type);

        if matches!(inferred_type, Type::Unknown) {
            None
        } else {
            Some(inferred_type)
        }
    }

    /// Find the AST node for the receiver expression before the final dot
    fn find_receiver_expression_node(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        cursor_offset: u32,
    ) -> Option<hyperion_parser::syntax::SyntaxNode> {
        // Preferred: locate the '.' immediately preceding the cursor and read the
        // receiver directly off ITS parent, without walking any further up the
        // tree. See `find_receiver_via_preceding_dot` for why the walk-up
        // fallback below can return the wrong node entirely when the completion
        // site is nested inside another call's arguments (e.g. a lambda body
        // that isn't the last thing in the file).
        if let Some(node) = Self::find_receiver_via_preceding_dot(root, cursor_offset) {
            return Some(node);
        }

        // Fallback: walk up from the token at the cursor looking for an
        // enclosing FIELD_ACCESS_EXPR/METHOD_CALL_EXPR. Kept for cursor
        // placements where the preceding token isn't a DOT.
        //
        // NOTE: this walk is known to be imprecise -- `node = token.parent()`
        // is never itself checked as a candidate (only `node.parent()`,
        // `node.parent().parent()`, ...), and the search does not stop at the
        // *innermost* qualifying ancestor, so it can cross ARG_LIST/LAMBDA_EXPR
        // boundaries and return an unrelated outer call's receiver. Prefer the
        // dot-based lookup above; this exists only as a safety net.
        let token = root
            .token_at_offset(rowan::TextSize::from(cursor_offset))
            .right_biased()?;

        // Walk up to find the parent expression node
        let mut node = token.parent()?;

        // Look for FIELD_ACCESS_EXPR or METHOD_CALL_EXPR containing our position
        while let Some(parent) = node.parent() {
            match parent.kind() {
                SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::METHOD_CALL_EXPR => {
                    // Found an expression - check if it contains a receiver before our dot
                    // The receiver is the first child before DOT token
                    for child in parent.children() {
                        // Check if this child is before the cursor
                        if child.text_range().end() <= rowan::TextSize::from(cursor_offset) {
                            // This could be our receiver expression
                            if matches!(
                                child.kind(),
                                SyntaxKind::NAME_EXPR
                                    | SyntaxKind::FIELD_ACCESS_EXPR
                                    | SyntaxKind::METHOD_CALL_EXPR
                                    | SyntaxKind::NEW_EXPR
                                    | SyntaxKind::PAREN_EXPR
                            ) {
                                return Some(child);
                            }
                        }
                    }
                }
                _ => {}
            }
            node = parent;
        }

        None
    }

    /// Find the receiver by locating the `.` token that directly precedes the
    /// cursor and reading its receiver off the node that owns that dot --
    /// without walking any further up the tree. This is the fix for the bug
    /// described on `find_receiver_expression_node`: it stays local to the
    /// dot actually being completed, so it can't cross into an unrelated
    /// enclosing call.
    fn find_receiver_via_preceding_dot(
        root: &hyperion_parser::syntax::SyntaxNode,
        cursor_offset: u32,
    ) -> Option<hyperion_parser::syntax::SyntaxNode> {
        const RECEIVER_KINDS: [SyntaxKind; 10] = [
            SyntaxKind::NAME_EXPR,
            SyntaxKind::FIELD_ACCESS_EXPR,
            SyntaxKind::METHOD_CALL_EXPR,
            SyntaxKind::NEW_EXPR,
            SyntaxKind::PAREN_EXPR,
            SyntaxKind::THIS_EXPR,
            SyntaxKind::SUPER_EXPR,
            SyntaxKind::ARRAY_ACCESS_EXPR,
            SyntaxKind::LITERAL_EXPR,
            SyntaxKind::CLASS_LITERAL_EXPR,
        ];
        // Skip the identifier prefix being typed, then whitespace, to reach
        // the dot being completed.
        let mut probe = root
            .token_at_offset(rowan::TextSize::from(cursor_offset))
            .left_biased()?;
        if probe.kind() == SyntaxKind::IDENT && u32::from(probe.text_range().end()) == cursor_offset
        {
            probe = probe.prev_token()?;
        }
        while probe.kind().is_trivia() {
            probe = probe.prev_token()?;
        }
        let dot = probe;
        if dot.kind() != SyntaxKind::DOT {
            return None;
        }
        let parent = dot.parent()?;
        if matches!(
            parent.kind(),
            SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::METHOD_CALL_EXPR
        ) {
            let owned = parent.children().find(|child| {
                child.text_range().end() <= dot.text_range().start()
                    && RECEIVER_KINDS.contains(&child.kind())
            });
            if owned.is_some() {
                return owned;
            }
        }
        // Error recovery leaves the trailing dot of `a.b().` as a stray token
        // beside the receiver it follows (`EXPR_STMT[METHOD_CALL_EXPR, DOT]`),
        // so the receiver is the expression node immediately before it.
        let mut previous = dot.prev_sibling_or_token();
        while let Some(element) = previous {
            if element.kind().is_trivia() {
                previous = element.prev_sibling_or_token();
                continue;
            }
            return element
                .into_node()
                .filter(|node| RECEIVER_KINDS.contains(&node.kind()));
        }
        None
    }

    /// Resolve a written type name (`Collections`, `Outer.Inner`,
    /// `java.util.List`) at the cursor through the canonical name-resolution
    /// engine: the file's own imports, the lexical owners at the
    /// cursor, same-package types and the language defaults, exactly as
    /// diagnostics and navigation see them.
    fn resolve_written_type(
        &self,
        context: &CompletionGeneratorContext<'_>,
        written: &str,
        root: &hyperion_parser::syntax::SyntaxNode,
        offset: u32,
    ) -> Result<String, String> {
        use hyperion_workspace::TypeNameResolution;
        let imports = hyperion_semantics::utils::build_imports_from_syntax(root);
        let caller_owner =
            MemberAccessContext::caller_owner_at(root, offset, imports.current_package.as_deref());
        let owners = access::lexical_owner_chain(&context.workspace_index, caller_owner.as_deref());
        match context.workspace_index.resolve_type_name_in_context(
            &imports,
            &owners,
            written,
            hyperion_workspace::Language::Java,
        ) {
            TypeNameResolution::Exact(fqn) => Ok(fqn),
            TypeNameResolution::Ambiguous(candidates) => Err(format!(
                "Ambiguous type '{written}': {}",
                candidates.join(", ")
            )),
            TypeNameResolution::Missing => Err(format!("Could not resolve class name: {written}")),
        }
    }

    /// Resolve a method's return type by looking it up in the workspace index.
    ///
    /// When the file-local symbol table stores `Unknown` for a method's return type
    /// (because it doesn't resolve cross-file types), this falls back to the workspace
    /// index which has fully resolved return type information.
    ///
    /// Walks up the class hierarchy so inherited methods are found too.
    fn resolve_method_return_type_via_workspace(
        &self,
        context: &CompletionGeneratorContext<'_>,
        method_name: &str,
        arg_count: Option<usize>,
    ) -> Option<String> {
        let file_symbols = context.workspace_index.get_file_symbols(context.file_id);

        for class_fqn in &file_symbols {
            if let Some(fqn) =
                self.find_method_in_hierarchy(context, class_fqn, method_name, arg_count)
            {
                return Some(fqn);
            }
        }

        None
    }

    /// Resolve a member's return type to an FQN using the defining class's imports.
    fn resolve_return_type_from_member(
        &self,
        context: &CompletionGeneratorContext<'_>,
        defining_class_fqn: &str,
        return_type_name: &str,
    ) -> Option<String> {
        let return_type = Type::Class(ClassType {
            name: return_type_name.to_string(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        });

        // Use the defining class's file for import resolution,
        // not the caller's file. The caller may not import the
        // return type, but the defining class likely does.
        let defining_file_id = context
            .workspace_index
            .find_symbol(defining_class_fqn)
            .map_or(context.file_id, |loc| loc.file_id);

        let resolved =
            resolve_type_with_imports(&return_type, defining_file_id, &context.workspace_index);

        utils::type_to_fqn(&resolved).ok()
    }

    /// Resolve the return type of `method_name` as seen through `class_fqn`.
    ///
    /// The candidate set is the canonical effective-method election
    /// so it is transitive through superclasses *and*
    /// superinterfaces; this used to be a `get_parent_class` loop over
    /// `get_class_members`, which never saw an interface `default` method.
    ///
    /// When `arg_count` is known it selects that arity; otherwise, and when no
    /// declaration of that arity carries a return type, the first declaration
    /// that does wins. Exact call sites use the overload resolver -- this is a
    /// completion recovery path for a receiver whose file-local symbol table
    /// recorded `Unknown`.
    fn find_method_in_hierarchy(
        &self,
        context: &CompletionGeneratorContext<'_>,
        class_fqn: &str,
        method_name: &str,
        arg_count: Option<usize>,
    ) -> Option<String> {
        let candidates = context
            .workspace_index
            .find_effective_methods_by_name(class_fqn, method_name);
        let typed = candidates
            .iter()
            .filter(|method| method.return_type.is_some() && method.container.is_some());
        let selected = arg_count
            .and_then(|arity| {
                typed
                    .clone()
                    .find(|method| method.parameters.len() == arity)
            })
            .or_else(|| typed.clone().next())?;
        let owner = selected.container.as_deref()?;
        let return_type = selected.return_type.as_deref()?;
        debug!("Found method '{method_name}' in '{owner}' with return type '{return_type}'");
        self.resolve_return_type_from_member(context, owner, return_type)
    }

    /// Get array type members (length field and clone method)
    fn get_array_members(&self) -> Vec<CompletionItem> {
        // Create metadata for array members
        let length_metadata = crate::completion::metadata::CompletionItemMetadata::new("length")
            .with_item_type("int".to_string());

        let clone_metadata = crate::completion::metadata::CompletionItemMetadata::new("clone")
            .with_item_type("Object".to_string());

        vec![
            CompletionItem {
                label: "length".to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("int".to_string()),
                documentation: Some(Documentation::String(
                    "The number of elements in this array".to_string(),
                )),
                insert_text: Some("length".to_string()),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                data: length_metadata.to_value(),
                ..Default::default()
            },
            CompletionItem {
                label: "clone".to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some("Object".to_string()),
                documentation: Some(Documentation::String(
                    "Creates and returns a copy of this array".to_string(),
                )),
                insert_text: Some("clone()".to_string()),
                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                data: clone_metadata.to_value(),
                ..Default::default()
            },
        ]
    }

    /// Get members of a type from the workspace index.
    fn get_type_members(
        &self,
        type_fqn: &str,
        receiver_arguments: &[String],
        is_static_access: bool,
        access: &MemberAccessContext,
        workspace_index: &hyperion_workspace::index::WorkspaceIndex,
        snippets: bool,
    ) -> Result<Vec<CompletionItem>, String> {
        debug!(
            "Getting members for type: {} (static: {})",
            type_fqn, is_static_access
        );

        // Handle simple class names (no package) - try to resolve via workspace
        let resolved_fqn = if type_fqn.contains('.') || workspace_index.has_class(type_fqn) {
            type_fqn.to_string()
        } else {
            let fqns = workspace_index.get_fqns_by_simple_name(type_fqn);
            match fqns.as_slice() {
                [] => type_fqn.to_string(),
                [fqn] => {
                    debug!("Resolved simple name '{}' to FQN '{}'", type_fqn, fqn);
                    fqn.clone()
                }
                _ => return Err(format!("Ambiguous receiver type '{type_fqn}'")),
            }
        };

        let mut members = workspace_index
            .find_projected_effective_methods(&resolved_fqn, receiver_arguments)
            .into_iter()
            .map(|method| {
                let (return_type, parameters) = method.projection.map_or((None, None), |sig| {
                    let declarations = sig.parameter_declarations();
                    (sig.return_type, Some(declarations))
                });
                (method.definition.symbol, return_type, parameters)
            })
            .collect::<Vec<_>>();
        members.extend(
            workspace_index
                .find_projected_effective_fields(&resolved_fqn, receiver_arguments)
                .into_iter()
                .map(|field| (field.definition.symbol, field.projected_type, None)),
        );

        let mut access_facts = access.for_receiver(workspace_index, &resolved_fqn);
        members.retain(|(symbol, _, _)| {
            // Preserve the established completion surface: although Java
            // permits selecting a static member through an expression, an
            // instance receiver suggests instance members only. Static access
            // through a type receives the static set.
            access_facts.includes(symbol, is_static_access)
        });

        debug!("Found {} members (including inherited)", members.len());

        // Convert to completion items. Semantic deduplication happens after all
        // generators run; doing it by label here would discard overloads.
        let completions: Vec<CompletionItem> = members
            .into_iter()
            .map(|(symbol, projected_type, parameters)| {
                member_item(&symbol, projected_type, parameters, snippets)
            })
            .collect();

        debug!("Deduplicated to {} unique completions", completions.len());

        let mut completions = completions;
        if is_static_access {
            completions.extend(nested_type_items(workspace_index, &resolved_fqn));
        }
        Ok(completions)
    }

    /// Member completions for a receiver already identified by FQN: instance
    /// members of a value of that type, or the static surface of the type.
    pub(crate) fn completions_for_fqn(
        &self,
        context: &CompletionGeneratorContext<'_>,
        fqn: &str,
        type_arguments: &[String],
        is_static: bool,
        caller_package: Option<&str>,
    ) -> Result<Vec<CompletionItem>, String> {
        let access = MemberAccessContext::for_package(caller_package);
        let receiver = if is_static {
            ReceiverType::Static(fqn.to_string(), type_arguments.to_vec())
        } else {
            ReceiverType::Instance(fqn.to_string(), type_arguments.to_vec())
        };
        self.completions_for_receiver_type(context, receiver, &access)
    }

    fn completions_for_receiver_type(
        &self,
        context: &CompletionGeneratorContext<'_>,
        receiver_type: ReceiverType,
        access: &MemberAccessContext,
    ) -> Result<Vec<CompletionItem>, String> {
        match receiver_type {
            ReceiverType::Primitive => Ok(Vec::new()),
            ReceiverType::Array => Ok(self.get_array_members()),
            ReceiverType::Instance(ref fqn, ref arguments)
            | ReceiverType::Static(ref fqn, ref arguments) => {
                let is_static = matches!(receiver_type, ReceiverType::Static(_, _));
                let mut members = self.get_type_members(
                    fqn,
                    arguments,
                    is_static,
                    access,
                    &context.workspace_index,
                    context.snippets,
                )?;
                Self::add_chain_snippets(fqn, &mut members);
                Ok(members)
            }
        }
    }
}

impl CompletionGenerator for MemberAccessCompletionGenerator {
    fn generate(
        &self,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String> {
        debug!(
            "Computing member access completions at {:?}",
            context.position
        );

        let offset = position_to_offset(context.source_text, context.position)?;
        let tree = context.snapshot.parse_file(context.file_id);
        let root = tree.syntax();
        let symbol_table = context.snapshot.file_symbol_table(context.file_id);

        // The receiver is what the syntax tree says precedes the dot; the
        // text scan is only the fallback for shapes the parser cannot recover.
        let receiver_owned = match self.find_receiver_expression_node(&root, offset as u32) {
            Some(node) => compact_expression_text(&node.text().to_string()),
            None => self
                .extract_receiver_text(context.source_text, offset)?
                .to_string(),
        };
        let receiver = receiver_owned.as_str();

        let scope = symbol_table
            .find_scope_at_offset(offset as u32)
            .ok_or_else(|| {
                debug!("Could not find scope at offset {}", offset);
                "Could not find scope".to_string()
            })?;

        let receiver_type = self.resolve_receiver_type(
            context,
            receiver,
            &root,
            offset as u32,
            &symbol_table,
            scope,
        )?;

        let caller_package = extract_package_from_root(&root);
        let access =
            MemberAccessContext::at_completion_site(&root, offset as u32, caller_package, receiver);
        self.completions_for_receiver_type(context, receiver_type, &access)
    }
}

#[cfg(test)]
mod tests;
