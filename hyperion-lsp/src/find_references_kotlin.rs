// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin-specific find-references implementation

use crate::navigation::NavigationProvider;
use crate::navigation_util::{offset_to_position, position_to_offset};
use crate::semantic_host::ReadTxn;
use hyperion_semantics::db::{FileId, ParseDatabase};
use hyperion_workspace::{CurrentReferences, QueryEngine, WorkspaceIndex};
use rowan::{NodeOrToken, TextRange, TextSize};
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Position, Range, Url};
use tracing::debug;

/// Free function for Kotlin find-references, callable without a NavigationProvider.
///
/// Takes explicit params instead of `&self` so it can be used from both
/// `NavigationProvider::find_references_kotlin` and `KotlinHandler::find_references`.
pub(crate) fn kotlin_find_references(
    txn: &ReadTxn,
    workspace_index: &Arc<WorkspaceIndex>,
    query_engine: &Arc<QueryEngine>,
    source_providers: &Arc<crate::source_provider::SourceProviderChain>,
    file_id: FileId,
    position: Position,
    source_text: &str,
    uri: &Url,
    include_declaration: bool,
) -> Result<Vec<Location>, String> {
    debug!(
        "REFS: Kotlin find-references for {:?} at {:?}",
        file_id, position
    );

    // Parse the Kotlin file. try_lock_for: degrade to empty results rather
    // than freeze if did_change is currently holding the DB mutex.
    let parse_tree = {
        let snapshot = txn.snapshot();
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
        "REFS: Kotlin token: {:?} (kind: {:?})",
        token.text(),
        token.kind()
    );

    use hyperion_kotlin::SyntaxKind as KtKind;
    let template_target = matches!(
        token.kind(),
        KtKind::STRING_LITERAL | KtKind::RAW_STRING_LITERAL
    )
    .then(|| {
        template_identifiers(&token).into_iter().find(|(_, range)| {
            range.start() <= TextSize::from(offset) && TextSize::from(offset) <= range.end()
        })
    })
    .flatten();

    if crate::navigation::ast_queries::is_in_comment_or_string_kotlin(&kotlin_root, offset)
        && template_target.is_none()
    {
        debug!("REFS: Cursor in comment/non-template string, skipping");
        return Ok(vec![]);
    }

    if token.kind() != KtKind::IDENT && template_target.is_none() {
        debug!("REFS: Not an identifier, skipping");
        return Ok(vec![]);
    }

    let identifier = template_target
        .as_ref()
        .map_or_else(|| token.text().to_string(), |(name, _)| name.clone());
    let target_range = template_target
        .as_ref()
        .map_or_else(|| token.text_range(), |(_, range)| *range);
    debug!("REFS: Finding references to: {}", identifier);

    let navigation_provider = NavigationProvider::new(
        txn,
        Arc::clone(workspace_index),
        Arc::clone(query_engine),
        Arc::clone(source_providers),
    );
    let resolved_workspace_target = navigation_provider.resolve_workspace_target_at_position(
        file_id,
        position,
        source_text,
        uri,
        None,
    )?;

    // Kotlin's scope graph resolves by name, so overloads need an additional
    // signature pass. Restrict this to genuinely overloaded functions and
    // retain ambiguous calls when their argument types cannot be inferred.
    let overloads = function_signatures(&kotlin_root, &identifier);
    let mut target_signature = if overloads.len() > 1 {
        overloads
            .iter()
            .find(|signature| signature.name_range == target_range)
            .cloned()
            .or_else(|| {
                (template_target.is_none())
                    .then(|| containing_callee_call(&token, &identifier))
                    .flatten()
                    .and_then(|call| {
                        let matches = overloads
                            .iter()
                            .filter(|signature| signature_matches_call(signature, &call))
                            .cloned()
                            .collect::<Vec<_>>();
                        (matches.len() == 1).then(|| matches[0].clone())
                    })
            })
    } else {
        None
    };
    let mut target_signature_file_id = target_signature.as_ref().map(|_| file_id);
    let mut target_signature_revision = target_signature.as_ref().and_then(|_| {
        let revision = workspace_index.current_file_revision(file_id)?;
        let indexed = workspace_index.get_indexed_revision_content(revision)?;
        (indexed.content.as_str() == source_text).then_some(revision)
    });

    // Collect references from multiple sources
    let mut all_references = Vec::new();
    let mut workspace_reference_proofs: Vec<CurrentReferences> = Vec::new();

    // 1. Try workspace-wide references (highest quality if FQN resolves)
    let target_identity = resolved_workspace_target
        .as_ref()
        .map(|resolved| resolved.definition.target.clone());

    // If we found the symbol, search for all workspace references
    if let Some(target) = target_identity {
        let fqn = target.fqn.to_string();
        debug!("REFS: Searching workspace for references to '{}'", fqn);

        // A call site in another file cannot see the declaration CST, while
        // Kotlin overloads still share one indexed base FQN. Recover every
        // declaration file contributing that FQN (overloads can themselves be
        // split across files), then select the declaration or matching call.
        if target_signature.is_none() && template_target.is_none() {
            let workspace_overloads =
                workspace_function_signatures(workspace_index, &fqn, &identifier);
            if workspace_overloads.len() > 1 {
                let selected = workspace_overloads
                    .iter()
                    .find(|(candidate_file_id, _, signature)| {
                        *candidate_file_id == file_id && signature.name_range == target_range
                    })
                    .cloned()
                    .or_else(|| {
                        let call = containing_callee_call(&token, &identifier)?;
                        let matches = workspace_overloads
                            .iter()
                            .filter(|(_, _, signature)| signature_matches_call(signature, &call))
                            .cloned()
                            .collect::<Vec<_>>();
                        (matches.len() == 1).then(|| matches[0].clone())
                    });
                if let Some((signature_file_id, signature_revision, signature)) = selected {
                    target_signature = Some(signature);
                    target_signature_file_id = Some(signature_file_id);
                    target_signature_revision = Some(signature_revision);
                }
            }
        }

        // A Kotlin declaration is also what its Java callers see through the
        // JVM view (`getNote()`, `SquareKt.f()`): search every alias too.
        let mut family_proofs: Vec<CurrentReferences> = Vec::new();
        let mut family_locations = Vec::new();
        for member in workspace_index.jvm_reference_family(&target) {
            if member.fqn == target.fqn && member.namespace == target.namespace {
                continue;
            }
            if let Ok(current) = query_engine.find_current_references_target(&member) {
                if !current.is_empty() {
                    family_locations.extend(current.locations().cloned());
                    family_proofs.push(current);
                }
            }
        }

        match query_engine.find_current_references_target(&target) {
            Ok(current) if !current.is_empty() || !family_locations.is_empty() => {
                debug!("REFS: Found {} workspace references", current.len());
                let mut locations = current.locations().cloned().collect::<Vec<_>>();
                locations.extend(family_locations);
                workspace_index.record_symbol_usage(&fqn);
                // The Kotlin workspace index historically stores top-level
                // function overloads under a shared base FQN. Its same-file
                // entries are therefore ambiguous; the CST pass below is
                // signature-aware and is authoritative for this file.
                if target_signature.is_some() {
                    locations.retain(|location| location.file_id != file_id);
                }
                if let Some(signature) = &target_signature {
                    let mut parsed_files = std::collections::HashMap::new();
                    locations.retain(|location| {
                        overload_location_matches(
                            workspace_index,
                            location,
                            &identifier,
                            signature,
                            &mut parsed_files,
                        )
                    });
                }
                if !include_declaration {
                    if let Some(definition) = workspace_index.find_symbol_definition(&target) {
                        locations.retain(|location| {
                            location.file_id != definition.location.file_id
                                || location.range != definition.location.range
                        });
                    }
                }
                all_references.extend(convert_symbol_locations_to_lsp(workspace_index, &locations));
                workspace_reference_proofs.push(current);
                workspace_reference_proofs.append(&mut family_proofs);
                debug!("REFS: Added {} workspace references", all_references.len());
            }
            Ok(_) => {
                debug!("REFS: No workspace references found for '{}'", fqn);
            }
            Err(e) => {
                debug!("REFS: Workspace query error: {:?}", e);
            }
        }
    }

    // The shared Kotlin overload FQN stores only one declaration location.
    // Add the selected declaration directly so a request from any overload's
    // cross-file call still returns the correct definition.
    if include_declaration {
        if let (Some(signature), Some(signature_file_id), Some(signature_revision)) = (
            &target_signature,
            target_signature_file_id,
            target_signature_revision,
        ) {
            if let Some(location) = text_range_location(
                workspace_index,
                signature_file_id,
                signature_revision,
                signature.name_range,
            ) {
                all_references.push(location);
            }
        }
    }

    // 2. ALWAYS add same-file references (local scope, highest priority)
    // This ensures we catch local variables, parameters, etc. that may not be indexed
    debug!("REFS: Adding same-file Kotlin references");
    let mut same_file_refs = Vec::new();

    // Build scope graph for scope-aware symbol matching
    let scope_graph = hyperion_kotlin::build_scope_graph(&kotlin_root);

    // Resolve the target symbol at the cursor position to get its definition range.
    // This lets us distinguish between different symbols with the same name (shadowing).
    let target_def_range = scope_graph
        .resolve_symbol(&identifier, offset as usize)
        .map(|sym| sym.definition_range)
        .or_else(|| {
            (template_target.is_none())
                .then(|| declaration_name_range(&token))
                .flatten()
        });
    if target_def_range.is_none() && resolved_workspace_target.is_none() {
        debug!("REFS: Kotlin identifier has no lexical or workspace identity");
        return Ok(Vec::new());
    }

    // Walk through all identifiers in the file
    for node in kotlin_root.descendants_with_tokens() {
        if let Some(tok) = node.as_token() {
            if tok.kind() == KtKind::IDENT && tok.text() == identifier {
                let range = tok.text_range();
                let token_offset = usize::from(range.start());

                if let Some(target) = &target_signature {
                    if let Some(declaration) = overloads
                        .iter()
                        .find(|signature| signature.name_range == range)
                    {
                        if declaration.parameter_types != target.parameter_types {
                            continue;
                        }
                    } else if let Some(call) = containing_callee_call(tok, &identifier) {
                        if !signature_matches_call(target, &call) {
                            continue;
                        }
                    }
                }

                // Scope-aware filtering: resolve this token's symbol and compare
                // definition ranges to ensure we only match the same logical symbol.
                if let Some(target_range) = target_def_range {
                    if let Some(resolved) = scope_graph.resolve_symbol(&identifier, token_offset) {
                        if resolved.definition_range != target_range {
                            debug!(
                                "REFS: Skipping same-name token at offset {} (different scope)",
                                token_offset
                            );
                            continue;
                        }
                    }
                    // If we can't resolve the token (e.g., it's a cross-file reference
                    // or class member), fall through — workspace refs handle those.
                }

                let start_pos = offset_to_position(source_text, usize::from(range.start()));
                let end_pos = offset_to_position(source_text, usize::from(range.end()));

                // Exclude the resolved declaration regardless of whether the
                // request started on the declaration or on one of its uses.
                if !include_declaration && target_def_range == Some(range) {
                    debug!("REFS: Skipping declaration at offset {}", token_offset);
                    continue;
                }

                same_file_refs.push(Location {
                    uri: uri.clone(),
                    range: Range {
                        start: start_pos,
                        end: end_pos,
                    },
                });
            } else if matches!(
                tok.kind(),
                KtKind::STRING_LITERAL | KtKind::RAW_STRING_LITERAL
            ) {
                for (template_name, range) in template_identifiers(tok) {
                    if template_name != identifier {
                        continue;
                    }
                    let occurrence_offset = usize::from(range.start());
                    if let Some(target_range) = target_def_range {
                        if let Some(resolved) =
                            scope_graph.resolve_symbol(&identifier, occurrence_offset)
                        {
                            if resolved.definition_range != target_range {
                                continue;
                            }
                        }
                    }

                    same_file_refs.push(Location {
                        uri: uri.clone(),
                        range: Range {
                            start: offset_to_position(source_text, occurrence_offset),
                            end: offset_to_position(source_text, usize::from(range.end())),
                        },
                    });
                }
            }
        }
    }

    debug!("REFS: Found {} same-file references", same_file_refs.len());

    // 3. Merge into the one deterministic location set expected by LSP clients.
    let final_references = crate::reference_results::merge(same_file_refs, all_references);

    debug!(
        "REFS: Returning {} total Kotlin references (after deduplication)",
        final_references.len()
    );
    if workspace_reference_proofs
        .iter()
        .all(|proof| query_engine.references_are_current(proof))
        && resolved_workspace_target
            .as_ref()
            .is_none_or(|target| navigation_provider.workspace_target_is_current(target))
    {
        Ok(final_references)
    } else {
        Ok(Vec::new())
    }
}

fn template_identifiers(token: &hyperion_kotlin::SyntaxToken) -> Vec<(String, TextRange)> {
    use hyperion_kotlin::SyntaxKind as K;

    if !matches!(token.kind(), K::STRING_LITERAL | K::RAW_STRING_LITERAL) {
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
        if token.kind() == K::STRING_LITERAL
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
        let name = String::from_utf8_lossy(&text[start..end]).into_owned();
        identifiers.push((
            name,
            TextRange::new(
                TextSize::from(token_start + start as u32),
                TextSize::from(token_start + end as u32),
            ),
        ));
        index = end;
    }

    identifiers
}

#[derive(Clone)]
struct FunctionSignature {
    name_range: TextRange,
    parameter_types: Vec<String>,
}

fn function_signatures(root: &hyperion_kotlin::SyntaxNode, name: &str) -> Vec<FunctionSignature> {
    use hyperion_kotlin::SyntaxKind as K;

    root.descendants()
        .filter(|node| node.kind() == K::FUNCTION_DECL)
        .filter_map(|node| {
            let mut seen_fun = false;
            let mut name_range = None;
            for element in node.children_with_tokens() {
                if let NodeOrToken::Token(token) = element {
                    if token.kind() == K::FUN_KW {
                        seen_fun = true;
                    } else if seen_fun
                        && matches!(token.kind(), K::IDENT | K::BACKTICK_IDENT)
                        && token.text().trim_matches('`') == name
                    {
                        name_range = Some(token.text_range());
                        break;
                    }
                }
            }

            let parameter_types = node
                .children()
                .find(|child| child.kind() == K::PARAMETER_LIST)
                .into_iter()
                .flat_map(|list| list.children())
                .filter(|parameter| parameter.kind() == K::PARAMETER)
                .map(|parameter| {
                    parameter
                        .children()
                        .find(|child| child.kind() == K::TYPE_REF)
                        .map_or_else(String::new, |ty| ty.text().to_string().trim().to_string())
                })
                .collect();

            Some(FunctionSignature {
                name_range: name_range?,
                parameter_types,
            })
        })
        .collect()
}

fn declaration_name_range(token: &hyperion_kotlin::SyntaxToken) -> Option<TextRange> {
    use hyperion_kotlin::SyntaxKind as K;

    let declaration = token.parent_ancestors().find(|node| {
        matches!(
            node.kind(),
            K::CLASS_DECL
                | K::INTERFACE_DECL
                | K::OBJECT_DECL
                | K::FUNCTION_DECL
                | K::PROPERTY_DECL
                | K::PARAMETER
                | K::TYPEALIAS_DECL
        )
    })?;

    let name = declaration.children_with_tokens().find_map(|element| {
        let candidate = element.into_token()?;
        matches!(candidate.kind(), K::IDENT | K::BACKTICK_IDENT).then_some(candidate)
    })?;

    (name.text_range() == token.text_range()).then_some(name.text_range())
}

fn containing_callee_call(
    token: &hyperion_kotlin::SyntaxToken,
    identifier: &str,
) -> Option<crate::kotlin_calls::KotlinCall> {
    use hyperion_kotlin::SyntaxKind as K;

    token.parent_ancestors().find_map(|node| {
        if node.kind() != K::CALL_EXPR {
            return None;
        }
        let call = crate::kotlin_calls::analyze_call(&node)?;
        if call.name != identifier {
            return None;
        }
        let boundary = call.l_paren.unwrap_or_else(|| node.text_range().end());
        (token.text_range().end() <= boundary).then_some(call)
    })
}

fn signature_matches_call(
    signature: &FunctionSignature,
    call: &crate::kotlin_calls::KotlinCall,
) -> bool {
    if signature.parameter_types.len() != call.arg_count() {
        return false;
    }

    call.args
        .iter()
        .zip(&signature.parameter_types)
        .all(|(argument, parameter)| {
            inferred_literal_type(&argument.value_text())
                .is_none_or(|argument_type| simple_type_name(parameter) == argument_type)
        })
}

fn kotlin_root_for_file(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
) -> Option<(
    hyperion_workspace::FileRevisionToken,
    hyperion_kotlin::SyntaxNode,
)> {
    let revision = workspace_index.current_file_revision(file_id)?;
    let root = kotlin_root_for_revision(workspace_index, revision)?;
    Some((revision, root))
}

fn kotlin_root_for_revision(
    workspace_index: &WorkspaceIndex,
    revision: hyperion_workspace::FileRevisionToken,
) -> Option<hyperion_kotlin::SyntaxNode> {
    let indexed = workspace_index.get_indexed_revision_content(revision)?;
    let tokens = hyperion_kotlin::Lexer::new(&indexed.content).collect();
    let (green, _) = hyperion_kotlin::Parser::parse(tokens);
    Some(hyperion_kotlin::SyntaxNode::new_root(green))
}

fn workspace_function_signatures(
    workspace_index: &WorkspaceIndex,
    fqn: &str,
    identifier: &str,
) -> Vec<(
    FileId,
    hyperion_workspace::FileRevisionToken,
    FunctionSignature,
)> {
    workspace_index
        .find_files_defining_symbol(fqn)
        .into_iter()
        .filter_map(|file_id| {
            kotlin_root_for_file(workspace_index, file_id)
                .map(|(revision, root)| (file_id, revision, root))
        })
        .flat_map(|(file_id, revision, root)| {
            function_signatures(&root, identifier)
                .into_iter()
                .map(move |signature| (file_id, revision, signature))
        })
        .collect()
}

fn overload_location_matches(
    workspace_index: &WorkspaceIndex,
    location: &hyperion_workspace::SymbolLocation,
    identifier: &str,
    target: &FunctionSignature,
    parsed_files: &mut std::collections::HashMap<
        hyperion_workspace::FileRevisionToken,
        hyperion_kotlin::SyntaxNode,
    >,
) -> bool {
    let Some(revisioned) = reject_unrevisioned_workspace_location(workspace_index, location) else {
        return false;
    };
    let root = if let Some(root) = parsed_files.get(&revisioned.revision) {
        root
    } else {
        let Some(root) = kotlin_root_for_revision(workspace_index, revisioned.revision) else {
            return false;
        };
        parsed_files.insert(revisioned.revision, root);
        parsed_files
            .get(&revisioned.revision)
            .expect("just inserted Kotlin root")
    };

    let range = TextRange::new(
        TextSize::from(location.range.start),
        TextSize::from(location.range.end),
    );
    if let Some(declaration) = function_signatures(root, identifier)
        .into_iter()
        .find(|signature| signature.name_range == range)
    {
        return declaration.parameter_types == target.parameter_types;
    }

    let Some(token) = root.token_at_offset(range.start()).right_biased() else {
        return false;
    };
    containing_callee_call(&token, identifier)
        .is_some_and(|call| signature_matches_call(target, &call))
}

fn text_range_location(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    revision: hyperion_workspace::FileRevisionToken,
    range: TextRange,
) -> Option<Location> {
    (revision.file_id() == file_id).then_some(())?;
    let indexed = workspace_index.get_indexed_revision_content(revision)?;
    let start = u32::try_from(usize::from(range.start())).ok()?;
    let end = u32::try_from(usize::from(range.end())).ok()?;
    Some(Location {
        uri: Url::from_file_path(&indexed.file_path).ok()?,
        range: crate::navigation_util::byte_range_to_lsp_range(&indexed.content, start, end)?,
    })
}

fn inferred_literal_type(text: &str) -> Option<&'static str> {
    let text = text.trim();
    if text.starts_with('"') {
        Some("String")
    } else if text.starts_with('\'') {
        Some("Char")
    } else if matches!(text, "true" | "false") {
        Some("Boolean")
    } else if text.parse::<i32>().is_ok() {
        Some("Int")
    } else if text
        .strip_suffix(['L', 'l'])
        .is_some_and(|number| number.parse::<i64>().is_ok())
    {
        Some("Long")
    } else if text.parse::<f64>().is_ok() {
        Some("Double")
    } else {
        None
    }
}

fn simple_type_name(parameter: &str) -> &str {
    parameter
        .trim_start_matches("vararg ")
        .trim_end_matches('?')
        .split('<')
        .next()
        .unwrap_or(parameter)
        .rsplit('.')
        .next()
        .unwrap_or(parameter)
}

/// Convert workspace symbol locations to LSP locations (Kotlin-specific).
///
/// Mirrors `NavigationProvider::convert_symbol_locations_to_lsp` but takes
/// explicit workspace_index param so it can be called without &self.
fn convert_symbol_locations_to_lsp(
    workspace_index: &WorkspaceIndex,
    locations: &[hyperion_workspace::SymbolLocation],
) -> Vec<Location> {
    let mut results = Vec::with_capacity(locations.len());

    for symbol_loc in locations {
        let Some(revisioned) = reject_unrevisioned_workspace_location(workspace_index, symbol_loc)
        else {
            continue;
        };
        let Some(indexed) = workspace_index.get_indexed_revision_content(revisioned.revision)
        else {
            continue;
        };
        let Ok(symbol_uri) = Url::from_file_path(&indexed.file_path) else {
            continue;
        };
        if !validate_indexed_reference_range(
            &indexed.content,
            revisioned.location.range.start,
            revisioned.location.range.end,
            &revisioned.location.name,
        ) {
            continue;
        }
        let Some(lsp_range) = crate::navigation_util::byte_range_to_lsp_range(
            &indexed.content,
            revisioned.location.range.start,
            revisioned.location.range.end,
        ) else {
            continue;
        };

        results.push(Location {
            uri: symbol_uri,
            range: lsp_range,
        });
    }

    results
}

fn reject_unrevisioned_workspace_location(
    workspace_index: &WorkspaceIndex,
    location: &hyperion_workspace::SymbolLocation,
) -> Option<hyperion_workspace::RevisionedLocation> {
    if let Some(revisioned) = workspace_index.revisioned_location(location) {
        return Some(revisioned);
    }
    debug!(
        "REFS: omitting unrevisioned Kotlin workspace range for '{}' until semantic publication provenance is available",
        location.fqn
    );
    None
}

fn validate_indexed_reference_range(
    content: &str,
    range_start: u32,
    range_end: u32,
    expected_name: &str,
) -> bool {
    let start = range_start as usize;
    let end = range_end as usize;
    if start > end
        || end > content.len()
        || !content.is_char_boundary(start)
        || !content.is_char_boundary(end)
    {
        return false;
    }
    let actual = &content[start..end];
    let expected_simple = expected_name.rsplit('.').next().unwrap_or(expected_name);
    let expected_identifier = expected_simple.split('(').next().unwrap_or(expected_simple);
    actual == expected_name || actual == expected_simple || actual == expected_identifier
}
