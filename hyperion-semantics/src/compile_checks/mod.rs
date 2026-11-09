// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! javac-style compile checks.
//!
//! The governing rule, from the product: a false positive is worse than a
//! missed error. Every check in this module therefore reports only when the
//! error is provable from a complete, resolved model, and stays silent when
//! resolution is uncertain (incomplete classpath, unresolved receiver type,
//! generic inference gaps).

pub mod abstract_impl;
pub mod assignments;
pub mod calls;
pub mod constructors;
pub mod conversion;
pub mod exceptions;
pub mod facts;
pub mod flow;
pub mod lexical;
pub mod lombok;
pub mod scopes;
pub mod switches;

use std::sync::Arc;

use hyperion_parser::SyntaxNode;
use hyperion_workspace::WorkspaceIndex;

use crate::file_semantics::FileSemantics;
use crate::semantic_error::SemanticError;

/// Every compile check that runs on a whole file (as opposed to the
/// expression-level checks owned by the type checker).
pub fn check_file(
    root: &SyntaxNode,
    file_semantics: &Arc<FileSemantics>,
    workspace_index: &WorkspaceIndex,
    package: Option<&str>,
) -> Vec<SemanticError> {
    let cache = facts::TypeFactsCache::default();
    let facts = facts::TypeFacts::new(workspace_index, &cache);
    let mut errors = Vec::new();
    errors.extend(flow::check_flow(root, file_semantics));
    errors.extend(assignments::check_final_assignments(root, file_semantics));
    errors.extend(scopes::check_scopes(root, file_semantics));
    errors.extend(switches::check_switch_exhaustiveness(
        root,
        file_semantics,
        &facts,
    ));
    errors.extend(abstract_impl::check_abstract_implementations(
        root, &facts, package,
    ));
    errors
}
