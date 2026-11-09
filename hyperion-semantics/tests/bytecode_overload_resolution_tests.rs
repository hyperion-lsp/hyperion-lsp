// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::process::Command;
use std::sync::Arc;

use hyperion_parser::{Lexer, Parser, SyntaxNode};
use hyperion_semantics::{
    semantic_walker::compute_file_semantics, symbol::SymbolTable, TypeChecker,
};
use hyperion_workspace::WorkspaceIndex;

#[test]
fn compiled_overloads_preserve_parameter_identity_for_diagnostics() {
    let directory = tempfile::tempdir().unwrap();
    let source_root = directory.path().join("sources");
    let classes = directory.path().join("classes");
    std::fs::create_dir_all(&source_root).unwrap();
    let source = r"package library;
        public interface Logger {
            void error(Marker marker, String message);
            void error(String message, Object value);
            void error(String message, Throwable failure);
            void info(String message, Object value);
            void info(String message, Object first, Object second);
            void info(String message, Object... values);
        }
        interface Marker {}";
    let logger_path = source_root.join("Logger.java");
    std::fs::write(&logger_path, source).unwrap();
    let constants_path = source_root.join("Constants.java");
    std::fs::write(
        &constants_path,
        r#"package library;
        public class Constants {
            public static final Value VALUE = new Value();
            public static class Value { public String text() { return "value"; } }
        }"#,
    )
    .unwrap();
    let output = Command::new("javac")
        .arg("-d")
        .arg(&classes)
        .arg(&logger_path)
        .arg(&constants_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let jar = directory.path().join("library.jar");
    let output = Command::new("jar")
        .arg("cf")
        .arg(&jar)
        .arg("-C")
        .arg(&classes)
        .arg(".")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let index = Arc::new(WorkspaceIndex::new(directory.path().join("workspace")));
    index.index_jar_file(&jar).unwrap();
    assert_eq!(
        index
            .get_symbol("library.Constants.VALUE")
            .unwrap()
            .return_type
            .as_deref(),
        Some("library.Constants.Value")
    );
    let source = r#"import library.Logger; import static library.Constants.VALUE;
        class Use {
            String value() { return VALUE.text(); }
            void test(Logger logger, Throwable failure) {
                logger.error("Example operation failed", failure);
                logger.info("Example {} value {}", 3, 7L);
                logger.info("Example {} value {} limit {}", 3, 2, 7L);
            }
        }"#;
    let path = directory.path().join("workspace/Use.java");
    index.index_file_with_source(&path, source).unwrap();
    let file_id = index.get_file_id(&path).unwrap();
    let (green, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(errors.is_empty(), "{errors:?}");
    let syntax = SyntaxNode::new_root(green);
    let semantics = compute_file_semantics(&syntax, file_id, &SymbolTable::new(), &index);
    let mut checker = TypeChecker::new();
    assert!(
        semantics.diagnostics().is_empty(),
        "{:?}",
        semantics.diagnostics()
    );
    checker.set_file_semantics(Arc::new(semantics));
    let errors = checker.check_with_context(&syntax, &[], file_id, index);
    assert!(errors.is_empty(), "{errors:?}");
}
