// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code Completion Provider
//!
//! This module implements code completion functionality for the Hyperion LSP.
//! It provides context-aware suggestions including keywords, symbols from the
//! current file, and other completion items.

use crate::completion::context::{CompletionContext, ContextAnalyzer};
use crate::completion::generators::{
    CompletionGenerator, CompletionGeneratorContext, ConstructorCompletionGenerator,
    KotlinMemberAccessCompletionGenerator, LocalVariableCompletionGenerator,
    MemberAccessCompletionGenerator, SymbolCompletionGenerator, WorkspaceTypeCompletionGenerator,
};
use crate::completion::gradle_completion;
use crate::completion::java_completion;
use crate::completion::kotlin_completion;
use crate::completion::ranker::{CompletionRanker, RankingContext};
use crate::completion::scala_completion;
use crate::completion::templates;
use crate::completion::utils;
use crate::groovy_support;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{
    Database, FileId, Language, ParseDatabase, SemanticDatabase, SourceDatabase,
};
use hyperion_semantics::symbol::Type;
use lru::LruCache;
use parking_lot::Mutex;
use rowan::TextSize;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, Position};
use tower_lsp::CancellationToken;
use tracing::{debug, warn};

use crate::semantic_host::ReadTxn;

/// Annotations offered right after a bare `@`.
fn common_annotation_items(index: &hyperion_workspace::WorkspaceIndex) -> Vec<CompletionItem> {
    use crate::completion::type_search::{search_types, TypeCandidate};
    [
        "Override",
        "Deprecated",
        "FunctionalInterface",
        "SuppressWarnings",
        "SafeVarargs",
    ]
    .iter()
    .flat_map(|name| search_types(index, None, name, 8))
    .filter(|candidate: &TypeCandidate| {
        candidate.is_annotation() && candidate.package() == "java.lang"
    })
    .map(|candidate| crate::completion::generators::workspace_type::type_item(&candidate))
    .collect()
}

/// Provides code completion suggestions
pub struct CompletionProvider {
    workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,
    ranker: CompletionRanker,
    context_analyzer: ContextAnalyzer,
    /// Cache for var type inference results (file_id, offset) -> Type
    /// Uses LRU eviction to bound memory (max 1000 entries)
    /// Cleared on file changes to maintain consistency
    var_type_cache: Arc<Mutex<LruCache<(FileId, u32), Type>>>,
    /// Whether the client declared `completionItem.snippetSupport`. Clients
    /// that did not (or that never say) get plain insert text.
    snippet_support: AtomicBool,
}

impl CompletionProvider {
    /// Creates a new CompletionProvider
    pub fn new(workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>) -> Self {
        let ranker = CompletionRanker::new(workspace_index.clone());
        let context_analyzer = ContextAnalyzer::new();
        Self {
            workspace_index,
            ranker,
            context_analyzer,
            var_type_cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(1000).unwrap()))),
            snippet_support: AtomicBool::new(true),
        }
    }

    /// Record whether the client can expand snippet insert text.
    pub fn set_snippet_support(&self, supported: bool) {
        self.snippet_support.store(supported, Ordering::Relaxed);
    }

    /// Whether snippet insert text may be sent.
    pub fn snippet_support(&self) -> bool {
        self.snippet_support.load(Ordering::Relaxed)
    }

    /// Generate completion items for a position in a document
    ///
    /// # Arguments
    ///
    /// * `file_id` - The FileId of the document
    /// * `position` - The cursor position in the document
    ///
    /// # Returns
    ///
    /// A vector of completion items
    pub fn complete(
        &self,
        txn: &ReadTxn,
        file_id: FileId,
        position: Position,
    ) -> Result<Vec<CompletionItem>, String> {
        self.complete_cancellable(txn, file_id, position, &CancellationToken::default())
    }

    /// Generate completion items while cooperatively observing LSP request cancellation.
    pub fn complete_cancellable(
        &self,
        txn: &ReadTxn,
        file_id: FileId,
        position: Position,
        cancellation: &CancellationToken,
    ) -> Result<Vec<CompletionItem>, String> {
        let mut items = self.complete_items(txn, file_id, position, cancellation)?;
        if !self.snippet_support() {
            super::snippet::downgrade_snippets(&mut items);
        }
        Ok(items)
    }

    fn complete_items(
        &self,
        txn: &ReadTxn,
        file_id: FileId,
        position: Position,
        cancellation: &CancellationToken,
    ) -> Result<Vec<CompletionItem>, String> {
        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        debug!(
            "Computing completions for file {:?} at position {:?}",
            file_id, position
        );

        // Check workspace index status (non-blocking)
        // If index isn't ready, we still return local completions - graceful degradation
        // is better UX than blocking the async runtime with std::thread::sleep()
        let stats = self.workspace_index.stats();
        if stats.symbols_indexed == 0 {
            debug!("Workspace index not yet populated - cross-file completions may be limited");
        }

        // One snapshot for the entire completion: the request's read
        // transaction is the only way to reach the semantic database.
        let snapshot = txn.snapshot();

        // Get source text and language
        let source_text = snapshot.file_text(file_id);
        let language = snapshot.file_language(file_id);

        // Check if this is a Gradle file
        if let Some(file_path) = self.workspace_index.get_file_path(file_id) {
            let path_str = file_path.to_string_lossy();
            if groovy_support::is_gradle_file(&path_str) {
                debug!("COMPLETION: Gradle file detected, using Gradle completion");
                return self.complete_gradle(position, &source_text);
            }
        }

        // Check for postfix completion first (highest priority)
        use crate::position_utils::position_to_offset;
        let offset = position_to_offset(&source_text, position)?;

        // Early check: if cursor is in comment/string, return empty completions
        let parse_tree = snapshot.parse_file(file_id);
        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        // Check based on language
        match language {
            Language::Kotlin => {
                let kotlin_root: hyperion_kotlin::SyntaxNode =
                    hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());
                if crate::navigation::ast_queries::is_in_comment_or_string_kotlin(
                    &kotlin_root,
                    offset as u32,
                ) {
                    debug!("COMPLETION: Cursor in comment/string, skipping completions");
                    return Ok(vec![]);
                }
            }
            Language::Scala => {
                if crate::navigation::ast_queries::is_in_comment_or_string_java(
                    &parse_tree.syntax(),
                    offset as u32,
                ) {
                    debug!("COMPLETION: Cursor in comment/string, skipping completions");
                    return Ok(vec![]);
                }
            }
            Language::Java | Language::Groovy => {
                if crate::navigation::ast_queries::is_in_comment_or_string_java(
                    &parse_tree.syntax(),
                    offset as u32,
                ) {
                    debug!("COMPLETION: Cursor in comment/string, skipping completions");
                    return Ok(vec![]);
                }
            }
        }

        let text_before_cursor = &source_text[..offset];

        if matches!(language, Language::Java | Language::Groovy) {
            if let Some(position_in_import) =
                crate::completion::import_completion::detect(text_before_cursor)
            {
                return Ok(crate::completion::import_completion::complete(
                    &self.workspace_index,
                    &position_in_import,
                ));
            }
        }

        if let Some((expression, template_prefix)) =
            crate::completion::match_postfix_pattern(text_before_cursor)
        {
            debug!(
                "Postfix completion detected: expr='{}', prefix='{}'",
                expression, template_prefix
            );

            // Templates are offered without the receiver type, so type-dependent templates are not filtered.
            let mut postfix_items =
                crate::completion::get_postfix_completions(&expression, &template_prefix, None);

            if !postfix_items.is_empty() {
                // Calculate the range to replace (expression + dot + prefix)
                let replace_start = offset - template_prefix.len() - 1 - expression.len();
                let start_pos =
                    crate::position_utils::offset_to_position(&source_text, replace_start);
                let end_pos = position;

                let replace_range = tower_lsp::lsp_types::Range {
                    start: start_pos,
                    end: end_pos,
                };

                // Set text_edit on each completion item
                for item in &mut postfix_items {
                    if let Some(ref insert_text) = item.insert_text {
                        item.text_edit = Some(tower_lsp::lsp_types::CompletionTextEdit::Edit(
                            tower_lsp::lsp_types::TextEdit {
                                range: replace_range,
                                new_text: insert_text.clone(),
                            },
                        ));
                    }
                }

                debug!("Returning {} postfix completion items", postfix_items.len());
                return Ok(postfix_items);
            }
        }

        // Analyze completion context using dedicated analyzer
        let mut context =
            self.context_analyzer
                .analyze_context(file_id, &source_text, position, snapshot)?;
        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        if !matches!(context, CompletionContext::MemberAccess) {
            if let Ok(offset) = position_to_offset(&source_text, position) {
                if is_member_access_prefix_text(&source_text, offset) {
                    context = CompletionContext::MemberAccess;
                } else {
                    let tree = snapshot.parse_file(file_id);
                    if is_member_access_token(&tree.syntax(), offset as u32) {
                        context = CompletionContext::MemberAccess;
                    }
                }
            }
        }
        debug!("Completion context: {:?}", context);

        let mut completions = Vec::new();

        // Context-aware filtering
        match &context {
            CompletionContext::MemberAccess => {
                // After '.', only show members (no keywords)
                debug!("Member access context detected, excluding keywords");
                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);
                let member_completions = match language {
                    Language::Kotlin => {
                        self.generate_with(&KotlinMemberAccessCompletionGenerator, &ctx)
                    }
                    Language::Java | Language::Groovy | Language::Scala => {
                        self.generate_with(&MemberAccessCompletionGenerator::new(), &ctx)
                    }
                };
                match member_completions {
                    Ok(member_completions) => {
                        debug!("Found {} member completions", member_completions.len());
                        completions.extend(member_completions);
                    }
                    Err(e) => {
                        debug!("Member access completion returned no results: {}", e);
                    }
                }
            }
            CompletionContext::ConstructorCall => {
                // After 'new', show instantiable types (classes, enums)
                debug!("CONSTRUCTOR CALL CONTEXT DETECTED!");
                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);
                let symbol_gen = SymbolCompletionGenerator::new();
                match self.generate_with(&symbol_gen, &ctx) {
                    Ok(symbol_completions) => {
                        // Filter to only instantiable types (classes, enums - not interfaces/abstracts)
                        let type_symbols: Vec<_> = symbol_completions
                            .into_iter()
                            .filter(|item| {
                                matches!(
                                    item.kind,
                                    Some(CompletionItemKind::CLASS | CompletionItemKind::ENUM)
                                )
                            })
                            .collect();
                        completions.extend(type_symbols);
                    }
                    Err(e) => warn!("Failed to get symbol completions: {}", e),
                }

                // Also add workspace-wide constructor completions
                let ctor_gen = ConstructorCompletionGenerator::new();
                match self.generate_with(&ctor_gen, &ctx) {
                    Ok(constructor_types) => {
                        debug!("Found {} constructor completions", constructor_types.len());
                        completions.extend(constructor_types);
                    }
                    Err(e) => {
                        debug!("Failed to get constructor completions: {}", e);
                    }
                }
            }
            CompletionContext::TypePosition => {
                // In type position, show types and type-related keywords
                debug!("Type position context detected");
                match language {
                    Language::Kotlin => {
                        completions.extend(kotlin_completion::type_keyword_completions());
                    }
                    Language::Scala => {
                        completions.extend(scala_completion::type_keyword_completions());
                    }
                    Language::Java | Language::Groovy => {
                        completions.extend(java_completion::type_keyword_completions());
                    }
                }
                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);
                let symbol_gen = SymbolCompletionGenerator::new();
                match self.generate_with(&symbol_gen, &ctx) {
                    Ok(symbol_completions) => {
                        // Filter to only type symbols (classes, interfaces, enums)
                        let type_symbols: Vec<_> = symbol_completions
                            .into_iter()
                            .filter(|item| {
                                matches!(
                                    item.kind,
                                    Some(
                                        CompletionItemKind::CLASS
                                            | CompletionItemKind::INTERFACE
                                            | CompletionItemKind::ENUM
                                    )
                                )
                            })
                            .collect();
                        completions.extend(type_symbols);
                    }
                    Err(e) => warn!("Failed to get symbol completions: {}", e),
                }

                // Also add workspace-wide type completions
                let ws_type_gen = WorkspaceTypeCompletionGenerator::new();
                match self.generate_with(&ws_type_gen, &ctx) {
                    Ok(workspace_types) => {
                        debug!("Found {} workspace type completions", workspace_types.len());
                        completions.extend(workspace_types);
                    }
                    Err(e) => {
                        debug!("Failed to get workspace type completions: {}", e);
                    }
                }
            }
            CompletionContext::GenericTypeArgument => {
                // Inside generic type arguments (List<|>, Map<String, |>)
                // Show types plus wildcard completions
                debug!("Generic type argument context detected");

                // Add wildcard completions first
                completions.extend(java_completion::wildcard_type_completions());

                // Add type symbols (classes, interfaces, enums)
                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);
                let symbol_gen = SymbolCompletionGenerator::new();
                match self.generate_with(&symbol_gen, &ctx) {
                    Ok(symbol_completions) => {
                        let type_symbols: Vec<_> = symbol_completions
                            .into_iter()
                            .filter(|item| {
                                matches!(
                                    item.kind,
                                    Some(
                                        CompletionItemKind::CLASS
                                            | CompletionItemKind::INTERFACE
                                            | CompletionItemKind::ENUM
                                    )
                                )
                            })
                            .collect();
                        completions.extend(type_symbols);
                    }
                    Err(e) => warn!("Failed to get symbol completions: {}", e),
                }

                // Also add workspace-wide type completions
                let ws_type_gen = WorkspaceTypeCompletionGenerator::new();
                match self.generate_with(&ws_type_gen, &ctx) {
                    Ok(workspace_types) => {
                        debug!("Found {} workspace type completions", workspace_types.len());
                        completions.extend(workspace_types);
                    }
                    Err(e) => {
                        debug!("Failed to get workspace type completions: {}", e);
                    }
                }
            }
            CompletionContext::LambdaExpression {
                interface_fqn,
                param_types,
                return_type,
            } => {
                // Lambda expression context - suggest lambda snippets
                debug!(
                    "Lambda expression context detected: {} ({} params)",
                    interface_fqn,
                    param_types.len()
                );

                let lambda_completions =
                    crate::completion::lambda_snippets::generate_lambda_completions(
                        interface_fqn,
                        param_types,
                        return_type,
                    );

                debug!("Generated {} lambda completions", lambda_completions.len());
                completions.extend(lambda_completions);

                // Also include general completions (might want a local variable, etc.)
                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);
                let local_gen = LocalVariableCompletionGenerator::new();
                match self.generate_with(&local_gen, &ctx) {
                    Ok(local_completions) => {
                        completions.extend(local_completions);
                    }
                    Err(e) => {
                        debug!("Failed to get local variable completions: {}", e);
                    }
                }
            }
            CompletionContext::OverrideImplement { class_fqn } => {
                // Override/implement method context - suggest method stubs.
                // The written supertypes only decide *whether* the context
                // applies; the candidate members come from the canonical
                // hierarchy walk, which is transitive.
                debug!("Override/implement context: class={class_fqn}");

                // Collect existing methods in the class to filter out
                let existing_methods = self.collect_existing_methods(file_id, snapshot);

                // Generate override/implement completions
                let override_completions =
                    crate::completion::override_completion::generate_override_implement_completions(
                        &self.workspace_index,
                        class_fqn,
                        &existing_methods,
                    );

                debug!(
                    "Generated {} override/implement completions",
                    override_completions.len()
                );
                completions.extend(override_completions);

                // Also include general completions (user might want to add a new method manually)
                // But don't add keywords - those would be noisy here
                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);
                let local_gen = LocalVariableCompletionGenerator::new();
                match self.generate_with(&local_gen, &ctx) {
                    Ok(local_completions) => {
                        completions.extend(local_completions);
                    }
                    Err(e) => {
                        debug!("Failed to get local variable completions: {}", e);
                    }
                }
            }
            CompletionContext::Annotation => {
                // In a class body `@` also starts an override/implement stub
                // (`@Override public String toString() {...}`).
                if let Some(CompletionContext::OverrideImplement { class_fqn }) = self
                    .context_analyzer
                    .detect_override_implement_context(file_id, &source_text, position, snapshot)
                {
                    let existing_methods = self.collect_existing_methods(file_id, snapshot);
                    completions.extend(
                        crate::completion::override_completion::generate_override_implement_completions(
                            &self.workspace_index,
                            &class_fqn,
                            &existing_methods,
                        ),
                    );
                }
                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);
                let prefix = utils::extract_identifier_prefix(&source_text, position);
                if prefix.is_empty() {
                    completions.extend(common_annotation_items(&self.workspace_index));
                } else {
                    match self.generate_with(&WorkspaceTypeCompletionGenerator::annotations(), &ctx)
                    {
                        Ok(annotations) => completions.extend(annotations),
                        Err(e) => debug!("Failed to get annotation completions: {}", e),
                    }
                }
            }
            CompletionContext::General => {
                // General context: show everything, filtered by prefix
                debug!("General context, showing all completions");

                // Extract prefix for filtering (case-insensitive)
                let prefix = utils::extract_identifier_prefix(&source_text, position);
                debug!("Filtering completions by prefix: '{}'", prefix);

                let ctx = self.create_generator_context(snapshot, file_id, position, &source_text);

                // Helper closure to filter by prefix (case-insensitive)
                let matches_prefix = |label: &str| -> bool {
                    hyperion_workspace::name_match::match_name(&prefix, label).is_some()
                };

                // Add local variables first (highest priority)
                let local_gen = LocalVariableCompletionGenerator::new();
                match self.generate_with(&local_gen, &ctx) {
                    Ok(local_completions) => {
                        let filtered: Vec<_> = local_completions
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        debug!(
                            "Found {} local variable completions (after prefix filter)",
                            filtered.len()
                        );
                        completions.extend(filtered);
                    }
                    Err(e) => {
                        debug!("Failed to get local variable completions: {}", e);
                    }
                }

                // Add language-specific keywords and snippets
                match language {
                    Language::Kotlin => {
                        let keywords: Vec<_> = kotlin_completion::keyword_completions()
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        completions.extend(keywords);
                        let snippets: Vec<_> = kotlin_completion::snippet_completions()
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        completions.extend(snippets);
                    }
                    Language::Scala => {
                        let keywords: Vec<_> = scala_completion::keyword_completions()
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        completions.extend(keywords);
                        let snippets: Vec<_> = scala_completion::snippet_completions()
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        completions.extend(snippets);
                    }
                    Language::Java | Language::Groovy => {
                        let keywords: Vec<_> = java_completion::keyword_completions()
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        completions.extend(keywords);
                        let snippets: Vec<_> = java_completion::snippet_completions()
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        completions.extend(snippets);

                        // Add live templates (IntelliJ-style snippets)
                        let template_context =
                            templates::detect_template_context(&source_text, position);
                        let template_completions =
                            templates::get_template_completions(&prefix, template_context);
                        debug!(
                            "Found {} template completions for prefix '{}' in context {:?}",
                            template_completions.len(),
                            prefix,
                            template_context
                        );
                        completions.extend(template_completions);
                    }
                }

                let symbol_gen = SymbolCompletionGenerator::new();
                match self.generate_with(&symbol_gen, &ctx) {
                    Ok(symbol_completions) => {
                        let filtered: Vec<_> = symbol_completions
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        completions.extend(filtered);
                    }
                    Err(e) => warn!("Failed to get symbol completions: {}", e),
                }

                // Add workspace types for import auto-insertion
                // Duplicates will be removed after all generators run
                let workspace_type_gen = WorkspaceTypeCompletionGenerator::new();
                match self.generate_with(&workspace_type_gen, &ctx) {
                    Ok(workspace_completions) => {
                        let filtered: Vec<_> = workspace_completions
                            .into_iter()
                            .filter(|c| matches_prefix(&c.label))
                            .collect();
                        debug!(
                            "Found {} workspace type completions (after prefix filter)",
                            filtered.len()
                        );
                        completions.extend(filtered);
                    }
                    Err(e) => {
                        warn!("Failed to get workspace type completions: {}", e);
                    }
                }
            }
        }

        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        if !matches!(
            context,
            CompletionContext::LambdaExpression { .. }
                | CompletionContext::OverrideImplement { .. }
        ) {
            super::prefix_filter::retain_matching(
                &mut completions,
                &utils::extract_identifier_prefix(&source_text, position),
            );
        }

        // Remove only semantic duplicates before scoring so repeated generator
        // output does not incur ranking and workspace lookup costs.
        super::dedup::deduplicate_completion_items(&mut completions);

        // Apply smart ranking to sort completions by relevance
        self.apply_ranking(
            &mut completions,
            &source_text,
            position,
            context,
            file_id,
            snapshot,
        );
        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        // Add import auto-insertion for type completions
        self.add_import_edits(&mut completions, file_id, &source_text);

        debug!("Returning {} completion items", completions.len());
        Ok(completions)
    }

    /// Create a generator context for completion generation
    fn create_generator_context<'a>(
        &self,
        snapshot: &'a salsa::Snapshot<Database>,
        file_id: FileId,
        position: Position,
        source_text: &'a str,
    ) -> CompletionGeneratorContext<'a> {
        CompletionGeneratorContext::new(
            snapshot,
            self.workspace_index.clone(),
            file_id,
            position,
            source_text,
            self.var_type_cache.clone(),
        )
        .with_snippets(self.snippet_support())
    }

    /// Generate completions using a specific generator
    fn generate_with<G: CompletionGenerator>(
        &self,
        generator: &G,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String> {
        generator.generate(context)
    }

    /// Analyze the context at the cursor position to determine what kind of completions to provide
    fn collect_existing_methods(
        &self,
        file_id: FileId,
        snapshot: &salsa::Snapshot<hyperion_semantics::db::Database>,
    ) -> std::collections::HashSet<String> {
        use hyperion_parser::syntax::SyntaxKind;

        let mut existing_methods = std::collections::HashSet::new();

        // Parse the file
        let root = {
            let parse_tree = snapshot.parse_file(file_id);
            parse_tree.syntax()
        };

        // Find all METHOD_DECL nodes
        for node in root.descendants() {
            if node.kind() == SyntaxKind::METHOD_DECL {
                // Extract method name
                for child in node.children_with_tokens() {
                    if child.kind() == SyntaxKind::IDENT {
                        if let Some(token) = child.as_token() {
                            existing_methods.insert(token.text().to_string());
                            break;
                        }
                    }
                }
            }
        }

        existing_methods
    }

    /// Apply smart ranking to completion items
    fn apply_ranking(
        &self,
        completions: &mut [CompletionItem],
        source_text: &str,
        position: Position,
        context: CompletionContext,
        file_id: FileId,
        snapshot: &salsa::Snapshot<hyperion_semantics::db::Database>,
    ) {
        use crate::position_utils::position_to_offset;

        // Extract prefix text (text before cursor)
        let prefix_text = if let Ok(offset) = position_to_offset(source_text, position) {
            source_text[..offset].to_string()
        } else {
            String::new()
        };

        // Build ranking context
        let mut ranking_context = RankingContext::new(prefix_text.clone());

        // Set type position flag for type-related contexts
        if context == CompletionContext::TypePosition
            || context == CompletionContext::ConstructorCall
            || context == CompletionContext::GenericTypeArgument
        {
            ranking_context = ranking_context.in_type_position();
        }

        // Try to extract receiver expression and type for member access
        if context == CompletionContext::MemberAccess {
            if let Ok(offset) = position_to_offset(source_text, position) {
                let before_cursor = &source_text[..offset];
                if let Some(dot_pos) = before_cursor.rfind('.') {
                    let before_dot = &source_text[..dot_pos];
                    let trimmed = before_dot.trim_end();

                    // Find the start of the qualified identifier chain (for builder patterns)
                    let chain_start = trimmed
                        .char_indices()
                        .rev()
                        .find(|(_, c)| {
                            !c.is_alphanumeric() && *c != '_' && *c != '.' && *c != '(' && *c != ')'
                        })
                        .map_or(0, |(i, _)| i + 1);

                    let receiver = trimmed[chain_start..].trim().to_string();

                    if !receiver.is_empty() {
                        ranking_context = ranking_context.with_receiver(receiver.clone());

                        // Builder pattern heuristic (ranking-specific, cheap check)
                        let resolved_fqn = if receiver.ends_with("builder()")
                            || receiver.ends_with(".builder()")
                        {
                            self.resolve_builder_type(&receiver)
                        } else {
                            None
                        };

                        // Fall back to the canonical generator resolution
                        let resolved_fqn = resolved_fqn.or_else(|| {
                            let generator = MemberAccessCompletionGenerator::new();
                            let ctx = self.create_generator_context(
                                snapshot,
                                file_id,
                                position,
                                source_text,
                            );
                            let tree = snapshot.parse_file(file_id);
                            let root = tree.syntax();
                            let symbol_table = snapshot.file_symbol_table(file_id);
                            let scope = symbol_table.find_scope_at_offset(offset as u32)?;
                            generator
                                .resolve_receiver_type(
                                    &ctx,
                                    &receiver,
                                    &root,
                                    offset as u32,
                                    &symbol_table,
                                    scope,
                                )
                                .ok()
                                .and_then(|rt| rt.to_fqn())
                        });

                        if let Some(receiver_type) = resolved_fqn {
                            debug!(
                                "Resolved receiver type: '{}' for builder ranking",
                                receiver_type
                            );
                            ranking_context = ranking_context.with_receiver_type(receiver_type);
                        }
                    }
                }
            }
        }

        // Smart completion: Infer expected type from context
        if let Some(expected_type) = utils::infer_expected_type(
            source_text,
            position,
            &prefix_text,
            file_id,
            snapshot,
            self.workspace_index.clone(),
        ) {
            debug!(
                "Smart completion: inferred expected type '{}'",
                expected_type
            );
            ranking_context = ranking_context.with_expected_type(expected_type);
        }

        // Apply ranking
        debug!(
            "Applying smart ranking to {} completions",
            completions.len()
        );
        self.ranker.rank(completions, &ranking_context);
    }

    /// Resolve builder type from a receiver ending in `builder()`.
    ///
    /// This is a ranking-specific heuristic: `User.builder()` → `User.Builder`.
    fn resolve_builder_type(&self, receiver: &str) -> Option<String> {
        let class_part = receiver
            .strip_suffix(".builder()")
            .or_else(|| receiver.strip_suffix("builder()"))
            .map_or(receiver, |s| s.trim_end_matches('.'));

        if class_part.is_empty() {
            return None;
        }

        let builder_fqn_dot = format!("{class_part}.Builder");
        let builder_fqn_dollar = format!("{class_part}$Builder");

        if self.workspace_index.get_symbol(&builder_fqn_dot).is_some() {
            return Some(builder_fqn_dot);
        }
        if self
            .workspace_index
            .get_symbol(&builder_fqn_dollar)
            .is_some()
        {
            return Some(builder_fqn_dollar);
        }

        None
    }

    /// Add import auto-insertion to completion items for workspace types
    ///
    /// For each CLASS/INTERFACE/ENUM completion item from the workspace,
    /// check if it needs an import and add the TextEdit to additional_text_edits
    fn add_import_edits(
        &self,
        completions: &mut [CompletionItem],
        file_id: FileId,
        source_text: &str,
    ) {
        utils::add_import_edits(
            completions,
            file_id,
            source_text,
            self.workspace_index.clone(),
        );
    }

    /// Generate Gradle completion items for a position in a Gradle file
    fn complete_gradle(
        &self,
        position: Position,
        source_text: &str,
    ) -> Result<Vec<CompletionItem>, String> {
        use crate::position_utils::position_to_offset;

        debug!(
            "COMPLETION: Generating Gradle completions at position {:?}",
            position
        );

        let offset = position_to_offset(source_text, position)?;
        let text_before_cursor = &source_text[..offset];

        // Determine the context and generate completions
        let context = gradle_completion::analyze_context(text_before_cursor);
        let completions = gradle_completion::completions_for_context(context);

        debug!(
            "COMPLETION: Generated {} Gradle completions",
            completions.len()
        );
        Ok(completions)
    }

    /// Get a reference to the var type cache (for file watcher access)
    pub const fn var_type_cache(&self) -> &Arc<Mutex<LruCache<(FileId, u32), Type>>> {
        &self.var_type_cache
    }

    /// Clear the var type inference cache for a specific file
    ///
    /// Should be called when a file is modified to ensure cache consistency.
    /// Removes all cached entries for the given file_id.
    pub fn invalidate_var_cache(&self, file_id: FileId) {
        let mut cache = self.var_type_cache.lock();
        // Collect keys to remove (can't remove while iterating LruCache)
        let keys_to_remove: Vec<_> = cache
            .iter()
            .filter(|((cached_file_id, _), _)| *cached_file_id == file_id)
            .map(|(key, _)| *key)
            .collect();
        for key in &keys_to_remove {
            cache.pop(key);
        }
        debug!(
            "Invalidated var type cache for file {:?}: removed {} entries, {} remaining",
            file_id,
            keys_to_remove.len(),
            cache.len()
        );
    }

    /// Clear the entire var type inference cache
    ///
    /// Should be called on workspace-wide changes (e.g., switching branches)
    pub fn clear_var_cache(&self) {
        let mut cache = self.var_type_cache.lock();
        let count = cache.len();
        cache.clear();
        debug!("Cleared entire var type cache ({} entries)", count);
    }
}

fn is_member_access_token(root: &SyntaxNode, offset: u32) -> bool {
    let token_at = root.token_at_offset(TextSize::from(offset));
    let token = token_at
        .clone()
        .right_biased()
        .or_else(|| token_at.left_biased());

    let mut current = token.and_then(|tok| tok.parent());

    while let Some(node) = current {
        if node.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
            return true;
        }
        current = node.parent();
    }

    false
}

fn is_member_access_prefix_text(source_text: &str, offset: usize) -> bool {
    let before_cursor = match source_text.get(..offset) {
        Some(text) => text,
        None => return false,
    };

    let line_start = before_cursor.rfind('\n').map_or(0, |i| i + 1);
    let line = &before_cursor[line_start..];
    let Some(dot_pos) = line.rfind('.') else {
        return false;
    };

    let after_dot = &line[dot_pos + 1..];
    let after_trim = after_dot.trim_start();
    if after_trim.is_empty() {
        return true;
    }

    after_trim.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use hyperion_semantics::db::{FileId, Language, SourceDatabase};
    use hyperion_workspace::index::WorkspaceIndex;
    use tower_lsp::lsp_types::InsertTextFormat;

    fn create_test_provider() -> (CompletionProvider, Arc<SemanticHost>) {
        let temp_dir = tempfile::tempdir().unwrap();
        let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
        let workspace_index = Arc::new(index);
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        (CompletionProvider::new(workspace_index), host)
    }

    #[test]
    fn test_keyword_completions() {
        use crate::completion::java_completion;

        let keywords = java_completion::keyword_completions();

        // Should have a reasonable number of keywords
        assert!(keywords.len() > 30);

        // Check that "class" is present
        assert!(keywords.iter().any(|c| c.label == "class"));

        // Check that all keywords have the KEYWORD kind
        assert!(keywords
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::KEYWORD)));
    }

    #[test]
    fn test_symbol_completions_empty() {
        let (provider, host) = create_test_provider();
        let file_id = FileId::new(0);
        let source_text = String::new();
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source_text.clone()));
            db.set_file_language(file_id, Language::Java);
        });

        // Create snapshot and generator context
        let txn = host.read_now();
        let ctx = provider.create_generator_context(
            txn.snapshot(),
            file_id,
            Position::new(0, 0),
            &source_text,
        );
        let generator = SymbolCompletionGenerator::new();
        let symbols = provider.generate_with(&generator, &ctx).unwrap();

        // Empty file should have no symbols
        assert_eq!(symbols.len(), 0);
    }

    #[test]
    fn test_symbol_completions_simple_class() {
        let (provider, host) = create_test_provider();
        let file_id = FileId::new(0);
        let source_text = "class Foo { }".to_string();
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source_text.clone()));
            db.set_file_language(file_id, Language::Java);
        });

        // Create snapshot and generator context
        let txn = host.read_now();
        let ctx = provider.create_generator_context(
            txn.snapshot(),
            file_id,
            Position::new(0, 0),
            &source_text,
        );
        let generator = SymbolCompletionGenerator::new();
        let symbols = provider.generate_with(&generator, &ctx).unwrap();

        // Should have the class symbol
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].label, "Foo");
        assert_eq!(symbols[0].kind, Some(CompletionItemKind::CLASS));
    }

    #[test]
    fn test_context_aware_member_access() {
        let (provider, host) = create_test_provider();
        let file_id = FileId::new(0);
        let source = "class Foo { }\nobj.";
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        // Position after the '.'
        let completions = provider
            .complete(&host.read_now(), file_id, Position::new(1, 4))
            .unwrap();

        // Should not include keywords after '.'
        assert!(!completions.iter().any(|c| c.label == "class"));
        assert!(!completions.iter().any(|c| c.label == "if"));
    }

    #[test]
    fn test_context_aware_type_position() {
        let (provider, host) = create_test_provider();
        let file_id = FileId::new(0);
        let source = "class Foo { }\npublic ";
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        // Position after 'public '
        let completions = provider
            .complete(&host.read_now(), file_id, Position::new(1, 7))
            .unwrap();

        // Should include type keywords
        assert!(completions.iter().any(|c| c.label == "class"));
        assert!(completions.iter().any(|c| c.label == "int"));

        // Should not include control flow keywords
        assert!(!completions.iter().any(|c| c.label == "if"));
        assert!(!completions.iter().any(|c| c.label == "while"));
    }

    #[test]
    fn test_context_aware_general() {
        let (provider, host) = create_test_provider();
        let file_id = FileId::new(0);
        let source = "class Foo { void bar() { ";
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        // Position in method body
        let completions = provider
            .complete(&host.read_now(), file_id, Position::new(0, 25))
            .unwrap();

        // Should include all keywords in general context
        assert!(completions.iter().any(|c| c.label == "if"));
        assert!(completions.iter().any(|c| c.label == "while"));
        assert!(completions.iter().any(|c| c.label == "return"));
    }

    #[test]
    fn test_type_keyword_completions() {
        use crate::completion::java_completion;

        let type_keywords = java_completion::type_keyword_completions();

        // Should have type-related keywords
        assert!(type_keywords.iter().any(|c| c.label == "class"));
        assert!(type_keywords.iter().any(|c| c.label == "int"));
        assert!(type_keywords.iter().any(|c| c.label == "void"));

        // Should not have control flow keywords
        assert!(!type_keywords.iter().any(|c| c.label == "if"));
        assert!(!type_keywords.iter().any(|c| c.label == "while"));
    }

    #[test]
    fn test_snippet_completions() {
        use crate::completion::java_completion;

        let snippets = java_completion::snippet_completions();

        // Should have common Java snippets
        assert!(snippets.iter().any(|c| c.label == "main"));
        assert!(snippets.iter().any(|c| c.label == "for"));
        assert!(snippets.iter().any(|c| c.label == "sysout"));
        assert!(snippets.iter().any(|c| c.label == "try"));
        assert!(snippets.iter().any(|c| c.label == "class"));

        // All should be SNIPPET kind
        assert!(snippets
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::SNIPPET)));

        // All should have SNIPPET insert format
        assert!(snippets
            .iter()
            .all(|c| c.insert_text_format == Some(InsertTextFormat::SNIPPET)));

        // Check that main snippet has correct content
        let main_snippet = snippets.iter().find(|c| c.label == "main").unwrap();
        assert!(main_snippet
            .insert_text
            .as_ref()
            .unwrap()
            .contains("public static void main"));
        assert!(main_snippet
            .insert_text
            .as_ref()
            .unwrap()
            .contains("String[] args"));
    }

    #[test]
    fn test_snippets_in_general_context() {
        let (provider, host) = create_test_provider();
        let file_id = FileId::new(0);
        let source = "class Foo { void bar() { ";
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        // Position in method body (general context)
        let completions = provider
            .complete(&host.read_now(), file_id, Position::new(0, 25))
            .unwrap();

        // Should include snippets in general context
        assert!(completions.iter().any(|c| c.label == "for"));
        assert!(completions.iter().any(|c| c.label == "if"));
        assert!(completions.iter().any(|c| c.label == "sysout"));
    }

    #[test]
    fn test_ranking_applied() {
        let (provider, host) = create_test_provider();
        let file_id = FileId::new(0);
        let source = "class Foo { void bar() { ";
        host.with_db(|db| {
            db.set_file_text(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, Language::Java);
        });

        // Position in method body
        let completions = provider
            .complete(&host.read_now(), file_id, Position::new(0, 25))
            .unwrap();

        // All completions should have sort_text set after ranking
        assert!(completions.iter().all(|c| c.sort_text.is_some()));

        // Verify ranking format is applied (should be 5-digit score + underscore + label)
        let if_completion = completions.iter().find(|c| c.label == "if");
        if let Some(if_item) = if_completion {
            let sort_text = if_item.sort_text.as_ref().unwrap();
            // Ranker format is "{:05}_{label}" where the first part is the score
            assert!(
                sort_text.contains('_'),
                "sort_text should contain underscore separator"
            );
            assert!(sort_text.ends_with("_if"), "sort_text should end with _if");
        }
    }
}
