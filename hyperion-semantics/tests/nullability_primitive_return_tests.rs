// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_parser::{lexer::Lexer, parser::Parser};
use hyperion_semantics::db::{Database, FileId, SemanticDatabase, SourceDatabase};
use hyperion_semantics::nullability::{extract_method_info, DiagnosticKind, Nullability};
use hyperion_workspace::Language;

#[test]
fn test_primitive_return_skips_null_return_warning() {
    let mut db = Database::new();
    let file = FileId::new(1);

    db.set_file_text(
        file,
        Arc::new(
            r#"
            class Test {
                private static boolean matcher(String allowedHost, String lowerCaseHost) {
                    return lowerCaseHost.endsWith(allowedHost.substring(1));
                }
            }
            "#
            .to_string(),
        ),
    );
    db.set_file_language(file, Language::Java);

    let analysis = db.file_nullability(file);
    let null_return_count = analysis
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::NullReturnFromNonNull)
        .count();

    assert_eq!(
        null_return_count, 0,
        "Primitive return methods should not warn about null returns"
    );
}

#[test]
fn test_primitive_param_does_not_mark_return_nonnull() {
    let code = r#"
        class Test {
            String value(int count) {
                return "ok";
            }
        }
    "#;

    let tokens: Vec<_> = Lexer::new(code).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    let root = SyntaxNode::new_root(green_node);

    let method_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::METHOD_DECL)
        .expect("Expected a method declaration");
    let info = extract_method_info(&method_node).expect("Expected method info");

    assert_eq!(info.return_nullability, Nullability::Unknown);
    assert!(!info.return_is_primitive);
}
