// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin signature help (`textDocument/signatureHelp`).
//!
//! Triggered on `(` and `,`, this walks the Kotlin CST to the innermost call
//! surrounding the cursor, resolves the callee, and reports every candidate
//! signature together with the parameter currently being typed.
//!
//! # What is covered
//!
//! * Top-level, member and local `fun` declarations, plus primary and
//!   secondary constructors — from the file being edited (all overloads) and
//!   from the workspace index.
//! * Named arguments: the active parameter follows the *name*, not the slot.
//! * Trailing lambdas, including the parenthesis-free `items.map { … }` form.
//! * Overloads: every candidate is returned, with `active_signature` pointing
//!   at the best fit for what has been typed so far.
//!
//! # What is not covered
//!
//! Kotlin stdlib functions. The stdlib registry in `hyperion-kotlin` stores
//! parameter *types* only ([`hyperion_kotlin::ParameterSig`] is
//! `Single(TypeExpr)` / `Vararg(TypeExpr)` — there is no name field), so a
//! stdlib signature could only ever render as `map((T) -> R)`. Emitting
//! parameter hints with no parameter names is worse than emitting none, so
//! signature help is scoped to workspace-defined and file-local callables.

use hyperion_semantics::db::{FileId, ParseDatabase, SemanticDatabase};
use hyperion_workspace::WorkspaceIndex;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    Documentation, ParameterInformation, ParameterLabel, Position, SignatureHelp,
    SignatureInformation,
};
use tracing::debug;

use crate::kotlin_calls::{
    active_parameter, best_signature, call_at_offset, KotlinCalleeResolver, KotlinSignature,
};
use crate::position_utils::position_to_offset;
use crate::semantic_host::ReadTxn;

/// Signature help for Kotlin buffers.
pub struct KotlinSignatureHelpProvider<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
}

impl<'txn> KotlinSignatureHelpProvider<'txn> {
    pub const fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self {
            txn,
            workspace_index,
        }
    }

    /// Signature help at `position`, or `None` when the cursor is not inside a
    /// call's argument list or the callee cannot be resolved.
    pub fn signature_help(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<SignatureHelp>, String> {
        let offset = position_to_offset(source_text, position)
            .map_err(|e| format!("Failed to convert position: {e}"))?;
        let offset = rowan::TextSize::from(offset as u32);

        // try_lock_for: degrade to "no signature help" rather than block the
        // editor if did_change currently holds the DB mutex.
        let (parse_tree, symbol_table) = {
            let snapshot = self.txn.snapshot();
            (
                snapshot.parse_file(file_id),
                snapshot.file_symbol_table(file_id),
            )
        };
        let root = hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());

        let Some(call) = call_at_offset(&root, offset) else {
            debug!("Kotlin signature help: no enclosing call at offset {offset:?}");
            return Ok(None);
        };

        let receiver_fqn = call.receiver.as_ref().and_then(|receiver| {
            let engine = hyperion_semantics::KotlinTypeInferenceEngine::new(
                Arc::clone(&self.workspace_index),
                file_id,
            );
            let mut resolver =
                hyperion_semantics::SymbolResolver::with_cached_table(Arc::clone(&symbol_table));
            let inferred = match receiver {
                rowan::NodeOrToken::Node(node) => engine.infer_expression(node, &mut resolver),
                rowan::NodeOrToken::Token(token) => {
                    engine.infer_token_expression(token, &mut resolver)
                }
            };
            crate::kotlin_calls::type_to_fqn(&inferred)
        });

        let resolver = KotlinCalleeResolver {
            workspace_index: &self.workspace_index,
            file_id,
            root: &root,
        };
        let signatures = resolver.resolve(&call, receiver_fqn.as_deref());

        if signatures.is_empty() {
            debug!("Kotlin signature help: no candidates for '{}'", call.name);
            return Ok(None);
        }

        let active_signature = best_signature(&signatures, &call);
        let active = active_parameter(&call, offset, &signatures[active_signature].parameters);

        debug!(
            "Kotlin signature help: {} candidate(s) for '{}', active signature {}, active parameter {}",
            signatures.len(),
            call.name,
            active_signature,
            active
        );

        Ok(Some(SignatureHelp {
            signatures: signatures.iter().map(to_signature_information).collect(),
            active_signature: Some(active_signature as u32),
            active_parameter: Some(active),
        }))
    }
}

fn to_signature_information(signature: &KotlinSignature) -> SignatureInformation {
    let parameters = signature
        .parameter_spans
        .iter()
        .map(|(start, end)| ParameterInformation {
            label: ParameterLabel::LabelOffsets([*start, *end]),
            documentation: None,
        })
        .collect();

    SignatureInformation {
        label: signature.label.clone(),
        documentation: signature
            .documentation
            .as_ref()
            .map(|doc| Documentation::String(doc.clone())),
        parameters: Some(parameters),
        active_parameter: None,
    }
}
