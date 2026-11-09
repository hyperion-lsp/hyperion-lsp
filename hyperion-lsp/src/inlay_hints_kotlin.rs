// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin inlay hints (`textDocument/inlayHint`).
//!
//! Two hint families, mirroring the Java provider in [`crate::inlay_hints`]:
//!
//! * **Type hints** on `val`/`var` declarations that omit the type annotation,
//!   rendered as `: Type` after the property name. The type comes from
//!   [`hyperion_semantics::KotlinTypeInferenceEngine`], the same engine hover
//!   uses, so `when`/`if`/`try` and lambda initializers get real types rather
//!   than the first literal in the token stream. A declaration the engine
//!   cannot type produces *no* hint — `Unknown` is never rendered.
//!
//! * **Parameter name hints** at call sites, rendered as `name = ` to match
//!   Kotlin's named-argument syntax. Following the IntelliJ heuristic these
//!   appear only for arguments that carry no name of their own — literals,
//!   negated literals, and short identifier references — and are suppressed
//!   when the argument already reads as the parameter (`greet(name)` for
//!   `fun greet(name: String)`).

use hyperion_kotlin::{SyntaxKind as K, SyntaxNode};
use hyperion_semantics::db::{FileId, ParseDatabase, SemanticDatabase};
use hyperion_workspace::WorkspaceIndex;
use rowan::TextSize;
use std::sync::Arc;
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Range};
use tracing::debug;

use crate::kotlin_calls::{
    analyze_call, best_signature, KotlinArgument, KotlinCalleeResolver, KotlinParameter,
};
use crate::position_utils::{offset_to_position, position_to_offset};
use crate::semantic_host::ReadTxn;

/// Longest identifier argument that still counts as a "short expression" and so
/// earns a parameter-name hint. Longer expressions are usually self-describing.
const MAX_HINTED_ARGUMENT_LEN: usize = 24;

/// Inlay hints for Kotlin buffers.
pub struct KotlinInlayHintsProvider<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
}

impl<'txn> KotlinInlayHintsProvider<'txn> {
    pub const fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self {
            txn,
            workspace_index,
        }
    }

    /// Hints for the visible `range` of a Kotlin document.
    pub fn inlay_hints(
        &self,
        file_id: FileId,
        range: Range,
        source_text: &str,
    ) -> Result<Vec<InlayHint>, String> {
        let start_offset = position_to_offset(source_text, range.start)
            .map_err(|e| format!("Invalid start position: {e}"))?;
        let end_offset = position_to_offset(source_text, range.end)
            .map_err(|e| format!("Invalid end position: {e}"))?;

        // try_lock_for: degrade to no hints rather than freeze the editor if
        // did_change currently holds the DB mutex.
        let (parse_tree, symbol_table) = {
            let snapshot = self.txn.snapshot();
            (
                snapshot.parse_file(file_id),
                snapshot.file_symbol_table(file_id),
            )
        };
        let root = hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());

        // One engine per request: its DSL-receiver memoisation is request-scoped,
        // matching how hover and navigation build it.
        let engine = hyperion_semantics::KotlinTypeInferenceEngine::new(
            Arc::clone(&self.workspace_index),
            file_id,
        );
        let callee_resolver = KotlinCalleeResolver {
            workspace_index: &self.workspace_index,
            file_id,
            root: &root,
        };

        let mut hints = Vec::new();
        for node in root.descendants() {
            let node_range = node.text_range();
            if usize::from(node_range.end()) < start_offset
                || usize::from(node_range.start()) > end_offset
            {
                continue;
            }

            match node.kind() {
                K::PROPERTY_DECL => {
                    if let Some(hint) = self.type_hint(&node, &engine, &symbol_table, source_text) {
                        hints.push(hint);
                    }
                }
                K::CALL_EXPR => {
                    self.parameter_hints(
                        &node,
                        &callee_resolver,
                        &engine,
                        &symbol_table,
                        source_text,
                        &mut hints,
                    );
                }
                _ => {}
            }
        }

        debug!("Kotlin inlay hints: generated {}", hints.len());
        Ok(hints)
    }

    /// `: Type` after the name of a `val`/`var` declared without an annotation.
    fn type_hint(
        &self,
        property: &SyntaxNode,
        engine: &hyperion_semantics::KotlinTypeInferenceEngine,
        symbol_table: &Arc<hyperion_semantics::SymbolTable>,
        source_text: &str,
    ) -> Option<InlayHint> {
        if is_destructuring(property) || property_has_explicit_type(property) {
            return None;
        }

        let name_end = property_name_end(property)?;
        let initializer = property_initializer(property)?;

        let mut resolver =
            hyperion_semantics::SymbolResolver::with_cached_table(Arc::clone(symbol_table));
        let inferred = match &initializer {
            rowan::NodeOrToken::Node(node) => engine.infer_expression(node, &mut resolver),
            rowan::NodeOrToken::Token(token) => engine.infer_token_expression(token, &mut resolver),
        };

        // Never render a placeholder: an uninferable declaration gets no hint.
        if matches!(inferred, hyperion_semantics::Type::Unknown) {
            return None;
        }
        let kotlin_type = hyperion_semantics::semantics_type_to_kotlin(&inferred);
        if matches!(kotlin_type, hyperion_kotlin::KotlinType::Unknown) {
            return None;
        }

        Some(InlayHint {
            position: offset_to_position(source_text, usize::from(name_end)),
            label: InlayHintLabel::String(format!(": {kotlin_type}")),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: Some(true),
            data: None,
        })
    }

    /// `name = ` before eligible arguments of a call.
    fn parameter_hints(
        &self,
        call_node: &SyntaxNode,
        callee_resolver: &KotlinCalleeResolver<'_>,
        engine: &hyperion_semantics::KotlinTypeInferenceEngine,
        symbol_table: &Arc<hyperion_semantics::SymbolTable>,
        source_text: &str,
        hints: &mut Vec<InlayHint>,
    ) {
        let Some(call) = analyze_call(call_node) else {
            return;
        };
        if call.args.is_empty() {
            return;
        }

        let receiver_fqn = call.receiver.as_ref().and_then(|receiver| {
            let mut resolver =
                hyperion_semantics::SymbolResolver::with_cached_table(Arc::clone(symbol_table));
            let inferred = match receiver {
                rowan::NodeOrToken::Node(node) => engine.infer_expression(node, &mut resolver),
                rowan::NodeOrToken::Token(token) => {
                    engine.infer_token_expression(token, &mut resolver)
                }
            };
            crate::kotlin_calls::type_to_fqn(&inferred)
        });

        let signatures = callee_resolver.resolve(&call, receiver_fqn.as_deref());
        if signatures.is_empty() {
            return;
        }
        let parameters = &signatures[best_signature(&signatures, &call)].parameters;

        for (index, argument) in call.args.iter().enumerate() {
            // A named argument already says what it binds to, and Kotlin
            // permits positional arguments after it only in restricted forms —
            // stop rather than risk mis-numbering the rest.
            if argument.name.is_some() {
                break;
            }
            let Some(parameter) = parameters.get(index) else {
                break;
            };
            if !should_hint(argument, parameter) {
                continue;
            }

            hints.push(InlayHint {
                position: offset_to_position(source_text, usize::from(argument.value_start())),
                label: InlayHintLabel::String(format!("{} = ", parameter.name)),
                kind: Some(InlayHintKind::PARAMETER),
                text_edits: None,
                tooltip: None,
                padding_left: None,
                padding_right: None,
                data: None,
            });
        }
    }
}

/// Does this argument earn a parameter-name hint?
///
/// The IntelliJ heuristic: hint literals (and negated literals), hint short
/// identifier references, hint nothing else — and never hint an argument whose
/// own text already names the parameter.
fn should_hint(argument: &KotlinArgument, parameter: &KotlinParameter) -> bool {
    // A vararg slot swallows an unbounded number of arguments, so `name = ` on
    // the first one would be actively wrong.
    if parameter.is_vararg {
        return false;
    }

    let Some(kind) = argument.value_kind() else {
        return false;
    };

    let text = argument.value_text();
    if text.is_empty() || text.len() > MAX_HINTED_ARGUMENT_LEN {
        return false;
    }

    // `greet(name)` for `fun greet(name: String)` reads fine already.
    if text.eq_ignore_ascii_case(&parameter.name) {
        return false;
    }

    match kind {
        K::LITERAL_EXPR => true,
        // `-1`, `!flag`: hint when the operand is itself hint-worthy.
        K::UNARY_EXPR => argument
            .value
            .as_ref()
            .and_then(rowan::NodeOrToken::as_node)
            .is_some_and(|node| {
                node.children_with_tokens().any(|child| {
                    matches!(child.kind(), K::LITERAL_EXPR | K::IDENT | K::BACKTICK_IDENT)
                })
            }),
        // A bare identifier reference is a token, not a node.
        K::IDENT | K::BACKTICK_IDENT => true,
        // Lambdas, object literals, nested calls, when/if expressions: the
        // argument is long enough to read on its own.
        _ => false,
    }
}

/// `val (a, b) = pair` — a destructuring declaration binds several names and
/// has no single place to hang a type.
fn is_destructuring(property: &SyntaxNode) -> bool {
    property
        .children_with_tokens()
        .filter(|child| !child.kind().is_trivia())
        .take_while(|child| child.kind() != K::EQ)
        .any(|child| child.kind() == K::L_PAREN)
}

/// Does the declaration carry a written type annotation?
///
/// Any node child before the `=` is the annotation — `TYPE_REF`,
/// `NULLABLE_TYPE` or `FUNCTION_TYPE`, all of which mean the editor already
/// shows the type.
fn property_has_explicit_type(property: &SyntaxNode) -> bool {
    for child in property.children_with_tokens() {
        if child.kind().is_trivia() {
            continue;
        }
        if child.kind() == K::EQ {
            return false;
        }
        if child.as_node().is_some() && !matches!(child.kind(), K::MODIFIERS | K::ANNOTATION) {
            return true;
        }
    }
    false
}

/// End offset of the declared name, where the `: Type` hint is anchored.
fn property_name_end(property: &SyntaxNode) -> Option<TextSize> {
    let mut seen_keyword = false;
    for child in property.children_with_tokens() {
        let rowan::NodeOrToken::Token(token) = child else {
            continue;
        };
        match token.kind() {
            K::VAL_KW | K::VAR_KW => seen_keyword = true,
            K::EQ | K::COLON => break,
            kind if seen_keyword && crate::kotlin_calls::is_name_token(kind) => {
                return Some(token.text_range().end())
            }
            _ => {}
        }
    }
    None
}

/// The initializer expression: the first element after `=`.
fn property_initializer(property: &SyntaxNode) -> Option<crate::kotlin_calls::KtElement> {
    let mut seen_eq = false;
    for child in property.children_with_tokens() {
        if child.kind().is_trivia() {
            continue;
        }
        if child.kind() == K::EQ {
            seen_eq = true;
            continue;
        }
        if seen_eq {
            return Some(child);
        }
    }
    None
}
