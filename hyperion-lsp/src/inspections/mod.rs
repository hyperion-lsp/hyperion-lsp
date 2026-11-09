// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Code Inspections Module
//!
//! This module provides lint-style warnings for code quality issues that aren't
//! type errors but may indicate problems or improvements.

pub mod kotlin;
mod safety_checks;
mod style_checks;
mod type_checks;

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_workspace::WorkspaceIndex;
use std::time::{Duration, Instant};
use tower_lsp::lsp_types::Diagnostic;
use tracing::{debug, warn};

pub use kotlin::{
    kotlin_inspection_codes, registry as kotlin_inspection_registry, run_kotlin_inspections,
    KotlinInspection, KotlinInspectionContext,
};

// Re-export check functions used by tests
#[cfg(test)]
pub(crate) use safety_checks::{check_null_safety, check_resource_leaks, check_threading_issues};
#[cfg(test)]
pub(crate) use style_checks::{
    check_magic_numbers, check_unused_imports, check_unused_parameters, check_unused_variables,
    extract_import_simple_name,
};
#[cfg(test)]
pub(crate) use type_checks::check_raw_type_usage;

/// One entry in the Java inspection pipeline.
///
/// Table-driven so the pipeline is enumerable: [`java_inspection_count`] is asserted by
/// tests, which catches an inspection being silently dropped.
enum JavaInspection {
    /// Needs only the syntax tree and the source text.
    Syntax {
        name: &'static str,
        /// Off unless the user opts in (`diagnostics.opt_in_inspections`).
        /// Mirrors IntelliJ's default profile, where these inspections exist
        /// but are disabled.
        opt_in: bool,
        check: fn(&SyntaxNode, &str) -> Vec<Diagnostic>,
    },
    /// Additionally needs the workspace index and the file's resolved imports.
    WorkspaceAware {
        name: &'static str,
        opt_in: bool,
        check: fn(
            &SyntaxNode,
            &str,
            &WorkspaceIndex,
            Option<&hyperion_workspace::ImportMap>,
        ) -> Vec<Diagnostic>,
    },
}

/// Every Java inspection, in reporting order.
const JAVA_INSPECTIONS: &[JavaInspection] = &[
    JavaInspection::Syntax {
        name: "unused variables",
        opt_in: false,
        check: style_checks::check_unused_variables,
    },
    JavaInspection::Syntax {
        name: "unused imports",
        opt_in: false,
        check: style_checks::check_unused_imports,
    },
    JavaInspection::Syntax {
        name: "missing override",
        opt_in: false,
        check: style_checks::check_missing_override,
    },
    JavaInspection::Syntax {
        name: "empty catch blocks",
        opt_in: false,
        check: style_checks::check_empty_catch_blocks,
    },
    JavaInspection::Syntax {
        name: "string comparison",
        opt_in: false,
        check: style_checks::check_string_comparison,
    },
    JavaInspection::Syntax {
        name: "redundant modifiers",
        opt_in: false,
        check: style_checks::check_redundant_modifiers,
    },
    JavaInspection::Syntax {
        name: "raw type usage",
        opt_in: true,
        check: type_checks::check_raw_type_usage,
    },
    JavaInspection::Syntax {
        name: "unused parameters",
        opt_in: false,
        check: style_checks::check_unused_parameters,
    },
    JavaInspection::Syntax {
        name: "magic numbers",
        opt_in: true,
        check: style_checks::check_magic_numbers,
    },
    JavaInspection::WorkspaceAware {
        name: "resource leaks",
        opt_in: false,
        check: safety_checks::check_resource_leaks,
    },
    JavaInspection::Syntax {
        name: "null safety",
        opt_in: true,
        check: safety_checks::check_null_safety,
    },
    JavaInspection::Syntax {
        name: "threading issues",
        opt_in: true,
        check: safety_checks::check_threading_issues,
    },
];

/// Number of Java inspections in the pipeline.
pub const fn java_inspection_count() -> usize {
    JAVA_INSPECTIONS.len()
}

/// Runs the default-on inspections on a file and returns diagnostics.
pub fn run_inspections(
    syntax: &SyntaxNode,
    source_text: &str,
    workspace_index: &WorkspaceIndex,
    file_id: hyperion_workspace::FileId,
) -> Vec<Diagnostic> {
    run_inspections_with_opt_in(syntax, source_text, workspace_index, file_id, false)
}

/// Runs the default-on inspections, plus the opt-in ones when `include_opt_in`.
pub fn run_inspections_with_opt_in(
    syntax: &SyntaxNode,
    source_text: &str,
    workspace_index: &WorkspaceIndex,
    file_id: hyperion_workspace::FileId,
    include_opt_in: bool,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let imports = workspace_index.get_imports(file_id);

    for inspection in JAVA_INSPECTIONS {
        let opt_in = match inspection {
            JavaInspection::Syntax { opt_in, .. }
            | JavaInspection::WorkspaceAware { opt_in, .. } => *opt_in,
        };
        if opt_in && !include_opt_in {
            continue;
        }
        let (name, found) = match inspection {
            JavaInspection::Syntax { name, check, .. } => {
                debug!(inspection = name, ?file_id, "Starting Java inspection");
                let started = Instant::now();
                let found = check(syntax, source_text);
                log_inspection_duration(name, file_id, started.elapsed());
                (name, found)
            }
            JavaInspection::WorkspaceAware { name, check, .. } => {
                debug!(inspection = name, ?file_id, "Starting Java inspection");
                let started = Instant::now();
                let found = check(syntax, source_text, workspace_index, imports.as_ref());
                log_inspection_duration(name, file_id, started.elapsed());
                (name, found)
            }
        };
        debug!(
            inspection = name,
            diagnostics = found.len(),
            "Java inspection completed"
        );
        diagnostics.extend(found);
    }

    diagnostics
}

fn log_inspection_duration(
    inspection: &'static str,
    file_id: hyperion_workspace::FileId,
    elapsed: Duration,
) {
    if elapsed >= Duration::from_millis(100) {
        warn!(
            inspection,
            ?file_id,
            ?elapsed,
            "Slow Java inspection completed"
        );
    } else {
        debug!(inspection, ?file_id, ?elapsed, "Java inspection timing");
    }
}

/// Find the containing scope (method body or block) for a node
pub(crate) fn find_containing_scope(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut current = node.parent();
    while let Some(parent) = current {
        // METHOD_DECL contains the scope for local variables
        if parent.kind() == SyntaxKind::METHOD_DECL {
            // Return the BLOCK_STMT inside the method (the method body)
            for child in parent.children() {
                if child.kind() == SyntaxKind::BLOCK_STMT {
                    return Some(child);
                }
            }
            return Some(parent);
        }
        // Also handle constructor declarations, static initializers, etc.
        if matches!(
            parent.kind(),
            SyntaxKind::CONSTRUCTOR_DECL | SyntaxKind::STATIC_INIT
        ) {
            for child in parent.children() {
                if child.kind() == SyntaxKind::BLOCK_STMT {
                    return Some(child);
                }
            }
            return Some(parent);
        }
        current = parent.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};
    use hyperion_workspace::{ImportMap, WorkspaceIndex};
    use tempfile::TempDir;
    use tower_lsp::lsp_types::NumberOrString;

    fn setup_resource_leak_test(
        source: &str,
        stubs: &[(&str, &str)],
    ) -> (TempDir, WorkspaceIndex, ImportMap) {
        let temp_dir = TempDir::new().expect("temp dir");
        let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());

        // Hierarchy queries deliberately fail closed when a recorded supertype
        // is absent. Model the JDK root contract explicitly so these fixtures
        // prove transitive AutoCloseable detection instead of depending on an
        // unresolved bare name being treated as a real hierarchy edge.
        index
            .index_file_with_source(
                &temp_dir.path().join("java/lang/AutoCloseable.java"),
                "package java.lang; public interface AutoCloseable { void close(); }",
            )
            .expect("index AutoCloseable contract");

        for (rel_path, contents) in stubs {
            let path = temp_dir.path().join(rel_path);
            index
                .index_file_with_source(&path, contents)
                .expect("index stub");
        }

        let source_path = temp_dir.path().join("Test.java");
        index
            .index_file_with_source(&source_path, source)
            .expect("index source");

        let file_id = index.get_file_id(&source_path).expect("file id");
        let imports = index.get_imports(file_id).expect("imports");

        (temp_dir, index, imports)
    }

    #[test]
    fn test_extract_import_simple_name() {
        assert_eq!(
            extract_import_simple_name("import java.util.List;"),
            Some("List".to_string())
        );
        assert_eq!(
            extract_import_simple_name("import static org.junit.Assert.assertEquals;"),
            Some("assertEquals".to_string())
        );
        assert_eq!(
            extract_import_simple_name("import java.util.*;"),
            Some("*".to_string())
        );
    }

    #[test]
    fn test_unused_variable_with_method_call_initializer() {
        // This test reproduces the bug where `getPending` was incorrectly identified
        // as the variable name instead of `pending`
        let source = r"
public class Test {
    public void process() {
        List<Money> pending = balance.getPending();
        for (Money m : pending) {
            result += m.getAmount();
        }
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_unused_variables(&root, source);

        // The variable `pending` IS used in the for-loop, so there should be no diagnostics
        // Previously this would incorrectly report `getPending` as unused
        assert!(
            diagnostics.is_empty(),
            "Expected no unused variable warnings, but got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_unused_variable_actually_unused() {
        let source = r#"
public class Test {
    public void process() {
        String unused = "hello";
        System.out.println("done");
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_unused_variables(&root, source);

        // Should report `unused` as unused
        assert_eq!(diagnostics.len(), 1, "Expected 1 unused variable warning");
        assert!(
            diagnostics[0].message.contains("unused"),
            "Expected warning about 'unused' variable, got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_raw_type_usage() {
        let source = r"
public class Test {
    public void process() {
        List rawList = new ArrayList();
        List<String> typedList = new ArrayList<>();
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_raw_type_usage(&root, source);

        // Should report `List` as raw type but not `List<String>`
        assert_eq!(diagnostics.len(), 1, "Expected 1 raw type warning");
        assert!(
            diagnostics[0].message.contains("List"),
            "Expected warning about 'List', got: {}",
            diagnostics[0].message
        );
    }

    fn unused_parameter_names(source: &str) -> Vec<String> {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);
        check_unused_parameters(&root, source)
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn test_unused_parameters() {
        let messages = unused_parameter_names(
            r"
public class Test {
    private void process(String used, String unused) {
        System.out.println(used);
    }
}
        ",
        );
        assert_eq!(messages, vec!["Parameter 'unused' is never used"]);
    }

    #[test]
    fn unused_parameter_is_reported_for_static_and_final_methods() {
        let messages = unused_parameter_names(
            r"
public class Test {
    static int a(int x) { return 1; }
    final int b(int y) { return 1; }
}
final class Sealed {
    public int c(int z) { return 1; }
}
",
        );
        assert_eq!(messages.len(), 3, "{messages:?}");
    }

    #[test]
    fn unused_parameter_is_not_reported_for_main() {
        assert!(unused_parameter_names(
            r"
public class Test {
    public static void main(String[] args) {
        System.out.println(1);
    }
}
"
        )
        .is_empty());
    }

    #[test]
    fn unused_parameter_is_not_reported_when_the_signature_is_imposed() {
        // Overridable, overriding, interface-implementing and framework
        // invoked methods cannot drop the parameter.
        let messages = unused_parameter_names(
            r"
public class Test implements Comparable<Test>, Runnable {
    public int compareTo(Test other) { return 0; }
    @Override
    public boolean equals(Object o) { return true; }
    public void overridable(int notUsed) { }
    protected void hook(int notUsed) { }
    @Bean
    private String bean(int injected) { return null; }
    private void endpoint(@RequestParam String id, @Valid Body body) { }
    public void run() { }
    Object anonymous() {
        return new Handler() {
            public void handle(String event) { }
        };
    }
}
",
        );
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn test_magic_numbers() {
        let source = r"
public class Test {
    public void process() {
        int x = 42;
        int y = 1;  // OK - common value
        int z = 0;  // OK - common value
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_magic_numbers(&root, source);

        // Should report 42 as magic number but not 0 or 1
        assert_eq!(diagnostics.len(), 1, "Expected 1 magic number warning");
        assert!(
            diagnostics[0].message.contains("42"),
            "Expected warning about '42', got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_resource_leak_basic() {
        let source = r#"
import java.io.FileInputStream;

public class Test {
    public void process() {
        FileInputStream fis = new FileInputStream("file.txt");
        // No close - resource leak!
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let (_temp_dir, index, imports) = setup_resource_leak_test(
            source,
            &[(
                "java/io/FileInputStream.java",
                "package java.io; public class FileInputStream implements AutoCloseable { public void close() {} }",
            )],
        );

        let diagnostics = check_resource_leaks(&root, source, &index, Some(&imports));

        assert_eq!(diagnostics.len(), 1, "Expected 1 resource leak warning");
        assert!(
            diagnostics[0].message.contains("FileInputStream"),
            "Expected warning about FileInputStream, got: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn test_resource_leak_try_with_resources_ok() {
        let source = r#"
import java.io.FileInputStream;

public class Test {
    public void process() {
        try (FileInputStream fis = new FileInputStream("file.txt")) {
            // Properly managed with try-with-resources
        }
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let (_temp_dir, index, imports) = setup_resource_leak_test(
            source,
            &[(
                "java/io/FileInputStream.java",
                "package java.io; public class FileInputStream implements AutoCloseable { public void close() {} }",
            )],
        );

        let diagnostics = check_resource_leaks(&root, source, &index, Some(&imports));

        // Should NOT report - resource is managed
        assert!(
            diagnostics.is_empty(),
            "Expected no warnings for try-with-resources, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_resource_leak_try_finally_ok() {
        let source = r#"
import java.io.FileInputStream;

public class Test {
    public void process() {
        FileInputStream fis = new FileInputStream("file.txt");
        try {
            // Use the resource
        } finally {
            fis.close();
        }
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let (_temp_dir, index, imports) = setup_resource_leak_test(
            source,
            &[(
                "java/io/FileInputStream.java",
                "package java.io; public class FileInputStream implements AutoCloseable { public void close() {} }",
            )],
        );

        let diagnostics = check_resource_leaks(&root, source, &index, Some(&imports));

        // Should NOT report - resource is closed in finally
        assert!(
            diagnostics.is_empty(),
            "Expected no warnings for try-finally with close(), got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_resource_leak_non_resource_type_ok() {
        let source = r#"
public class Test {
    public void process() {
        String text = new String("hello");
        // String is not a resource type - no warning
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let (_temp_dir, index, imports) = setup_resource_leak_test(source, &[]);

        let diagnostics = check_resource_leaks(&root, source, &index, Some(&imports));

        assert!(
            diagnostics.is_empty(),
            "Expected no warnings for non-resource types, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_resource_leak_multiple_resources() {
        let source = r"
import java.io.BufferedReader;
import java.sql.Connection;
import java.util.Scanner;

public class Test {
    public void process() {
        BufferedReader reader = new BufferedReader(null);
        Connection conn = getConnection();
        Scanner scanner = new Scanner(System.in);
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let (_temp_dir, index, imports) = setup_resource_leak_test(
            source,
            &[
                (
                    "java/io/BufferedReader.java",
                    "package java.io; public class BufferedReader implements AutoCloseable { public void close() {} }",
                ),
                (
                    "java/sql/Connection.java",
                    "package java.sql; public interface Connection extends AutoCloseable {}",
                ),
                (
                    "java/util/Scanner.java",
                    "package java.util; public class Scanner implements AutoCloseable { public void close() {} }",
                ),
            ],
        );

        let diagnostics = check_resource_leaks(&root, source, &index, Some(&imports));

        // Should report all 3 resources
        assert_eq!(diagnostics.len(), 3, "Expected 3 resource leak warnings");
    }

    #[test]
    fn test_resource_leak_non_autocloseable_type_ok() {
        let source = r"
import java.net.HttpURLConnection;

public class Test {
    public void process() {
        HttpURLConnection conn = null;
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let (_temp_dir, index, imports) = setup_resource_leak_test(
            source,
            &[(
                "java/net/HttpURLConnection.java",
                "package java.net; public class HttpURLConnection {}",
            )],
        );

        let diagnostics = check_resource_leaks(&root, source, &index, Some(&imports));

        assert!(
            diagnostics.is_empty(),
            "Expected no warnings for HttpURLConnection, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_null_safety_nullable_method_no_check() {
        let source = r#"
public class Test {
    public void process() {
        String value = map.get("key");
        value.length();  // Potential null pointer
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_null_safety(&root, source);

        // Should report potential null pointer
        assert!(!diagnostics.is_empty(), "Expected null safety warning");
        assert!(
            diagnostics.iter().any(|d| d.message.contains("value")),
            "Expected warning about 'value', got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_null_safety_with_null_check() {
        let source = r#"
public class Test {
    public void process() {
        String value = map.get("key");
        if (value != null) {
            value.length();  // Safe - null checked
        }
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_null_safety(&root, source);

        // Should NOT report - variable is null-checked
        let null_pointer_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("null pointer"))
            .collect();
        assert!(
            null_pointer_warnings.is_empty(),
            "Expected no null pointer warnings with null check, got: {:?}",
            null_pointer_warnings
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_null_safety_method_chain() {
        let source = r#"
public class Test {
    public void process() {
        String name = map.get("key").toUpperCase();  // Chain on nullable result
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_null_safety(&root, source);

        // Should report method chain on nullable result
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("chain") || d.message.contains("get")),
            "Expected warning about method chain on nullable, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_null_safety_non_nullable_method_ok() {
        let source = r"
public class Test {
    public void process() {
        String value = list.size();  // size() doesn't return null
        value.length();
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_null_safety(&root, source);

        // Should NOT report - size() is not nullable
        let null_pointer_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("null pointer") && d.message.contains("value"))
            .collect();
        assert!(
            null_pointer_warnings.is_empty(),
            "Expected no warnings for non-nullable method, got: {:?}",
            null_pointer_warnings
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    // ========================================================================
    // Threading Safety Tests
    // ========================================================================

    #[test]
    fn test_threading_sleep_in_synchronized() {
        let source = r"
public class Test {
    public void process() {
        synchronized (this) {
            Thread.sleep(1000);
        }
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_threading_issues(&root, source);

        assert!(
            diagnostics.iter().any(|d| {
                d.code == Some(NumberOrString::String("sleep_in_synchronized".to_string()))
            }),
            "Expected sleep_in_synchronized warning"
        );
    }

    #[test]
    fn test_threading_wait_outside_synchronized() {
        let source = r"
public class Test {
    private Object lock = new Object();

    public void process() {
        lock.wait();
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_threading_issues(&root, source);

        assert!(
            diagnostics.iter().any(|d| {
                d.code
                    == Some(NumberOrString::String(
                        "wait_notify_without_sync".to_string(),
                    ))
            }),
            "Expected wait_notify_without_sync warning"
        );
    }

    #[test]
    fn test_threading_wait_inside_synchronized_ok() {
        let source = r"
public class Test {
    private Object lock = new Object();

    public void process() {
        synchronized (lock) {
            lock.wait();
        }
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_threading_issues(&root, source);

        assert!(
            !diagnostics.iter().any(|d| {
                d.code
                    == Some(NumberOrString::String(
                        "wait_notify_without_sync".to_string(),
                    ))
            }),
            "Expected no wait_notify_without_sync warning when inside synchronized"
        );
    }

    #[test]
    fn test_threading_synchronized_on_non_final() {
        let source = r"
public class Test {
    private Object lock = new Object();

    public void process() {
        synchronized (lock) {
            doWork();
        }
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_threading_issues(&root, source);

        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == Some(NumberOrString::String("sync_on_non_final".to_string()))),
            "Expected sync_on_non_final warning"
        );
    }

    #[test]
    fn test_threading_synchronized_on_final_ok() {
        let source = r"
public class Test {
    private final Object lock = new Object();

    public void process() {
        synchronized (lock) {
            doWork();
        }
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_threading_issues(&root, source);

        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == Some(NumberOrString::String("sync_on_non_final".to_string()))),
            "Expected no sync_on_non_final warning for final field"
        );
    }

    #[test]
    fn test_import_used_only_as_parameter_annotation() {
        let source = r"
import org.springframework.web.bind.annotation.RequestBody;

public class Test {
    public void process(@RequestBody String body) {
        System.out.println(body);
    }
}
        ";

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        let diagnostics = check_unused_imports(&root, source);

        // RequestBody is used as an annotation on a parameter - should NOT be reported as unused
        assert!(
            diagnostics.is_empty(),
            "Expected no unused import warnings for annotation used on parameter, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
