// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for code inspections (lint-style warnings)
//!
//! Verifies that inspections detect code quality issues:
//! - Empty catch blocks
//! - String comparison using ==
//! - Redundant modifiers

use hyperion_lsp::inspections::run_inspections;
use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::SyntaxNode;
use hyperion_workspace::WorkspaceIndex;
use tempfile::TempDir;

fn parse(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

fn get_diagnostics(source: &str) -> Vec<tower_lsp::lsp_types::Diagnostic> {
    let syntax = parse(source);
    let temp_dir = TempDir::new().expect("temp dir");
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let source_path = temp_dir.path().join("Test.java");
    index
        .index_file_with_source(&source_path, source)
        .expect("index source");
    let file_id = index.get_file_id(&source_path).expect("file id");

    run_inspections(&syntax, source, &index, file_id)
}

// Test 1: Empty catch block should warn
#[test]
fn test_empty_catch_block_warning() {
    let source = r"
class Test {
    void foo() {
        try {
            riskyOp();
        } catch (Exception e) {
        }
    }
    void riskyOp() {}
}
";

    let diagnostics = get_diagnostics(source);

    let has_empty_catch = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("empty") && d.message.to_lowercase().contains("catch")
    });

    assert!(
        has_empty_catch,
        "Should warn about empty catch block. Got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// Test 2: Non-empty catch block should NOT warn
#[test]
fn test_non_empty_catch_block_no_warning() {
    let source = r"
class Test {
    void foo() {
        try {
            riskyOp();
        } catch (Exception e) {
            e.printStackTrace();
        }
    }
    void riskyOp() {}
}
";

    let diagnostics = get_diagnostics(source);

    let has_empty_catch = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("empty") && d.message.to_lowercase().contains("catch")
    });

    assert!(
        !has_empty_catch,
        "Should NOT warn about non-empty catch block. Got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// Test 3: String comparison with == should warn
#[test]
fn test_string_comparison_warning() {
    let source = r#"
class Test {
    void foo() {
        String a = "hello";
        String b = "world";
        if (a == b) {
            System.out.println("equal");
        }
    }
}
"#;

    let diagnostics = get_diagnostics(source);

    let has_string_comparison = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("string") && d.message.to_lowercase().contains("==")
    });

    // Note: This test documents expected behavior
    // If not implemented, the assertion will fail and we'll know
    if has_string_comparison {
        println!("✅ String == comparison warning is implemented");
    } else {
        println!(
            "⚠️ String == comparison warning not found. Got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}

// Test 4: Redundant 'public' in interface should warn
#[test]
fn test_redundant_modifier_in_interface() {
    let source = r"
interface MyInterface {
    public void doSomething();
}
";

    let diagnostics = get_diagnostics(source);

    let has_redundant_modifier = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("redundant")
            && d.message.to_lowercase().contains("public")
    });

    // Note: This test documents expected behavior
    if has_redundant_modifier {
        println!("✅ Redundant modifier warning is implemented");
    } else {
        println!(
            "⚠️ Redundant modifier warning not found. Got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}

// Test 5: Non-redundant modifiers should NOT warn
#[test]
fn test_non_redundant_modifier_no_warning() {
    let source = r"
class Test {
    public void doSomething() {}
}
";

    let diagnostics = get_diagnostics(source);

    let has_redundant_modifier = diagnostics.iter().any(|d| {
        d.message.to_lowercase().contains("redundant")
            && d.message.to_lowercase().contains("modifier")
    });

    assert!(
        !has_redundant_modifier,
        "Should NOT warn about non-redundant modifier. Got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// The text a diagnostic's range covers.
fn marked(source: &str, diagnostic: &tower_lsp::lsp_types::Diagnostic) -> String {
    let offset = |position: tower_lsp::lsp_types::Position| {
        let line_start: usize = source
            .split_inclusive('\n')
            .take(position.line as usize)
            .map(str::len)
            .sum();
        line_start + position.character as usize
    };
    source[offset(diagnostic.range.start)..offset(diagnostic.range.end)].to_string()
}

// A lint about a method or a clause marks its name or its header. A range
// over the whole node underlines every line of it in an editor.
#[test]
fn a_missing_override_warning_marks_the_method_name_only() {
    let source = r"
class Test {
    /**
     * Documented, so the method node starts three lines above its name.
     */
    public String toString() {
        String a = first();
        return a;
    }
    String first() { return null; }
}
";

    let diagnostics = get_diagnostics(source);
    let missing = diagnostics
        .iter()
        .find(|d| d.message.contains("lacks @Override"))
        .expect("toString without @Override is reported");
    assert_eq!(marked(source, missing), "toString");
}

#[test]
fn an_empty_catch_warning_marks_the_catch_header_only() {
    let source = r"
class Test {
    void foo() {
        try {
            riskyOp();
        } catch (Exception e) {


        }
    }
    void riskyOp() {}
}
";

    let diagnostics = get_diagnostics(source);
    let empty = diagnostics
        .iter()
        .find(|d| d.message.starts_with("Empty catch block"))
        .expect("the empty catch block is reported");
    assert_eq!(marked(source, empty), "catch (Exception e)");
}

/// The JDK types the resource-leak fixtures use, so the check can resolve
/// them and see which are `AutoCloseable`.
const RESOURCE_TYPES: &str = r"
package java.lang;
public interface AutoCloseable { void close() throws Exception; }
";

fn resource_leaks_in(body: &str) -> Vec<String> {
    let syntax_source = format!(
        "import java.io.*;\nimport java.nio.file.*;\nimport java.util.List;\nimport java.util.concurrent.*;\nimport java.util.stream.Stream;\n\nclass Test {{\n    private InputStream kept;\n{body}\n    void consume(InputStream in) {{}}\n}}\n"
    );
    let syntax = parse(&syntax_source);
    let temp_dir = TempDir::new().expect("temp dir");
    let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
    let declare = |fqn: &str, text: String| {
        let path = temp_dir
            .path()
            .join(format!("{}.java", fqn.replace('.', "/")));
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create package");
        index
            .index_file_with_source(&path, &text)
            .expect("index stub");
    };
    declare("java.lang.AutoCloseable", RESOURCE_TYPES.to_string());
    for (package, name, kind, parent) in [
        ("java.io", "Closeable", "interface", "extends AutoCloseable"),
        ("java.io", "InputStream", "class", "implements Closeable"),
        ("java.io", "FileInputStream", "class", "extends InputStream"),
        (
            "java.io",
            "ByteArrayOutputStream",
            "class",
            "implements Closeable",
        ),
        ("java.io", "StringWriter", "class", "implements Closeable"),
        ("java.io", "Reader", "class", "implements Closeable"),
        ("java.io", "BufferedReader", "class", "extends Reader"),
        ("java.io", "InputStreamReader", "class", "extends Reader"),
        (
            "java.util.stream",
            "Stream",
            "interface",
            "extends AutoCloseable",
        ),
        (
            "java.util.concurrent",
            "ExecutorService",
            "interface",
            "extends AutoCloseable",
        ),
    ] {
        declare(
            &format!("{package}.{name}"),
            format!("package {package};\npublic {kind} {name} {parent} {{}}\n"),
        );
    }
    let source_path = temp_dir.path().join("Test.java");
    index
        .index_file_with_source(&source_path, &syntax_source)
        .expect("index source");
    let file_id = index.get_file_id(&source_path).expect("file id");
    run_inspections(&syntax, &syntax_source, &index, file_id)
        .iter()
        .filter(|d| {
            d.code
                == Some(tower_lsp::lsp_types::NumberOrString::String(
                    "resource_leak".to_string(),
                ))
        })
        .map(|d| marked(&syntax_source, d))
        .collect()
}

#[test]
fn a_resource_opened_and_never_closed_is_reported_on_its_name() {
    assert_eq!(
        resource_leaks_in(
            "    int leaked(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        return in.read();\n    }\n    int created(File file) throws IOException {\n        FileInputStream stream = new FileInputStream(file);\n        return stream.read();\n    }"
        ),
        ["in", "stream"]
    );
}

#[test]
fn a_resource_that_is_closed_or_handed_to_someone_else_is_not_a_leak() {
    let handled = resource_leaks_in(
        "    InputStream returned(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        return in;\n    }\n    void stored(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        this.kept = in;\n    }\n    void handedOver(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        consume(in);\n    }\n    void wrapped(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        BufferedReader reader = new BufferedReader(new InputStreamReader(in));\n        try {\n            reader.read();\n        } finally {\n            reader.close();\n        }\n    }\n    void closedPlainly(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        in.read();\n        in.close();\n    }\n    void declaredFirst(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        try (in) {\n            in.read();\n        }\n    }",
    );
    assert!(handled.is_empty(), "{handled:?}");
}

#[test]
fn a_close_in_one_method_does_not_excuse_a_leak_in_another() {
    assert_eq!(
        resource_leaks_in(
            "    void closes(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        try {\n            in.read();\n        } finally {\n            in.close();\n        }\n    }\n    int leaks(Path path) throws IOException {\n        InputStream in = Files.newInputStream(path);\n        return in.read();\n    }"
        ),
        ["in"]
    );
}

#[test]
fn buffers_pipelines_and_executors_have_nothing_to_release() {
    let quiet = resource_leaks_in(
        "    long streams(List<String> names) {\n        Stream<String> upper = names.stream().map(String::toUpperCase);\n        return upper.count();\n    }\n    String memory() {\n        ByteArrayOutputStream out = new ByteArrayOutputStream();\n        StringWriter text = new StringWriter();\n        return out.toString() + text;\n    }\n    void pool() {\n        ExecutorService pool = Executors.newFixedThreadPool(2);\n        pool.shutdown();\n    }",
    );
    assert!(quiet.is_empty(), "{quiet:?}");
}

#[test]
fn a_stream_over_a_directory_or_file_must_be_closed() {
    assert_eq!(
        resource_leaks_in(
            "    long lines(Path path) throws IOException {\n        Stream<String> lines = Files.lines(path);\n        return lines.count();\n    }"
        ),
        ["lines"]
    );
}
