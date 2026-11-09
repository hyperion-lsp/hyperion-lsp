// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Checking and Validation Tests
//!
//! Tests for detecting type mismatches in variable declarations and assignments.
//! This is essential for providing compile-time error detection in the LSP.

use hyperion_lsp::diagnostics::semantic_error_to_diagnostic;
use hyperion_parser::{lexer::Lexer, parser::Parser, SyntaxNode};
use hyperion_semantics::{semantic_walker::compute_file_semantics, ResolverBuilder, TypeChecker};
use hyperion_workspace::index::WorkspaceIndex;
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

fn parse_java(source: &str) -> SyntaxNode {
    let tokens: Vec<_> = Lexer::new(source).collect();
    let (green_node, _errors) = Parser::parse(tokens);
    SyntaxNode::new_root(green_node)
}

fn semantic_diagnostics(source: &str) -> Vec<Diagnostic> {
    let root = parse_java(source);
    let mut checker = TypeChecker::new();
    let errors = checker.check(&root, &[]);
    errors
        .iter()
        .map(|error| semantic_error_to_diagnostic(error, source))
        .collect()
}

fn semantic_diagnostics_with_workspace(source: &str) -> Vec<Diagnostic> {
    semantic_diagnostics_with_workspace_and_files(source, &[])
}

fn semantic_diagnostics_with_workspace_and_files(
    source: &str,
    extra_files: &[(&str, &str)],
) -> Vec<Diagnostic> {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace_root = temp_dir.path().join("workspace");
    let file_path = workspace_root.join("src/main/java/Test.java");
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).expect("Failed to create test workspace");
    }
    fs::write(&file_path, source).expect("Failed to write test file");

    let index = WorkspaceIndex::new(workspace_root.clone());

    for (rel_path, contents) in extra_files {
        let full_path = workspace_root.join(rel_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create extra file dir");
        }
        fs::write(&full_path, contents).expect("Failed to write extra file");
        index
            .index_file_with_source(&full_path, contents)
            .expect("Failed to index extra file");
    }

    index
        .index_file_with_source(&file_path, source)
        .expect("Failed to index test file");
    let file_id = index
        .get_file_id(&file_path)
        .expect("File ID not found in index");

    let workspace_index = Arc::new(index);
    let root = parse_java(source);

    // Match production diagnostics flow:
    // 1) unresolved-symbol diagnostics from FileSemantics
    // 2) type-checking diagnostics from TypeChecker
    let symbol_resolver = ResolverBuilder::with_workspace(file_id, Arc::clone(&workspace_index))
        .build_from_syntax(&root);
    let file_semantics = Arc::new(compute_file_semantics(
        &root,
        file_id,
        symbol_resolver.symbol_table(),
        &workspace_index,
    ));

    let mut errors = file_semantics.diagnostics().to_vec();

    let mut checker = TypeChecker::new();
    checker.set_file_semantics(Arc::clone(&file_semantics));
    errors.extend(checker.check_with_context(&root, &[], file_id, workspace_index));

    errors
        .iter()
        .map(|error| semantic_error_to_diagnostic(error, source))
        .collect()
}

fn count_code(diagnostics: &[Diagnostic], code: &str) -> usize {
    diagnostics
        .iter()
        .filter(|diag| matches!(&diag.code, Some(NumberOrString::String(s)) if s == code))
        .count()
}

fn assert_has_code(diagnostics: &[Diagnostic], code: &str) {
    assert!(
        count_code(diagnostics, code) > 0,
        "Expected diagnostic code '{code}', got: {:?}",
        diagnostics
            .iter()
            .map(|diag| diag.code.as_ref())
            .collect::<Vec<_>>()
    );
}

fn assert_error_severity(diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics
            .iter()
            .all(|diag| diag.severity == Some(DiagnosticSeverity::ERROR)),
        "Expected all diagnostics to be errors, got: {:?}",
        diagnostics
            .iter()
            .map(|diag| diag.severity)
            .collect::<Vec<_>>()
    );
}

fn assert_no_diagnostics(diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics, got: {:?}",
        diagnostics
            .iter()
            .map(|diag| diag.message.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_missing_import_reports_error() {
    let source = r"
import com.example.MissingType;

class Test {
}
";

    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        source,
        &[(
            "src/main/java/com/example/PresentType.java",
            "package com.example; public class PresentType {}",
        )],
    );

    assert_has_code(&diagnostics, "undefined-symbol");
    assert!(
        diagnostics.iter().any(|diag| diag
            .message
            .contains("cannot find symbol: com.example.MissingType")),
        "Expected missing import diagnostic, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_static_imported_member_type_resolves() {
    let source = r"
        package com.example;

        import static com.example.Outer.Nested;

        class Test {
            Nested value;
            int number = Nested;

            void run() {
                Nested();
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        source,
        &[(
            "src/main/java/com/example/Outer.java",
            concat!(
                "package com.example; public class Outer { ",
                "public static class Nested {} ",
                "public static int Nested; ",
                "public static void Nested() {} }",
            ),
        )],
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
fn test_unresolved_receiver_reports_error() {
    let source = r"
        class Test {
            void run() {
                missing.call();
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_has_code(&diagnostics, "undefined-symbol");
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.message.contains("cannot find symbol: missing")),
        "Expected undefined symbol for receiver, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Test that reassigning a var variable with incompatible type produces diagnostic
#[test]
fn test_var_reassignment_type_mismatch() {
    let source = r#"
        class Test {
            void test() {
                var foo = 1;
                foo = "Hey";
            }
        }
    "#;

    let diagnostics = semantic_diagnostics(source);
    assert_has_code(&diagnostics, "type-mismatch-assignment");
    assert_error_severity(&diagnostics);
    assert_eq!(diagnostics.len(), 1);
}

/// Test that declaring a variable with incompatible initializer produces diagnostic
#[test]
fn test_declaration_type_mismatch() {
    let source = r"
        class Test {
            void test() {
                String foo = 1;
            }
        }
    ";

    let diagnostics = semantic_diagnostics(source);
    assert_has_code(&diagnostics, "type-mismatch-declaration");
    assert_error_severity(&diagnostics);
    assert_eq!(diagnostics.len(), 1);
}

/// Test that compatible assignments don't produce diagnostics
#[test]
fn test_compatible_assignment_no_error() {
    let source = r#"
        class Test {
            void test() {
                var foo = 1;
                foo = 2;  // OK: both int

                String bar = "hello";
                bar = "world";  // OK: both String
            }
        }
    "#;

    let diagnostics = semantic_diagnostics(source);
    assert_no_diagnostics(&diagnostics);
}

/// Test widening conversions are allowed
#[test]
fn test_widening_conversion_allowed() {
    let source = r"
        class Test {
            void test() {
                long foo = 1;  // OK: int -> long widening
                double bar = 1.0f;  // OK: float -> double widening
            }
        }
    ";

    let diagnostics = semantic_diagnostics(source);
    assert_no_diagnostics(&diagnostics);
}

/// Test narrowing conversions without cast produce error
#[test]
fn test_narrowing_conversion_requires_cast() {
    let source = r"
        class Test {
            void test() {
                int foo = 1L;  // ERROR: long -> int requires cast
                float bar = 1.0;  // ERROR: double -> float requires cast
            }
        }
    ";

    let diagnostics = semantic_diagnostics(source);
    assert_error_severity(&diagnostics);
    assert_eq!(count_code(&diagnostics, "type-mismatch-declaration"), 2);
    assert_eq!(diagnostics.len(), 2);
}

/// Test null assignment to reference types
#[test]
fn test_null_assignment_to_reference() {
    let source = r"
        class Test {
            void test() {
                String foo = null;  // OK
                Integer bar = null;  // OK
                int baz = null;  // ERROR: can't assign null to primitive
            }
        }
    ";

    let diagnostics = semantic_diagnostics(source);
    assert_has_code(&diagnostics, "null-to-primitive");
    assert_error_severity(&diagnostics);
    assert_eq!(diagnostics.len(), 1);
}

/// Test subtype assignment is allowed
#[test]
fn test_subtype_assignment() {
    let source = r"
        package com.example;

        class Parent {}
        class Child extends Parent {}

        class Test {
            void test() {
                Parent p = new Child();  // OK: Child is subtype of Parent
                Child c = new Parent();  // ERROR: Parent is not subtype of Child
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_has_code(&diagnostics, "type-mismatch-declaration");
    assert_error_severity(&diagnostics);
    assert_eq!(diagnostics.len(), 1);
}

/// Test interface implementation assignment
#[test]
fn test_interface_assignment() {
    let source = r"
        package com.example;

        interface TaskRunner {
            void run();
        }

        class MyTaskRunner implements TaskRunner {
            public void run() {}
        }

        class Test {
            void test() {
                TaskRunner r = new MyTaskRunner();  // OK
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_no_diagnostics(&diagnostics);
}

/// Test var inference prevents type mismatch
#[test]
fn test_var_prevents_incompatible_reassignment() {
    let source = r#"
        class Test {
            void test() {
                var x = 1;      // x inferred as int
                x = 2;          // OK
                x = "hello";    // ERROR: can't assign String to int
            }
        }
    "#;

    let diagnostics = semantic_diagnostics(source);
    assert_has_code(&diagnostics, "type-mismatch-assignment");
    assert_error_severity(&diagnostics);
    assert_eq!(diagnostics.len(), 1);
}

/// Test autoboxing/unboxing
#[test]
fn test_autoboxing_unboxing() {
    let source = r"
        package com.example;

        class Test {
            void test() {
                Integer i = 1;  // OK: autoboxing int -> Integer
                int j = new Integer(2);  // OK: unboxing Integer -> int
            }
        }
    ";

    let diagnostics = semantic_diagnostics(source);
    assert_no_diagnostics(&diagnostics);
}

/// Test generic interface assignment
#[test]
fn test_generic_interface_assignment() {
    let source = r"
        package com.example;

        interface MyInterface<T> {
            T getValue();
        }

        class MyImplementation<T> implements MyInterface<T> {
            private T value;

            public T getValue() {
                return value;
            }
        }

        class Main {
            void test() {
                // This should work: MyImplementation<String> implements MyInterface<String>
                MyInterface<String> myVar = new MyImplementation<>();
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_no_diagnostics(&diagnostics);
}

/// Test that literal method calls are correctly typed when used as arguments
/// Bug: "str".length() was being inferred as String instead of int
/// because argument extraction took the FIRST expression (LITERAL_EXPR)
/// instead of the LAST (METHOD_CALL_EXPR) in flat AST structures.
#[test]
fn test_literal_method_call_as_argument() {
    // Use user-defined classes since JDK isn't indexed in tests
    let source = r"
        package com.example;

        class StringWrapper {
            public int length() { return 0; }
        }

        class Consumer {
            public void consume(int value) {}
            public void consume(String value) {}
        }

        class Test {
            void test() {
                StringWrapper wrapper = new StringWrapper();
                Consumer consumer = new Consumer();
                // wrapper.length() returns int, so this should call consume(int)
                consumer.consume(wrapper.length());
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    // Should have NO type mismatch errors
    assert_no_diagnostics(&diagnostics);
}

/// Regression test: chained method calls with literals should correctly infer types
/// This tests the specific bug where "literal".method() was typed as the literal type
#[test]
fn test_chained_method_call_argument_type() {
    let source = r"
        package com.example;

        class Processor {
            public int getLength() { return 0; }
        }

        class Calculator {
            public int calculate(int input) { return input; }
        }

        class Test {
            void test() {
                Processor proc = new Processor();
                Calculator calc = new Calculator();
                // proc.getLength() returns int - this should NOT error
                int result = calc.calculate(proc.getLength());
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_no_diagnostics(&diagnostics);
}

/// Regression: parse_type_from_string used to drop trailing `[]` from method
/// return types, so `byte[] x; x = foo.method();` (where method returns byte[])
/// reported "incompatible types: byte cannot be converted to byte[]".
#[test]
fn test_interface_method_returning_byte_array_assigned_to_byte_array() {
    let source = r"
        package com.example;

        class Test {
            void use(Foo f) {
                byte[] x;
                x = f.method();
            }
        }
    ";

    let foo_iface = r"
        package com.example;

        public interface Foo {
            byte[] method();
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        source,
        &[("src/main/java/com/example/Foo.java", foo_iface)],
    );

    assert_eq!(
        count_code(&diagnostics, "type-mismatch-assignment"),
        0,
        "byte[] x = f.method() (method returns byte[]) should not be a mismatch, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Same bug, class method (not interface) returning a reference array.
/// Confirms the fix is not interface-specific.
#[test]
fn test_class_method_returning_string_array_assigned_to_string_array() {
    let source = r"
        package com.example;

        class Provider {
            public String[] names() { return null; }
        }

        class Test {
            void use() {
                Provider p = new Provider();
                String[] xs;
                xs = p.names();
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_eq!(
        count_code(&diagnostics, "type-mismatch-assignment"),
        0,
        "String[] xs = p.names() (method returns String[]) should not be a mismatch, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Multi-dimensional array return type round-trips through parse_type_from_string.
#[test]
fn test_method_returning_int_2d_array_assigned() {
    let source = r"
        package com.example;

        class Grid {
            public int[][] cells() { return null; }
        }

        class Test {
            void use() {
                Grid g = new Grid();
                int[][] xs;
                xs = g.cells();
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_eq!(
        count_code(&diagnostics, "type-mismatch-assignment"),
        0,
        "int[][] xs = g.cells() (method returns int[][]) should not be a mismatch, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Negative case: assigning a byte[] return value to a non-array still flags.
/// Guards against the fix becoming over-permissive.
#[test]
fn test_method_returning_byte_array_assigned_to_byte_still_errors() {
    let source = r"
        package com.example;

        class Provider {
            public byte[] data() { return null; }
        }

        class Test {
            void use() {
                Provider p = new Provider();
                byte x;
                x = p.data();
            }
        }
    ";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert!(
        count_code(&diagnostics, "type-mismatch-assignment") >= 1,
        "byte = byte[] should still flag a mismatch, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Regression (Board.java-style case): assigning a same-package array
/// parameter to a same-package array field must not flag. The field type
/// stays simple-name (`CellType[][]`) while the parameter resolves to the
/// FQN (`org.example.CellType[][]`); array element comparison must resolve
/// both through the workspace before deciding they differ.
#[test]
fn test_same_package_array_field_assignment_no_mismatch() {
    let source = r"
package org.example;

public class Board {
    private CellType[][] cells;

    public Board(CellType[][] cells) {
        this.cells = cells;
    }
}
";

    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        source,
        &[(
            "src/main/java/org/example/CellType.java",
            "package org.example; public enum CellType { Empty, Pink }",
        )],
    );

    assert_eq!(
        count_code(&diagnostics, "type-mismatch-assignment"),
        0,
        "this.cells = cells (both CellType[][] in org.example) should not be a mismatch, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A method call on a generic receiver whose declared type binds the type
/// parameter must substitute the type variable in the return type before the
/// type checker compares it to the declared variable type.
///
/// Repro: `Holder<T>` declares `T fetch()`. Calling `holder.fetch()` where
/// `holder` is declared as `Holder<Item>` should yield `Item`, not raw `T`.
/// Pre-fix, the inference engine returned `T`, so `Item it = holder.fetch();`
/// reported "incompatible types: T cannot be converted to Item".
#[test]
fn test_method_returning_type_param_on_parameterized_field_no_mismatch() {
    let source = r"
        package demo.app;

        public class Consumer {
            private final Holder<Item> holder = null;

            public Item run() {
                Item it = holder.fetch();
                return it;
            }
        }
    ";

    let holder = r"
        package demo.app;

        public class Holder<T> {
            public T fetch() {
                return null;
            }
        }
    ";

    let item = r"
        package demo.app;

        public class Item {}
    ";

    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        source,
        &[
            ("src/main/java/demo/app/Holder.java", holder),
            ("src/main/java/demo/app/Item.java", item),
        ],
    );

    assert_eq!(
        count_code(&diagnostics, "type-mismatch-declaration"),
        0,
        "holder.fetch() on Holder<Item> should yield Item, not raw T. Got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ===========================================================================
// Name-resolution regressions
// ===========================================================================

/// `Foo.bar()` parses as METHOD_CALL_EXPR[NAME_EXPR, `.`, IDENT, ARG_LIST], so
/// the receiver is a direct NAME_EXPR child rather than a FIELD_ACCESS_EXPR
/// chain. Only chain receivers were allowed to denote a type from an unindexed
/// dependency, so every static call on an imported third-party class reported
/// "cannot find symbol" on the class itself.
#[test]
fn test_static_call_on_imported_external_class_is_not_reported() {
    let source = r"
import com.ext.Ext;

class Test {
    void run() {
        Ext.doIt();
    }
}
";

    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

/// `Foo::bar` puts the qualifier in the same position as `Foo.bar()`, so it
/// needs the same allowance: `StringBuilder::new` was reported as an
/// unresolved symbol.
#[test]
fn test_method_reference_qualifier_is_not_reported() {
    let source = r"
import com.ext.Ext;

class Test {
    void run() {
        use(Ext::create);
        use(StringBuilder::new);
    }

    void use(Object action) {
    }
}
";

    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

/// Argument extraction only counted nodes on an allow-list of expression
/// kinds, and constructor references were not on it, so a correct call with a
/// `Foo::new` argument was reported as having zero arguments.
#[test]
fn test_constructor_reference_argument_is_counted() {
    let source = r"
class Test {
    void run() {
        use(StringBuilder::new);
        use(o instanceof String);
        use(switch (1) { default -> 0; });
    }

    void use(Object value) {
    }
}
";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_eq!(
        count_code(&diagnostics, "wrong-argument-count"),
        0,
        "Expected no argument-count diagnostics, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_static_call_on_java_lang_class_without_indexed_jdk() {
    let source = r"
class Test {
    void run() {
        String value = String.valueOf(1);
    }
}
";

    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

/// The external-type allowance must not swallow genuine typos. A receiver in a
/// packaged compilation unit always has a same-package candidate, and that
/// package is indexed, so nothing suppresses the diagnostic.
#[test]
fn test_unresolved_receiver_in_packaged_file_still_reports_error() {
    let source = r"
package com.example;

class Test {
    void run() {
        missing.call();
    }
}
";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_has_code(&diagnostics, "undefined-symbol");
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.message.contains("cannot find symbol: missing")),
        "Expected undefined symbol for receiver, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Declarations defined their own symbol and stopped, so a lambda in an
/// initializer never reached the symbol table and its parameter was reported
/// as an undefined symbol at every use.
#[test]
fn test_lambda_parameter_in_local_initializer_resolves() {
    let source = r"
import java.util.function.Function;

class Test {
    void run() {
        Function<String, String> trim = value -> value.trim();
    }
}
";

    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_lambda_parameter_in_field_initializer_resolves() {
    let source = r"
import java.util.function.Function;

class Test {
    Function<String, String> trim = value -> value.trim();
}
";

    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_lambda_parameter_with_block_body_resolves() {
    let source = r"
import java.util.function.Function;

class Test {
    void run() {
        Function<String, String> trim = value -> {
            return value.trim();
        };
    }
}
";

    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

/// PARAM was a leaf in the semantic walk, so a parameter's declared type was
/// never resolved: no navigation target, and an unknown type went unreported
/// even though the identical type in a local declaration was flagged.
#[test]
fn test_unknown_parameter_type_is_reported() {
    let source = r"
class Test {
    void run(MissingType value) {
    }
}
";

    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_has_code(&diagnostics, "undefined-symbol");
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.message.contains("cannot find symbol: MissingType")),
        "Expected undefined symbol for parameter type, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_known_parameter_type_is_not_reported() {
    let source = r"
import com.example.Helper;

class Test {
    void run(Helper helper) {
    }
}
";

    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        source,
        &[(
            "src/main/java/com/example/Helper.java",
            "package com.example; public class Helper {}",
        )],
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
fn test_inherited_interface_method_arguments_and_casts() {
    let source = r"
        package demo;
        interface Root {}
        interface Task extends Root {}
        class Base implements Task {}
        class Child extends Base {}
        class Sink {
            void root(Root value) {}
            void base(Base value) {}
            void roots(Root... values) {}
        }
        class Test {
            void run(Sink sink, Child child, Object object) {
                sink.root(child);
                sink.base(child);
                sink.root(new Child());
                sink.root((Child) object);
                sink.root((Task) object);
                sink.roots(child, new Child());
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_bounded_subclass_and_interface_arguments() {
    let source = r"
        package demo;
        interface Task {}
        class Base {}
        class Sink {
            void task(Task value) {}
            void base(Base value) {}
        }
        class Test {
            <T extends Base & Task> void run(Sink sink, T value) {
                sink.task(value);
                sink.base(value);
                Task task = value;
                Base base = value;
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_generic_subtype_arguments_preserve_parameterization() {
    let source = r"
        package demo;
        class Item {}
        class Other {}
        interface Box<T> {}
        class Base<T> implements Box<T> {}
        class Child extends Base<Item> {}
        class Sink { void accept(Box<Other> value) {} }
        class Test {
            void run(Sink sink, Child child) {
                sink.accept(child);
                Box<Other> wrong = child;
            }
        }
    ";
    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_eq!(
        count_code(&diagnostics, "method-argument-type-mismatch"),
        1,
        "{diagnostics:#?}"
    );
    assert_eq!(
        count_code(&diagnostics, "type-mismatch-declaration"),
        1,
        "{diagnostics:#?}"
    );
}

#[test]
fn test_reference_array_conversions_in_assignments() {
    let source = r"
        class Test {
            void run(int[][] primitiveRows, String[][] referenceRows, int[] numbers) {
                Object[] a = primitiveRows;
                Object[] b = referenceRows;
                Cloneable c = numbers;
                java.io.Serializable d = numbers;
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_cast_selects_subtype_overload_return_type() {
    let source = r"
        package demo;
        interface Task {}
        class Child implements Task {}
        class Result {}
        class Sink {
            Result accept(Task value) { return null; }
            int accept(Object value) { return 0; }
        }
        class Test {
            void run(Sink sink, Object object) {
                Result first = sink.accept((Child) object);
                Result second = sink.accept(new Child());
                Result third = sink.accept((Task) object);
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_generic_subtype_valid_arguments_and_wildcards() {
    let source = r"
        package demo;
        class Item {}
        class Special extends Item {}
        interface Box<T> {}
        class Base<T> implements Box<T> {}
        class Child extends Base<Special> {}
        class Sink {
            void exact(Box<Special> value) {}
            void upper(Box<? extends Item> value) {}
            void lower(Box<? super Special> value) {}
        }
        class Test {
            void run(Sink sink, Child child, Base<Item> parent) {
                sink.exact(child);
                sink.upper(child);
                sink.lower(parent);
                Box<Special> exact = child;
                Box<? extends Item> upper = child;
                Box<? super Special> lower = parent;
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_invalid_array_and_bound_conversions_remain_errors() {
    let source = r"
        package demo;
        interface Task {}
        class Other {}
        class Sink { void task(Task value) {} }
        class Test {
            <T extends Other> void run(Sink sink, T value, int[] numbers, Integer[] boxed) {
                sink.task(value);
                Task task = value;
                Object[] objects = numbers;
                long[] widened = numbers;
                int[] unboxed = boxed;
            }
        }
    ";
    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_eq!(
        count_code(&diagnostics, "method-argument-type-mismatch"),
        1,
        "{diagnostics:#?}"
    );
    assert_eq!(
        count_code(&diagnostics, "type-mismatch-declaration"),
        4,
        "{diagnostics:#?}"
    );
}

#[test]
fn test_transitive_and_shadowed_type_parameter_bounds() {
    let source = r"
        package demo;
        interface Task {}
        interface Recursive<T> {}
        class Other {}
        class Sink { void task(Task value) {} }
        class Test<T extends Other> {
            <T extends Task, U extends T> void run(Sink sink, U value) {
                sink.task(value);
                Task task = value;
                T parent = value;
            }
            <T extends Task & Recursive<T>> void recursive(Sink sink, T value) {
                sink.task(value);
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_intersection_cast_retains_all_interface_components() {
    let source = r"
        package demo;
        interface Left {}
        interface Right {}
        class Result {}
        class Sink {
            Result accept(Right value) { return null; }
            int accept(Object value) { return 0; }
        }
        class Test {
            void run(Sink sink, Object value) {
                Result result = sink.accept((Left & Right) value);
                Right right = (Left & Right) value;
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_array_casts_preserve_dimensions_and_component_identity() {
    let source = r"
        package demo;
        interface Task {}
        class Child implements Task {}
        class Result {}
        class Sink {
            Result accept(Task[] values) { return null; }
            int accept(Object value) { return 0; }
        }
        class Test {
            void run(Sink sink, Object value) {
                Result result = sink.accept((Child[]) value);
                Task[] tasks = (Child[]) value;
                Object[] rows = (int[][]) value;
                int[] numbers = (int[]) value;
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_generic_subtype_preserves_lexical_type_variables() {
    let source = r"
        package demo;
        interface Task {}
        interface Box<T> {}
        class Base<T> implements Box<T> {}
        class Sink<T> {
            void accept(Box<T> value) {}
            Box<T> echo(Box<T> value) { return value; }
        }
        class Test {
            <T extends Task> void run(Sink<T> sink, Base<T> value) {
                sink.accept(value);
                Box<T> box = value;
                Box<? extends T> upper = value;
                Box<T> echoed = sink.echo(value);
            }
        }
    ";
    assert_no_diagnostics(&semantic_diagnostics_with_workspace(source));
}

#[test]
fn test_diamond_cannot_change_fixed_interface_type_arguments() {
    let source = r"
        package demo;
        class Item {}
        class Other {}
        interface Box<T> {}
        class Flexible<T> implements Box<T> {}
        class Fixed<T> implements Box<Other> {}
        class Sink { void accept(Box<Item> value) {} }
        class Test {
            void run(Sink sink) {
                sink.accept(new Flexible<>());
                sink.accept(new Fixed<>());
            }
        }
    ";
    let diagnostics = semantic_diagnostics_with_workspace(source);
    assert_eq!(
        count_code(&diagnostics, "method-argument-type-mismatch"),
        1,
        "{diagnostics:#?}"
    );
}

#[test]
fn enum_class_literals_are_valid_in_method_arguments_and_annotations() {
    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        r"
import model.SomeEnum;
@interface Uses { Class<?> value(); }
@Uses(SomeEnum.class)
class Test {
    void accept(Class<?> type) {}
    void run() {
        accept(SomeEnum.class);
        accept(model.SomeEnum.class);
        accept(SomeEnum[].class);
    }
}
",
        &[(
            "model/SomeEnum.java",
            "package model; public enum SomeEnum { ONE }",
        )],
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

/// Lombok logger annotations declare a `log` field. Reading it must
/// not report `cannot find symbol: log`, for every supported annotation.
#[test]
fn lombok_logger_annotations_declare_the_log_field() {
    for annotation in [
        "Slf4j",
        "Log4j2",
        "CommonsLog",
        "Log",
        "JBossLog",
        "Flogger",
        "XSlf4j",
    ] {
        let source = format!(
            "import lombok.{annotation};\n@{annotation}\nclass Test {{\n    void run() {{ log.info(\"x\"); }}\n}}\n"
        );
        let diagnostics = semantic_diagnostics_with_workspace(&source);
        assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.message.contains("cannot find symbol: log")),
            "@{annotation} must declare `log`, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    // Without the annotation the name is genuinely unresolved.
    let diagnostics = semantic_diagnostics_with_workspace(
        "class Test {\n    void run() { log.info(\"x\"); }\n}\n",
    );
    assert!(diagnostics
        .iter()
        .any(|diag| diag.message.contains("cannot find symbol: log")));
}

/// A statically imported, overloaded method (`assertEquals`) is one
/// callable name; it must resolve even though several signatures match.
#[test]
fn statically_imported_overloads_are_not_reported_unresolved() {
    let diagnostics = semantic_diagnostics_with_workspace_and_files(
        r"
import static lib.Assertions.assertEquals;
class Test {
    void run() {
        assertEquals(1, 2);
        assertEquals(1.0, 2.0, 0.1);
    }
}
",
        &[(
            "lib/Assertions.java",
            "package lib; public class Assertions { \
             public static void assertEquals(int a, int b) {} \
             public static void assertEquals(Object a, Object b) {} \
             public static void assertEquals(double a, double b, double d) {} }",
        )],
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.message.contains("cannot find symbol")),
        "{diagnostics:#?}"
    );
}
