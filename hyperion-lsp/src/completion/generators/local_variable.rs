// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Local variable completion generator
//!
//! Generates completions for local variables, parameters, and fields
//! visible in the current scope.

use super::context::{CompletionGenerator, CompletionGeneratorContext};
use crate::position_utils::position_to_offset;
use hyperion_semantics::db::SemanticDatabase;
use hyperion_semantics::symbol::{PrimitiveType, SymbolKind, Type};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use tracing::debug;

/// Generator for local variable completions
pub struct LocalVariableCompletionGenerator;

impl Default for LocalVariableCompletionGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalVariableCompletionGenerator {
    pub const fn new() -> Self {
        Self
    }

    /// Convert a Type to a string for display
    fn type_to_string(typ: &Type) -> String {
        match typ {
            Type::Class(class_type) => {
                if let Some(ref pkg) = class_type.package {
                    format!("{}.{}", pkg, class_type.name)
                } else {
                    class_type.name.to_string()
                }
            }
            Type::Primitive(prim) => match prim {
                PrimitiveType::Int => "int".to_string(),
                PrimitiveType::Long => "long".to_string(),
                PrimitiveType::Short => "short".to_string(),
                PrimitiveType::Byte => "byte".to_string(),
                PrimitiveType::Char => "char".to_string(),
                PrimitiveType::Float => "float".to_string(),
                PrimitiveType::Double => "double".to_string(),
                PrimitiveType::Boolean => "boolean".to_string(),
            },
            Type::Array {
                element_type,
                dimensions,
            } => format!(
                "{}{}",
                Self::type_to_string(element_type),
                "[]".repeat(*dimensions as usize)
            ),
            Type::Void => "void".to_string(),
            Type::Null => "null".to_string(),
            Type::Unknown => "?".to_string(),
            Type::TypeVariable { name, .. } => name.clone(),
            Type::Wildcard { .. } => "?".to_string(),
            Type::Intersection { .. } => "?".to_string(),
            Type::Union { .. } => "?".to_string(),
        }
    }
}

impl CompletionGenerator for LocalVariableCompletionGenerator {
    fn generate(
        &self,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String> {
        debug!(
            "Computing local variable completions at {:?}",
            context.position
        );

        // Convert position to offset
        let offset = position_to_offset(context.source_text, context.position)?;

        // Get cached symbol table from pre-acquired snapshot
        let symbol_table = context.snapshot.file_symbol_table(context.file_id);

        // Find scope at cursor position
        let scope_id = if let Some(s) = symbol_table.find_scope_at_offset(offset as u32) {
            s
        } else {
            debug!("Could not find scope at offset {}", offset);
            return Err("Could not find scope".to_string());
        };
        let mut completions = Vec::new();
        let mut visited_scopes = std::collections::HashSet::new();

        // Walk up the scope chain collecting all local symbols
        let mut current_scope_id = Some(scope_id);
        while let Some(sid) = current_scope_id {
            if !visited_scopes.insert(sid) {
                break; // Prevent infinite loops
            }

            if let Some(scope) = symbol_table.get_scope(sid) {
                // Collect symbols from this scope
                for (name, symbol_id) in &scope.symbols {
                    if let Some(symbol) = symbol_table.get_symbol(*symbol_id) {
                        // Filter for local variables, parameters, and fields
                        let (should_include, completion_kind) = match &symbol.kind {
                            SymbolKind::LocalVariable { .. } => {
                                (true, CompletionItemKind::VARIABLE)
                            }
                            SymbolKind::Parameter { .. } => (true, CompletionItemKind::VARIABLE),
                            SymbolKind::Field { .. } => (true, CompletionItemKind::FIELD),
                            _ => (false, CompletionItemKind::VARIABLE),
                        };

                        if should_include {
                            let type_str = Self::type_to_string(&symbol.type_info);

                            // Create metadata for ranking cache
                            // Local variables don't have FQN, but we can cache type info
                            let metadata =
                                crate::completion::metadata::CompletionItemMetadata::new(name)
                                    .with_item_type(type_str.clone());

                            completions.push(CompletionItem {
                                label: name.clone(),
                                label_details: None,
                                kind: Some(completion_kind),
                                detail: Some(format!("{type_str} {name}")),
                                documentation: None,
                                preselect: None,
                                sort_text: Some(format!("0_{name}")), // Higher priority than class names
                                filter_text: None,
                                insert_text: Some(name.clone()),
                                insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
                                insert_text_mode: None,
                                text_edit: None,
                                additional_text_edits: None,
                                command: None,
                                commit_characters: None,
                                data: metadata.to_value(),
                                tags: None,
                                deprecated: None,
                            });
                        }
                    }
                }

                // Move to parent scope
                current_scope_id = scope.parent;
            } else {
                break;
            }
        }

        debug!("Found {} local variable completions", completions.len());

        // Remove duplicates (closer scope shadows outer scope, so keep first occurrence)
        let mut seen = std::collections::HashSet::new();
        completions.retain(|c| seen.insert(c.label.clone()));

        Ok(completions)
    }
}
