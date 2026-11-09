// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for inlay hints feature
//!
//! Tests type hints for var declarations

use hyperion_lsp::inlay_hints::InlayHintsProvider;
use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_semantics::db::FileId;
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tower_lsp::lsp_types::{Position, Range};

/// Helper to create test environment and run inlay hints
fn test_inlay_hints(source: &str) -> Vec<String> {
    // Create range covering entire document
    let lines = source.lines().count();
    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: lines as u32,
            character: 0,
        },
    };

    let workspace_index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/test")));
    let host = SemanticHost::new(Arc::clone(&workspace_index));

    // Set up file - MUST call register_file to set both text AND language
    // See hyperion-semantics/src/db.rs for documentation on why this is required
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });

    let txn = host.read_now();
    let provider = InlayHintsProvider::new(&txn, workspace_index);

    match provider.inlay_hints(file_id, range, source) {
        Ok(hints) => hints
            .into_iter()
            .map(|hint| {
                // Extract just the label text
                match hint.label {
                    tower_lsp::lsp_types::InlayHintLabel::String(s) => s,
                    tower_lsp::lsp_types::InlayHintLabel::LabelParts(parts) => {
                        parts.into_iter().map(|p| p.value).collect::<String>()
                    }
                }
            })
            .collect(),
        Err(e) => {
            eprintln!("Inlay hints error: {e}");
            vec![]
        }
    }
}

/// Helper that indexes a real workspace file before requesting inlay hints.
///
/// This enables method/receiver resolution paths that depend on workspace index data.
fn test_inlay_hints_with_indexed_file(source: &str, rel_path: &str) -> Vec<String> {
    let lines = source.lines().count();
    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: lines as u32,
            character: 0,
        },
    };

    let temp = TempDir::new().expect("Failed to create temp workspace");
    let root = temp.path().to_path_buf();
    let file_path = root.join(rel_path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create parent directory");
    }
    std::fs::write(&file_path, source).expect("Failed to write source file");

    let workspace_index = Arc::new(WorkspaceIndex::new(root));
    workspace_index
        .index_file_with_source(&file_path, source)
        .expect("Failed to index workspace source");
    let file_id = workspace_index
        .get_file_id(&file_path)
        .expect("Expected indexed file_id");

    let host = SemanticHost::new(Arc::clone(&workspace_index));
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });

    let txn = host.read_now();
    let provider = InlayHintsProvider::new(&txn, workspace_index);
    match provider.inlay_hints(file_id, range, source) {
        Ok(hints) => hints
            .into_iter()
            .map(|hint| match hint.label {
                tower_lsp::lsp_types::InlayHintLabel::String(s) => s,
                tower_lsp::lsp_types::InlayHintLabel::LabelParts(parts) => {
                    parts.into_iter().map(|p| p.value).collect::<String>()
                }
            })
            .collect(),
        Err(e) => {
            eprintln!("Inlay hints error: {e}");
            vec![]
        }
    }
}

#[test]
fn test_var_simple_constructor() {
    let source = r"
package com.example;

class User {
    String name;
}

class Test {
    void test() {
        var user = new User();
    }
}
";

    let hints = test_inlay_hints(source);

    assert_eq!(
        hints.len(),
        1,
        "Expected 1 hint, got {}: {:?}",
        hints.len(),
        hints
    );
    assert!(
        hints[0].contains("User"),
        "Expected hint to contain 'User', got: {}",
        hints[0]
    );
}

#[test]
fn test_var_primitive() {
    let source = r"
class Test {
    void test() {
        var count = 42;
    }
}
";

    let hints = test_inlay_hints(source);

    assert_eq!(hints.len(), 1);
    assert!(
        hints[0].contains("int"),
        "Expected hint to contain 'int', got: {}",
        hints[0]
    );
}

#[test]
fn test_var_string_literal() {
    let source = r#"
class Test {
    void test() {
        var message = "Hello";
    }
}
"#;

    let hints = test_inlay_hints(source);

    assert_eq!(hints.len(), 1);
    assert!(
        hints[0].contains("String"),
        "Expected hint to contain 'String', got: {}",
        hints[0]
    );
}

#[test]
fn test_var_method_call() {
    let source = r#"
package com.example;

class User {
    String getName() { return ""; }
}

class Service {
    User getUser() { return new User(); }
}

class Test {
    void test() {
        Service service = new Service();
        var user = service.getUser();
    }
}
"#;

    let hints = test_inlay_hints_with_indexed_file(source, "src/main/java/Test.java");

    assert_eq!(hints, vec![": User"]);
}

#[test]
fn test_parameter_hints_same_arity_overloads_use_argument_types() {
    let source = r#"
class Utils {
    void process(int count) {}
    void process(String text) {}
}

class Test {
    void test() {
        Utils utils = new Utils();
        utils.process("hello");
    }
}
"#;

    let hints = test_inlay_hints_with_indexed_file(source, "src/main/java/Test.java");
    let param_hints: Vec<_> = hints.iter().filter(|h| h.ends_with(':')).cloned().collect();

    assert!(
        param_hints.iter().any(|h| h == "text:"),
        "Expected parameter hint for String overload ('text:'), got: {:?}",
        param_hints
    );
    assert!(
        !param_hints.iter().any(|h| h == "count:"),
        "Did not expect int overload hint ('count:') for String call, got: {:?}",
        param_hints
    );
}

#[test]
fn test_parameter_hints_resolve_overloaded_receiver_in_call_chain() {
    let source = r#"
class WidenedChoice {
    void accept(String widened) {}
}

class BoxedChoice {
    void accept(String boxed) {}
}

class Factory {
    BoxedChoice choose(Integer value) { return new BoxedChoice(); }
    WidenedChoice choose(long value) { return new WidenedChoice(); }
}

class Test {
    void test() {
        Factory factory = new Factory();
        factory.choose(1).accept("result");
    }
}
"#;

    let hints = test_inlay_hints_with_indexed_file(source, "src/main/java/Test.java");
    let param_hints: Vec<_> = hints.iter().filter(|hint| hint.ends_with(':')).collect();

    assert!(
        param_hints.iter().any(|hint| hint.as_str() == "widened:"),
        "primitive widening should select WidenedChoice before boxing: {param_hints:?}"
    );
    assert!(
        !param_hints.iter().any(|hint| hint.as_str() == "boxed:"),
        "the declaration-first boxed receiver must not leak into the outer hint: {param_hints:?}"
    );
}

#[test]
fn test_parameter_hints_do_not_guess_for_incompatible_known_argument() {
    let source = r#"
class Utils {
    void process(int count) {}
    void process(long distance) {}
}

class Test {
    void test() {
        Utils utils = new Utils();
        utils.process("not a number");
    }
}
"#;

    let hints = test_inlay_hints_with_indexed_file(source, "src/main/java/Test.java");
    let param_hints: Vec<_> = hints.iter().filter(|hint| hint.ends_with(':')).collect();

    assert!(
        !param_hints
            .iter()
            .any(|hint| matches!(hint.as_str(), "count:" | "distance:")),
        "an invalid call must not receive a guessed overload's parameter name: {param_hints:?}"
    );
}

#[test]
fn parameter_hints_do_not_borrow_an_unrelated_unique_method_name() {
    let source = r#"
class Unrelated {
    void add(String element) {}
}

class Test {
    void test() {
        missing.add("value");
    }
}
"#;

    let hints = test_inlay_hints_with_indexed_file(source, "src/main/java/Test.java");
    assert!(
        !hints.iter().any(|hint| hint == "element:"),
        "an unresolved receiver must not borrow Unrelated.add parameters: {hints:?}"
    );
}

#[test]
fn test_parameter_hints_multiple_arguments() {
    let source = r"
class Geometry {
    double area(double width, double height) { return width * height; }
}

class Test {
    void test() {
        Geometry geo = new Geometry();
        geo.area(10.0, 20.0);
    }
}
";

    let hints = test_inlay_hints_with_indexed_file(source, "src/main/java/Test.java");
    let param_hints: Vec<_> = hints.iter().filter(|h| h.ends_with(':')).cloned().collect();

    assert_eq!(
        param_hints.len(),
        2,
        "Expected 2 parameter hints for area(width, height), got: {:?}",
        param_hints
    );
    assert!(
        param_hints.iter().any(|h| h == "width:"),
        "Expected 'width:' hint, got: {:?}",
        param_hints
    );
    assert!(
        param_hints.iter().any(|h| h == "height:"),
        "Expected 'height:' hint, got: {:?}",
        param_hints
    );
}

#[test]
fn test_parameter_hints_three_arguments() {
    let source = r"
class Color {
    static Color rgb(int r, int g, int b) { return new Color(); }
}

class Test {
    void test() {
        Color.rgb(255, 128, 0);
    }
}
";

    let hints = test_inlay_hints_with_indexed_file(source, "src/main/java/Test.java");
    let param_hints: Vec<_> = hints.iter().filter(|h| h.ends_with(':')).cloned().collect();

    assert_eq!(
        param_hints.len(),
        3,
        "Expected 3 parameter hints for rgb(r, g, b), got: {:?}",
        param_hints
    );
    assert!(
        param_hints.iter().any(|h| h == "r:"),
        "Expected 'r:' hint, got: {:?}",
        param_hints
    );
    assert!(
        param_hints.iter().any(|h| h == "g:"),
        "Expected 'g:' hint, got: {:?}",
        param_hints
    );
    assert!(
        param_hints.iter().any(|h| h == "b:"),
        "Expected 'b:' hint, got: {:?}",
        param_hints
    );
}

#[test]
fn test_var_generic_collection() {
    let source = r"
import java.util.ArrayList;
import java.util.List;

class User {}

class Test {
    void test() {
        var users = new ArrayList<User>();
    }
}
";

    let hints = test_inlay_hints(source);

    assert_eq!(hints.len(), 1);
    // Should show ArrayList<User>
    assert!(
        hints[0].contains("ArrayList"),
        "Expected hint to contain 'ArrayList', got: {}",
        hints[0]
    );
    assert!(
        hints[0].contains("User"),
        "Expected hint to contain 'User', got: {}",
        hints[0]
    );
}

#[test]
fn test_no_hint_for_explicit_type() {
    let source = r"
class User {}

class Test {
    void test() {
        User user = new User();
    }
}
";

    let hints = test_inlay_hints(source);

    // No hints for explicit types
    assert_eq!(
        hints.len(),
        0,
        "Expected no hints for explicit type, got: {hints:?}"
    );
}

#[test]
fn test_var_array() {
    let source = r"
class Test {
    void test() {
        var numbers = new int[10];
        var matrix = new String[2][3];
        var initialized = new long[]{1L, 2L};
    }
}
";

    let hints = test_inlay_hints(source);

    assert_eq!(hints, vec![": int[]", ": String[][]", ": long[]"]);
}

#[test]
fn test_multiple_vars() {
    let source = r#"
class Test {
    void test() {
        var count = 42;
        var message = "Hello";
        var flag = true;
    }
}
"#;

    let hints = test_inlay_hints(source);

    assert_eq!(
        hints.len(),
        3,
        "Expected 3 hints, got {}: {:?}",
        hints.len(),
        hints
    );

    // Check types
    assert!(hints.iter().any(|h| h.contains("int")));
    assert!(hints.iter().any(|h| h.contains("String")));
    assert!(hints.iter().any(|h| h.contains("boolean")));
}
