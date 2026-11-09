// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_semantics::db::{Database, FileId, ParseDatabase, SemanticDatabase, SourceDatabase};
use hyperion_semantics::nullability::{extract_method_info, NullabilityAnalysis};
use hyperion_workspace::Language;

/// Analyze the single method named `test` through the same Salsa query used by
/// diagnostics in the language server. Fixtures may include supporting methods.
pub fn analyze_single_method(source: &str) -> NullabilityAnalysis {
    let mut db = Database::new();
    let file = FileId::new(0);
    db.set_file_text(file, Arc::new(source.to_owned()));
    db.set_file_language(file, Language::Java);

    let parse_tree = db.parse_file(file);
    assert!(
        parse_tree.errors.is_empty(),
        "nullability fixture must parse cleanly: {:?}",
        parse_tree.errors
    );

    let test_method_offsets: Vec<u32> = parse_tree
        .syntax()
        .descendants()
        .filter_map(|node| {
            let info = extract_method_info(&node)?;
            (info.name == "test").then(|| node.text_range().start().into())
        })
        .collect();
    assert_eq!(
        test_method_offsets.len(),
        1,
        "nullability fixture must contain exactly one method named `test`"
    );

    let file_analysis = db.file_nullability(file);
    file_analysis
        .methods
        .get(&test_method_offsets[0])
        .expect("production nullability query did not analyze the `test` method")
        .clone()
}
