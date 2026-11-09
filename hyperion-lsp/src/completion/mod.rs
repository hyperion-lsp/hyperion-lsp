// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code Completion Module
//!
//! This module contains all code completion functionality:
//! - Completion provider (main API)
//! - Generators (specialized completion generation)
//! - Ranking (intelligent sorting by relevance)
//! - Fuzzy matching (camelCase abbreviation, substring)
//! - Postfix completions (.for, .if, .null, etc.)
//! - Live templates (sout, psvm, fori, etc.)
//! - Kotlin-specific completions (keywords, snippets)
//! - Gradle-specific completions (dependencies, repositories, plugins)
//! - Override/implement method completions
//! - Chain snippets (Stream, Optional, CompletableFuture operations)

pub mod chain_snippets;
pub mod context;
mod dedup;
pub mod functional_interface;
pub mod fuzzy;
pub mod generators;
pub mod gradle_completion;
pub mod import_completion;
pub mod java_completion;
pub mod kotlin_completion;
pub mod lambda_snippets;
pub mod metadata;
pub mod override_completion;
pub mod postfix;
mod prefix_filter;
pub mod provider;
pub mod ranker;
pub mod scala_completion;
pub mod snippet;
pub mod templates;
pub mod type_search;
pub mod type_utils;
pub mod utils;

pub use fuzzy::fuzzy_match;
pub use metadata::CompletionItemMetadata;
pub use postfix::{get_postfix_completions, match_postfix_pattern};
pub use provider::CompletionProvider;
pub use ranker::{CompletionRanker, RankingContext};
pub use templates::{get_template_completions, TemplateContext};
