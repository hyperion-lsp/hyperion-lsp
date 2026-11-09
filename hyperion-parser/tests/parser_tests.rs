// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for the Java parser
//!
//! This test suite covers:
//! - Package and import declarations
//! - Class, interface, enum, and record declarations
//! - Field and method declarations
//! - Statements (if, while, for, switch, try, etc.)
//! - Expressions (binary, unary, method calls, etc.)
//! - Type parsing (primitives, generics, arrays)
//! - Error recovery
//! - Complex real-world code

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::{AstNode, SourceFile, SyntaxKind, SyntaxNode};

/// Helper function to parse source code and return the root node
fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

/// Helper function to parse and expect no errors
fn parse_ok(source: &str) -> SyntaxNode {
    let (node, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");
    node
}

/// Helper function to check that parsing produces errors
fn parse_err(source: &str) {
    let (_, errors) = parse(source);
    assert!(!errors.is_empty(), "Expected parse errors but got none");
}

// ============================================================================
// Basic Parsing Tests
// ============================================================================

#[test]
fn test_empty_source() {
    let root = parse_ok("");
    assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
}

#[test]
fn test_source_file_node() {
    let root = parse_ok("class Foo {}");
    assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);

    let source_file = SourceFile::cast(root).unwrap();
    assert_eq!(source_file.types().count(), 1);
}

// ============================================================================
// Package and Import Declaration Tests
// ============================================================================

#[test]
fn test_package_declaration() {
    let root = parse_ok("package com.example.app;");
    let source_file = SourceFile::cast(root).unwrap();

    let package = source_file.package();
    assert!(package.is_some(), "Package declaration should be present");
}

#[test]
fn test_package_with_single_name() {
    let root = parse_ok("package app;");
    let source_file = SourceFile::cast(root).unwrap();
    assert!(source_file.package().is_some());
}

#[test]
fn test_simple_import() {
    let root = parse_ok("import java.util.List;");
    let source_file = SourceFile::cast(root).unwrap();

    assert_eq!(source_file.imports().count(), 1);
}

#[test]
fn test_wildcard_import() {
    let root = parse_ok("import java.util.*;");
    let source_file = SourceFile::cast(root).unwrap();

    let import = source_file.imports().next().unwrap();
    assert!(import.is_wildcard());
}

#[test]
fn test_static_import() {
    let root = parse_ok("import static java.lang.Math.PI;");
    let source_file = SourceFile::cast(root).unwrap();

    let import = source_file.imports().next().unwrap();
    assert!(import.is_static());
}

#[test]
fn test_multiple_imports() {
    let source = r"
        import java.util.List;
        import java.util.ArrayList;
        import java.io.*;
    ";
    let root = parse_ok(source);
    let source_file = SourceFile::cast(root).unwrap();

    assert_eq!(source_file.imports().count(), 3);
}

#[test]
fn test_package_and_imports() {
    let source = r"
        package com.example;
        import java.util.*;
        class Foo {}
    ";
    let root = parse_ok(source);
    let source_file = SourceFile::cast(root).unwrap();

    assert!(source_file.package().is_some());
    assert_eq!(source_file.imports().count(), 1);
    assert_eq!(source_file.types().count(), 1);
}

// ============================================================================
// Class Declaration Tests
// ============================================================================

#[test]
fn test_empty_class() {
    let root = parse_ok("class Foo {}");
    let source_file = SourceFile::cast(root).unwrap();

    assert_eq!(source_file.types().count(), 1);
}

#[test]
fn test_public_class() {
    let root = parse_ok("public class Foo {}");
    let source_file = SourceFile::cast(root).unwrap();

    let class = source_file.types().next().unwrap();
    let class_name = class.name().unwrap();
    assert_eq!(class_name.text(), "Foo");
}

#[test]
fn test_class_with_modifiers() {
    let _root = parse_ok("public final class Foo {}");
    parse_ok("abstract class Bar {}");
    parse_ok("public abstract class Baz {}");
}

#[test]
fn test_class_with_extends() {
    let root = parse_ok("class Foo extends Bar {}");
    let source_file = SourceFile::cast(root).unwrap();

    // Verify it parses without errors
    assert_eq!(source_file.types().count(), 1);
}

#[test]
fn test_class_with_implements() {
    parse_ok("class Foo implements Bar {}");
    parse_ok("class Foo implements Bar, Baz {}");
    parse_ok("class Foo implements Bar, Baz, Qux {}");
}

#[test]
fn test_class_with_extends_and_implements() {
    let source = "class Foo extends Bar implements Baz, Qux {}";
    parse_ok(source);
}

#[test]
fn test_class_with_type_parameters() {
    parse_ok("class Foo<T> {}");
    parse_ok("class Foo<T, U> {}");
    parse_ok("class Foo<T extends Number> {}");
    parse_ok("class Foo<T extends Number & Comparable<T>> {}");
}

#[test]
fn test_nested_class() {
    let source = r"
        class Outer {
            class Inner {}
        }
    ";
    parse_ok(source);
}

#[test]
fn test_static_nested_class() {
    let source = r"
        class Outer {
            static class StaticNested {}
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Interface Declaration Tests
// ============================================================================

#[test]
fn test_empty_interface() {
    parse_ok("interface Foo {}");
}

#[test]
fn test_interface_with_extends() {
    parse_ok("interface Foo extends Bar {}");
    parse_ok("interface Foo extends Bar, Baz {}");
}

#[test]
fn test_interface_with_type_parameters() {
    parse_ok("interface Foo<T> {}");
    parse_ok("interface Foo<T extends List<String>> {}");
}

// ============================================================================
// Enum Declaration Tests
// ============================================================================

#[test]
fn test_empty_enum() {
    parse_ok("enum Color {}");
}

#[test]
fn test_enum_with_implements() {
    parse_ok("enum Color implements Serializable {}");
}

// ============================================================================
// Record Declaration Tests (Java 14+)
// ============================================================================

#[test]
fn test_empty_record() {
    parse_ok("record Point() {}");
}

#[test]
fn test_record_with_components() {
    parse_ok("record Point(int x, int y) {}");
}

#[test]
fn test_record_with_generics() {
    parse_ok("record Pair<T, U>(T first, U second) {}");
}

#[test]
fn test_record_with_implements() {
    parse_ok("record Point(int x, int y) implements Serializable {}");
}

// ============================================================================
// Annotation Type Declaration Tests
// ============================================================================

#[test]
fn test_annotation_type() {
    parse_ok("@interface MyAnnotation {}");
}

#[test]
fn test_annotation_type_with_modifier() {
    parse_ok("public @interface MyAnnotation {}");
}

// ============================================================================
// Field Declaration Tests
// ============================================================================

#[test]
fn test_simple_field() {
    let source = r"
        class Foo {
            int x;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_field_with_initializer() {
    let source = r"
        class Foo {
            int x = 42;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_multiple_fields_in_declaration() {
    let source = r"
        class Foo {
            int x, y, z;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_field_with_modifiers() {
    let source = r"
        class Foo {
            public static final int CONSTANT = 100;
            private volatile boolean flag;
            protected transient String name;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_array_field() {
    let source = r"
        class Foo {
            int[] arr;
            String[][] matrix;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_generic_field() {
    let source = r"
        class Foo {
            List<String> names;
            Map<String, Integer> counts;
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Method Declaration Tests
// ============================================================================

#[test]
fn test_simple_method() {
    let source = r"
        class Foo {
            void bar() {}
        }
    ";
    parse_ok(source);
}

#[test]
fn test_method_with_return_type() {
    let source = r"
        class Foo {
            int getNumber() {
                return 42;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_method_with_parameters() {
    let source = r"
        class Foo {
            void method(int x, String s) {}
        }
    ";
    parse_ok(source);
}

#[test]
fn test_method_with_varargs() {
    let source = r"
        class Foo {
            void method(String... args) {}
            void method2(int x, String... args) {}
        }
    ";
    parse_ok(source);
}

#[test]
fn test_method_with_modifiers() {
    let source = r"
        class Foo {
            public static void staticMethod() {}
            private final int finalMethod() { return 0; }
            protected synchronized void syncMethod() {}
        }
    ";
    parse_ok(source);
}

#[test]
fn test_method_with_throws() {
    let source = r"
        class Foo {
            void method() throws IOException {}
            void method2() throws IOException, SQLException {}
        }
    ";
    parse_ok(source);
}

#[test]
fn test_generic_method() {
    let source = r"
        class Foo {
            <T> T identity(T value) {
                return value;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_abstract_method() {
    let source = r"
        abstract class Foo {
            abstract void method();
        }
    ";
    parse_ok(source);
}

#[test]
fn test_method_with_array_parameters() {
    let source = r"
        class Foo {
            void process(int[] arr) {}
            void process2(String[][] matrix) {}
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Statement Tests
// ============================================================================

#[test]
fn test_empty_statement() {
    let source = r"
        class Foo {
            void bar() {
                ;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_expression_statement() {
    let source = r"
        class Foo {
            void bar() {
                x = 5;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_local_variable_declaration() {
    let source = r#"
        class Foo {
            void bar() {
                int x = 42;
                String s = "hello";
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_if_statement() {
    let source = r"
        class Foo {
            void bar() {
                if (true) {
                    x = 1;
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_if_else_statement() {
    let source = r"
        class Foo {
            void bar() {
                if (condition) {
                    x = 1;
                } else {
                    x = 2;
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_if_else_if_chain() {
    let source = r"
        class Foo {
            void bar() {
                if (x == 1) {
                    a();
                } else if (x == 2) {
                    b();
                } else {
                    c();
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_while_loop() {
    let source = r"
        class Foo {
            void bar() {
                while (condition) {
                    doSomething();
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_do_while_loop() {
    let source = r"
        class Foo {
            void bar() {
                do {
                    doSomething();
                } while (condition);
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_for_loop() {
    let source = r"
        class Foo {
            void bar() {
                for (int i = 0; i < 10; i++) {
                    doSomething();
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_enhanced_for_loop() {
    let source = r"
        class Foo {
            void bar() {
                for (String s : list) {
                    process(s);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_return_statement() {
    let source = r"
        class Foo {
            int bar() {
                return 42;
            }
            void baz() {
                return;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_break_continue() {
    let source = r"
        class Foo {
            void bar() {
                while (true) {
                    if (x) break;
                    if (y) continue;
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_throw_statement() {
    let source = r#"
        class Foo {
            void bar() {
                throw new RuntimeException("error");
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_try_catch() {
    let source = r"
        class Foo {
            void bar() {
                try {
                    riskyOperation();
                } catch (IOException e) {
                    handleError(e);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_try_catch_finally() {
    let source = r"
        class Foo {
            void bar() {
                try {
                    riskyOperation();
                } catch (IOException e) {
                    handleError(e);
                } finally {
                    cleanup();
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_try_multi_catch() {
    let source = r"
        class Foo {
            void bar() {
                try {
                    riskyOperation();
                } catch (IOException e) {
                    handleIO(e);
                } catch (SQLException e) {
                    handleSQL(e);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_synchronized_statement() {
    let source = r"
        class Foo {
            void bar() {
                synchronized (lock) {
                    criticalSection();
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_assert_statement() {
    let source = r#"
        class Foo {
            void bar() {
                assert condition;
                assert x > 0 : "x must be positive";
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_switch_statement() {
    let source = r"
        class Foo {
            void bar() {
                switch (x) {
                    case 1:
                        doOne();
                        break;
                    case 2:
                        doTwo();
                        break;
                    default:
                        doDefault();
                }
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Expression Tests (Basic)
// ============================================================================

#[test]
fn test_literal_expressions() {
    let source = r#"
        class Foo {
            void bar() {
                int a = 42;
                boolean b = true;
                String s = "hello";
                char c = 'x';
                Object o = null;
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_binary_expressions() {
    let source = r"
        class Foo {
            void bar() {
                int x = a + b;
                int y = a - b;
                int z = a * b;
                int w = a / b;
                int m = a % b;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_comparison_expressions() {
    let source = r"
        class Foo {
            void bar() {
                boolean a = x == y;
                boolean b = x != y;
                boolean c = x < y;
                boolean d = x > y;
                boolean e = x <= y;
                boolean f = x >= y;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_logical_expressions() {
    let source = r"
        class Foo {
            void bar() {
                boolean a = x && y;
                boolean b = x || y;
                boolean c = !x;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_assignment_expressions() {
    let source = r"
        class Foo {
            void bar() {
                x = 5;
                x += 1;
                x -= 1;
                x *= 2;
                x /= 2;
                x %= 3;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_increment_decrement() {
    let source = r"
        class Foo {
            void bar() {
                x++;
                ++x;
                x--;
                --x;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_ternary_expression() {
    let source = r"
        class Foo {
            void bar() {
                int x = condition ? 1 : 0;
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Type Parsing Tests
// ============================================================================

#[test]
fn test_primitive_types() {
    let source = r"
        class Foo {
            byte b;
            short s;
            int i;
            long l;
            float f;
            double d;
            boolean bool;
            char c;
            void method() {}
        }
    ";
    parse_ok(source);
}

#[test]
fn test_array_types() {
    let source = r"
        class Foo {
            int[] arr1;
            int[][] arr2;
            String[] strings;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_generic_types() {
    let source = r"
        class Foo {
            List<String> list;
            Map<String, Integer> map;
            List<List<String>> nested;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_wildcard_types() {
    let source = r"
        class Foo {
            List<?> any;
            List<? extends Number> numbers;
            List<? super Integer> ints;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_var_type() {
    let source = r"
        class Foo {
            void bar() {
                var x = 10;
                var list = new ArrayList<String>();
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Annotation Tests
// ============================================================================

#[test]
fn test_simple_annotation() {
    let source = r"
        @Override
        class Foo {}
    ";
    parse_ok(source);
}

#[test]
fn test_annotation_with_single_value() {
    let source = r#"
        @SuppressWarnings("unchecked")
        class Foo {}
    "#;
    parse_ok(source);
}

#[test]
fn test_multiple_annotations() {
    let source = r"
        @Override
        @Deprecated
        class Foo {}
    ";
    parse_ok(source);
}

#[test]
fn test_method_annotation() {
    let source = r#"
        class Foo {
            @Override
            public String toString() {
                return "";
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_field_annotation() {
    let source = r"
        class Foo {
            @Deprecated
            private int oldField;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_annotation_with_text_block() {
    // Start with simple case: single-line text block
    let source = r#"
        class Example {
            @Doc(value = """hello""")
            void test() {}
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_annotation_text_block_with_quotes() {
    let source = r#"
        class Example {
            @ApiDoc("""
                This is a "quoted string" inside a text block.
                It can also contain 'single quotes' and special chars: \n \t
                """)
            public void method() {}
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_annotation_multiline_text_block() {
    // Nested annotation arguments containing a multiline text block
    let source = r#"
        @Operation(
            summary = "Add Book",
            description = """
                Place a new book on the selected shelf.
                Multi-line description.
                """
        )
        public void addBook() {}
    "#;
    parse_ok(source);
}

// ============================================================================
// Error Recovery Tests
// ============================================================================

#[test]
fn test_missing_semicolon() {
    // Should still parse but report errors
    parse_err("class Foo { int x }");
}

#[test]
fn test_missing_brace() {
    parse_err("class Foo {");
}

#[test]
fn test_unexpected_token() {
    parse_err("class Foo { @@@ }");
}

#[test]
fn test_incomplete_method() {
    // Missing body
    parse_err("class Foo { void bar() }");
}

#[test]
fn test_malformed_expression() {
    parse_err("class Foo { void bar() { int x = ; } }");
}

// ============================================================================
// Complex Real-World Code Tests
// ============================================================================

#[test]
fn test_hello_world() {
    let source = r#"
        package com.example;

        public class HelloWorld {
            public static void main(String[] args) {
                System.out.println("Hello, World!");
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_generic_class_with_methods() {
    let source = r"
        public class Container<T> {
            private T value;

            public Container(T value) {
                this.value = value;
            }

            public T get() {
                return value;
            }

            public void set(T value) {
                this.value = value;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_interface_with_default_methods() {
    let source = r#"
        public interface Processor {
            void process();

            default void beforeProcess() {
                System.out.println("Before");
            }

            static void info() {
                System.out.println("Processor interface");
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_enum_with_constants_and_methods() {
    let source = r"
        public enum Day {
            MONDAY,
            TUESDAY,
            WEDNESDAY;

            public boolean isWeekend() {
                return this == MONDAY;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_nested_classes_and_interfaces() {
    let source = r"
        public class Outer {
            private int field;

            public class Inner {
                public void access() {
                    field = 10;
                }
            }

            public static class StaticNested {
                public void method() {}
            }

            public interface NestedInterface {
                void method();
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_lambda_expressions() {
    let source = r#"
        class Foo {
            void bar() {
                Runnable r = () -> System.out.println("Hello");
                Function<String, Integer> f = s -> s.length();
                BiFunction<Integer, Integer, Integer> add = (a, b) -> a + b;
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_method_references() {
    let source = r"
        class Foo {
            void bar() {
                Function<String, Integer> f = String::length;
                Supplier<List> s = ArrayList::new;
                Consumer<String> c = System.out::println;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_stream_operations() {
    let source = r"
        class Foo {
            void bar() {
                list.stream()
                    .filter(x -> x > 0)
                    .map(x -> x * 2)
                    .collect(Collectors.toList());
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_try_with_resources() {
    let source = r#"
        class Foo {
            void bar() {
                try (BufferedReader br = new BufferedReader(new FileReader("file.txt"))) {
                    String line = br.readLine();
                } catch (IOException e) {
                    e.printStackTrace();
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_try_with_resources_final_modifier() {
    let source = r#"
        class Foo {
            void bar() {
                try (final ResultSet rs = ps.executeQuery()) {
                    while (rs.next()) {
                        String name = rs.getString(1);
                    }
                } catch (SQLException e) {
                    e.printStackTrace();
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_try_with_resources_multiple_with_final() {
    let source = r#"
        class Foo {
            void bar() {
                try (final Connection conn = getConnection();
                     final PreparedStatement ps = conn.prepareStatement("SELECT * FROM users");
                     ResultSet rs = ps.executeQuery()) {
                    while (rs.next()) {
                        String name = rs.getString("name");
                    }
                } catch (SQLException e) {
                    e.printStackTrace();
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_try_with_resources_annotations() {
    let source = r#"
        class Foo {
            void bar() {
                try (@SuppressWarnings("resource") final InputStream is = new FileInputStream("file.txt")) {
                    int data = is.read();
                } catch (IOException e) {
                    e.printStackTrace();
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_record_with_custom_constructor() {
    let source = r"
        public record Point(int x, int y) {
            public Point {
                if (x < 0 || y < 0) {
                    throw new IllegalArgumentException();
                }
            }

            public double distance() {
                return Math.sqrt(x * x + y * y);
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_sealed_class_hierarchy() {
    let source = r"
        public sealed class Shape permits Circle, Rectangle, Triangle {
            abstract double area();
        }

        final class Circle extends Shape {
            double area() { return 0; }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_switch_expression() {
    let source = r"
        class Foo {
            int bar(int x) {
                return switch(x) {
                    case 1 -> 10;
                    case 2 -> 20;
                    default -> 0;
                };
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_switch_arrow_blocks() {
    let source = r#"
        class Foo {
            void bar(int readingSchedule) {
                switch (readingSchedule) {
                    case 1 -> {
                        System.out.println("daily");
                        schedule.setInterval(1);
                    }
                    case 2 -> {
                        System.out.println("weekly");
                        schedule.setInterval(2);
                    }
                    case 3 -> {
                        System.out.println("monthly");
                        schedule.setInterval(3);
                    }
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_switch_arrow_enum_simple() {
    let source = r#"
        class Foo {
            void bar(MyEnum e) {
                switch (e) {
                    case OPTION_A -> System.out.println("A");
                    case OPTION_B -> System.out.println("B");
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_switch_arrow_mixed_syntax() {
    let source = r#"
        class Foo {
            void bar(ReadingSchedule schedule) {
                switch (schedule) {
                    case DAILY -> {
                        config.setInterval(Interval.DAILY);
                        config.setDelayDays(config.catalogRefreshDelay());
                    }
                    case WEEKLY -> {
                        config.setInterval(Interval.WEEKLY);
                        config.setDelayDays(config.catalogRefreshDelay());
                        config.setWeeklyAnchor(config.catalogRefreshDay());
                    }
                    case MONTHLY -> {
                        config.setInterval(Interval.MONTHLY);
                        config.setDelayDays(config.catalogRefreshDelay());
                        config.setMonthlyAnchor(config.monthlyAnchor());
                    }
                    default -> throw new IllegalArgumentException("Unknown schedule");
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_switch_arrow_expressions() {
    let source = r"
        class Foo {
            int bar(int x) {
                return switch (x) {
                    case 1 -> 10;
                    case 2 -> 20;
                    case 3, 4, 5 -> 30;
                    default -> 0;
                };
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_switch_arrow_with_throw() {
    let source = r#"
        class Foo {
            void bar(int x) {
                switch (x) {
                    case 1, 2 -> System.out.println("low");
                    case 3, 4 -> System.out.println("mid");
                    default -> throw new IllegalArgumentException("Invalid");
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_pattern_matching() {
    let source = r"
        class Foo {
            void bar(Object obj) {
                if (obj instanceof String s) {
                    System.out.println(s.length());
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_matching_with_qualified_type() {
    let source = r"
        class Foo {
            void bar(Object obj) {
                if (obj instanceof java.lang.String str) {
                    System.out.println(str);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_matching_with_generic_type() {
    let source = r"
        class Foo {
            void bar(Object obj) {
                if (obj instanceof List<String> list) {
                    list.forEach(System.out::println);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_matching_with_array_type() {
    let source = r"
        class Foo {
            void bar(Object obj) {
                if (obj instanceof String[] arr) {
                    System.out.println(arr.length);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_regular_instanceof_still_works() {
    let source = r"
        class Foo {
            void bar(Object obj) {
                if (obj instanceof String) {
                    String s = (String) obj;
                    System.out.println(s);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_matching_with_primitive_types() {
    // Primitive types aren't typically used with instanceof, but parser should handle it
    let source = r"
        class Foo {
            void bar(Object obj) {
                boolean check1 = obj instanceof Integer i;
                boolean check2 = obj instanceof int;
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Pattern Matching AND Chain Tests (Java 16+)
// ============================================================================

#[test]
fn test_pattern_in_and_chain() {
    // Pattern variable used in AND RHS
    let source = r"
        class Test {
            void method(Object obj) {
                if (obj instanceof String s && s.length() > 10) {
                    System.out.println(s);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_multiple_patterns_in_and_chain() {
    // Multiple pattern variables in AND chain
    let source = r"
        class Test {
            void method(Object a, Object b) {
                if (a instanceof String s && b instanceof Integer i && s.length() == i) {
                    System.out.println(s + i);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_in_nested_and_chains() {
    // Nested AND chains with pattern variables
    let source = r#"
        class Test {
            void method(Object obj) {
                if (obj instanceof String s && s.length() > 5 && s.contains("test")) {
                    System.out.println(s);
                }
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_pattern_with_parenthesized_and() {
    // Parenthesized pattern instanceof in AND chain
    let source = r"
        class Test {
            void method(Object obj) {
                if ((obj instanceof String s) && s.length() > 10) {
                    System.out.println(s);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_in_or_chain() {
    // Pattern variable in OR chain (pattern NOT available after)
    let source = r"
        class Test {
            void method(Object obj) {
                if (obj instanceof String s || obj.toString().isEmpty()) {
                    // s should NOT be available here in general
                    // (would need both branches to define same pattern)
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_in_mixed_and_or() {
    // Mixed AND/OR with pattern variables
    let source = r"
        class Test {
            void method(Object obj, boolean flag) {
                if (obj instanceof String s && (flag || s.isEmpty())) {
                    System.out.println(s);
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_pattern_and_chain_with_method_call() {
    // Pattern variable used in method call in AND RHS
    let source = r"
        class Test {
            void method(Object obj) {
                if (obj instanceof String s && isValid(s)) {
                    System.out.println(s);
                }
            }

            boolean isValid(String s) {
                return s != null;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_complex_generics() {
    let source = r"
        class Foo {
            <T extends Comparable<T>> T max(T a, T b) {
                return a.compareTo(b) > 0 ? a : b;
            }

            <K, V> Map<K, List<V>> groupBy(List<V> items, Function<V, K> keyFn) {
                return items.stream()
                    .collect(Collectors.groupingBy(keyFn));
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_annotations_everywhere() {
    let source = r#"
        @Entity
        @Table(name = "users")
        public class User {
            @Id
            @GeneratedValue
            private Long id;

            @Column(nullable = false)
            private String name;

            @Override
            @Transactional
            public boolean equals(Object o) {
                return false;
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_multi_level_nested_generics() {
    let source = r"
        class Foo {
            Map<String, List<Map<Integer, Set<String>>>> complex;
        }
    ";
    parse_ok(source);
}

#[test]
fn test_complete_java_class() {
    let source = r#"
        package com.example.app;

        import java.util.*;
        import java.io.*;

        /**
         * A sample class demonstrating various Java features.
         */
        @SuppressWarnings("unchecked")
        public final class SampleClass<T extends Comparable<T>> implements Serializable {
            private static final long serialVersionUID = 1L;
            private static int counter = 0;

            private final T value;
            private List<T> items;

            static {
                System.out.println("Static initializer");
            }

            {
                System.out.println("Instance initializer");
            }

            public SampleClass(T value) {
                this.value = value;
                this.items = new ArrayList<>();
                counter++;
            }

            public T getValue() {
                return value;
            }

            public void addItem(T item) {
                items.add(item);
            }

            public <U> U transform(Function<T, U> fn) {
                return fn.apply(value);
            }

            @Override
            public boolean equals(Object o) {
                if (this == o) return true;
                if (!(o instanceof SampleClass)) return false;
                SampleClass<?> that = (SampleClass<?>) o;
                return Objects.equals(value, that.value);
            }

            @Override
            public int hashCode() {
                return Objects.hash(value);
            }

            public static <T extends Comparable<T>> T max(T a, T b) {
                return a.compareTo(b) > 0 ? a : b;
            }

            private class InnerClass {
                void method() {
                    System.out.println(value);
                }
            }

            public static class NestedClass {
                public void method() {}
            }
        }
    "#;
    parse_ok(source);
}

// ============================================================================
// Bitwise Operator Tests
// ============================================================================

#[test]
fn test_bitwise_operators() {
    let source = r"
        class Foo {
            void bar() {
                int a = x & y;   // AND
                int b = x | y;   // OR
                int c = x ^ y;   // XOR
                int d = ~x;      // NOT
                int e = x << 2;  // Left shift
                int f = x >> 2;  // Right shift
                int g = x >>> 2; // Unsigned right shift
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_bitwise_operators_in_lambda() {
    let source = r"
        class Foo {
            void bar() {
                // Bitwise OR in lambda body
                BinaryOperator<Integer> or = (a, b) -> a | b;

                // Bitwise AND in lambda body
                BinaryOperator<Integer> and = (a, b) -> a & b;

                // Bitwise XOR in lambda body
                BinaryOperator<Integer> xor = (a, b) -> a ^ b;

                // Complex expression with bitwise operators
                BiFunction<Integer, Integer, Integer> complex = (a, b) -> (a | b) & 0xFF;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_compound_bitwise_assignment() {
    let source = r"
        class Foo {
            void bar() {
                int x = 10;
                x |= 5;   // OR assignment
                x &= 3;   // AND assignment
                x ^= 2;   // XOR assignment
                x <<= 1;  // Left shift assignment
                x >>= 1;  // Right shift assignment
                x >>>= 1; // Unsigned right shift assignment
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Cast Expression Tests
// ============================================================================

#[test]
fn test_cast_expressions() {
    let source = r"
        class Foo {
            void bar() {
                int a = (int) 3.14;
                String s = (String) obj;
                Number n = (Number) value;
                List<String> list = (List<String>) rawList;
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_cast_in_expression() {
    let source = r"
        class Foo {
            void bar() {
                int result = ((int) value) + 10;
                Object o = (Object) (x + y);
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Array Access Expression Tests
// ============================================================================

#[test]
fn test_array_access() {
    let source = r"
        class Foo {
            void bar() {
                int x = arr[0];
                String s = names[i];
                int val = matrix[row][col];
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_array_access_chain() {
    let source = r"
        class Foo {
            void bar() {
                int x = list[0].length();
                String s = matrix[i][j].toUpperCase();
                int val = arrays[0][1][2];
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// New Expression / Constructor Call Tests
// ============================================================================

#[test]
fn test_new_expressions() {
    let source = r#"
        class Foo {
            void bar() {
                Object o = new Object();
                String s = new String("hello");
                List<String> list = new ArrayList<>();
                int[] arr = new int[10];
                String[][] matrix = new String[5][10];
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_new_with_diamond_operator() {
    let source = r"
        class Foo {
            void bar() {
                List<String> names = new ArrayList<>();
                Map<Integer, String> map = new HashMap<>();
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Anonymous Class Tests
// ============================================================================

#[test]
fn test_anonymous_class() {
    let source = r#"
        class Foo {
            void bar() {
                Runnable r = new Runnable() {
                    public void run() {
                        System.out.println("Running");
                    }
                };
            }
        }
    "#;
    parse_ok(source);
}

#[test]
fn test_anonymous_class_with_interface() {
    let source = r"
        class Foo {
            void bar() {
                Comparator<String> comp = new Comparator<String>() {
                    @Override
                    public int compare(String a, String b) {
                        return a.length() - b.length();
                    }
                };
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Labeled Statement Tests
// ============================================================================

#[test]
fn test_labeled_statements() {
    let source = r"
        class Foo {
            void bar() {
                outer: for (int i = 0; i < 10; i++) {
                    inner: for (int j = 0; j < 10; j++) {
                        if (condition) break outer;
                        if (other) continue inner;
                    }
                }
            }
        }
    ";
    parse_ok(source);
}

#[test]
fn test_labeled_block() {
    let source = r"
        class Foo {
            void bar() {
                search: {
                    for (int i = 0; i < 10; i++) {
                        if (found) break search;
                    }
                }
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Yield Statement Tests (for switch expressions)
// ============================================================================

#[test]
fn test_yield_in_switch_expression() {
    let source = r#"
        class Foo {
            int bar(int x) {
                return switch(x) {
                    case 1 -> {
                        System.out.println("One");
                        yield 10;
                    }
                    case 2 -> {
                        System.out.println("Two");
                        yield 20;
                    }
                    default -> {
                        yield 0;
                    }
                };
            }
        }
    "#;
    parse_ok(source);
}

// ============================================================================
// Parenthesized Expression Tests
// ============================================================================

#[test]
fn test_parenthesized_expressions() {
    let source = r"
        class Foo {
            void bar() {
                int a = (1 + 2) * 3;
                boolean b = (x > 5) && (y < 10);
                int c = ((a + b) * (c - d));
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Unary Operator Tests (including bitwise NOT)
// ============================================================================

#[test]
fn test_unary_operators() {
    let source = r"
        class Foo {
            void bar() {
                int a = +x;
                int b = -y;
                int c = ~flags;  // Bitwise NOT
                boolean d = !condition;
            }
        }
    ";
    parse_ok(source);
}

// ============================================================================
// Constructor Delegation Tests (this(...) and super(...))
// ============================================================================

#[test]
fn test_constructor_delegation_this_simple() {
    let source = r#"
        class Foo {
            private String name;

            public Foo() {
                this("default");
            }

            public Foo(String name) {
                this.name = name;
            }
        }
    "#;
    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");

    // Verify EXPLICIT_CONSTRUCTOR_INVOCATION node exists
    let has_explicit_invocation = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION);
    assert!(
        has_explicit_invocation,
        "Should have EXPLICIT_CONSTRUCTOR_INVOCATION node"
    );
}

#[test]
fn test_constructor_delegation_super_simple() {
    let source = r"
        class Child extends Parent {
            public Child() {
                super();
            }
        }
    ";
    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");

    let has_explicit_invocation = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION);
    assert!(
        has_explicit_invocation,
        "Should have EXPLICIT_CONSTRUCTOR_INVOCATION node"
    );
}

#[test]
fn test_constructor_delegation_super_with_args() {
    let source = r"
        class Child extends Parent {
            private String name;

            public Child(String name, int value) {
                super(value);
                this.name = name;
            }
        }
    ";
    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");

    let has_explicit_invocation = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION);
    assert!(
        has_explicit_invocation,
        "Should have EXPLICIT_CONSTRUCTOR_INVOCATION node"
    );
}

#[test]
fn test_constructor_delegation_with_type_args() {
    let source = r#"
        class GenericClass<T> {
            public GenericClass() {
                <String>this("default");
            }

            <U> GenericClass(U value) {
                super();
            }
        }
    "#;
    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");

    let has_explicit_invocation = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION);
    assert!(
        has_explicit_invocation,
        "Should have EXPLICIT_CONSTRUCTOR_INVOCATION node"
    );
}

#[test]
fn test_constructor_no_delegation() {
    // Constructor without delegation should NOT have EXPLICIT_CONSTRUCTOR_INVOCATION
    let source = r"
        class Foo {
            private String name;

            public Foo(String name) {
                this.name = name;
            }
        }
    ";
    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");

    let has_explicit_invocation = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION);
    assert!(
        !has_explicit_invocation,
        "Should NOT have EXPLICIT_CONSTRUCTOR_INVOCATION node"
    );
}

#[test]
fn test_constructor_delegation_must_be_first_statement() {
    // this() must be the first statement - additional statements should follow
    let source = r#"
        class Foo {
            private String name;
            private int count;

            public Foo() {
                this("default");
                this.count = 0;
            }

            public Foo(String name) {
                this.name = name;
                this.count = 0;
            }
        }
    "#;
    let (root, errors) = parse(source);
    assert!(errors.is_empty(), "Parse errors: {errors:?}");

    let has_explicit_invocation = root
        .descendants()
        .any(|n| n.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION);
    assert!(
        has_explicit_invocation,
        "Should have EXPLICIT_CONSTRUCTOR_INVOCATION node"
    );
}

// ============================================================================
// Contextual Keyword Tests (record, var, yield, etc.)
// ============================================================================
// These keywords are only keywords in specific contexts and should be allowed
// as identifiers (variable names, parameter names, etc.) in other contexts.

#[test]
fn test_record_as_variable_name() {
    let source = r"
class Test {
    void test() {
        ReleaseRecord record = getRecord();
        record.process();
    }
}
";
    parse_ok(source);
}

#[test]
fn test_record_as_field_name() {
    let source = r"
class Test {
    private ReleaseRecord record;

    void setRecord(ReleaseRecord record) {
        this.record = record;
    }
}
";
    parse_ok(source);
}

// ============================================================================
// Contextual Keyword Type Name Restriction Tests
// ============================================================================
// According to JLS, contextual keywords (record, var, yield) cannot be used
// as type names (class, interface, enum names) even though they can be used
// as variable names, parameter names, field names, etc.

#[test]
fn test_reject_record_as_class_name() {
    let source = r"
package com.example;

public class record {
    private String name;
}
";
    let (_node, errors) = parse(source);

    // Should have parse error - 'record' cannot be a class name
    assert!(
        !errors.is_empty(),
        "Expected parse error when using 'record' as class name, but parsing succeeded"
    );
}

#[test]
fn test_reject_var_as_interface_name() {
    let source = r"
package com.example;

public interface var {
    void doSomething();
}
";
    let (_node, errors) = parse(source);

    // Should have parse error - 'var' cannot be an interface name
    assert!(
        !errors.is_empty(),
        "Expected parse error when using 'var' as interface name, but parsing succeeded"
    );
}

#[test]
fn test_reject_yield_as_enum_name() {
    let source = r"
package com.example;

public enum yield {
    VALUE1, VALUE2
}
";
    let (_node, errors) = parse(source);

    // Should have parse error - 'yield' cannot be an enum name
    assert!(
        !errors.is_empty(),
        "Expected parse error when using 'yield' as enum name, but parsing succeeded"
    );
}

#[test]
fn test_accept_record_as_inner_class_field_name() {
    // Verify that 'record' IS allowed as field/variable/parameter name
    let source = r"
package com.example;

public class ReleaseDelta {
    private ReleaseRecord record;  // Field name - OK

    public void replay(ReleaseRecord record) {  // Parameter name - OK
        ReleaseRecord record2 = record;  // Variable name - OK
        System.out.println(record);
    }

    class ReleaseRecord {
        String name;
    }
}
";
    parse_ok(source);
}

// Class Literal Tests

#[test]
fn test_primitive_class_literal() {
    // Test int.class and other primitive class literals
    let source = r"
class Test {
    void test() {
        Class<?> intClass = int.class;
        Class<?> longClass = long.class;
        Class<?> boolClass = boolean.class;
        Class<?> voidClass = void.class;
        Class<?> charClass = char.class;
        Class<?> byteClass = byte.class;
        Class<?> shortClass = short.class;
        Class<?> floatClass = float.class;
        Class<?> doubleClass = double.class;
    }
}
";
    parse_ok(source);
}

#[test]
fn test_reference_type_class_literal() {
    // Test String.class and other reference type class literals
    let source = r"
class Test {
    void test() {
        Class<?> stringClass = String.class;
        Class<?> listClass = java.util.List.class;
    }
}
";
    parse_ok(source);
}
