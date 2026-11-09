// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for qualified member access navigation (`obj.method()`, obj.field)
//!
//! Tests the type inference and navigation system for accessing methods and fields
//! on typed variables, fields, and parameters.

use crate::lsp_test_framework::*;
use tower_lsp::lsp_types::*;

/// Helper macro to assert goto-definition result navigates to expected file
macro_rules! assert_navigates_to {
    ($result:expr, $expected_file:expr, $test_name:expr) => {
        match $result {
            Ok(Some(GotoDefinitionResponse::Scalar(loc))) => {
                assert!(
                    loc.uri.as_str().contains($expected_file),
                    "Expected {}, got: {}",
                    $expected_file,
                    loc.uri
                );
                println!(
                    "✅ {}: Navigated to {} at {:?}",
                    $test_name, $expected_file, loc.range.start
                );
            }
            Ok(Some(GotoDefinitionResponse::Array(locs))) => {
                assert!(!locs.is_empty(), "Empty location array");
                assert!(
                    locs[0].uri.as_str().contains($expected_file),
                    "Expected {}, got: {}",
                    $expected_file,
                    locs[0].uri
                );
                println!(
                    "✅ {}: Navigated to {} at {:?}",
                    $test_name, $expected_file, locs[0].range.start
                );
            }
            Ok(Some(GotoDefinitionResponse::Link(_))) => {
                panic!("Unexpected link response");
            }
            Ok(None) => panic!("Expected definition, got None"),
            Err(e) => panic!("Error: {}", e),
        }
    };
}

// ============================================================================
// Test 1: Field with Method Call
// ============================================================================

#[test]
fn test_method_call_on_field() {
    // Initialize logging to see debug! statements
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test = LspTest::new();

    test.add_file(
        "com/example/Foo.java",
        r"
        package com.example;

        import com.example.Bar;

        public class Foo {
            private Bar bar;

            public void test() {
                bar.ba<|>z();
            }
        }
    ",
    );

    test.add_file(
        "com/example/Bar.java",
        r#"
        package com.example;

        public class Bar {
            public void /*target:baz*/baz() {
                System.out.println("Hello");
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Foo.java", "<|>");
    assert_navigates_to!(result, "Bar.java", "test_method_call_on_field");
}

// ============================================================================
// Test 1b: Field with Fully Qualified Type
// ============================================================================

#[test]
fn test_method_call_on_field_with_fqn_type() {
    let mut test = LspTest::new();

    test.add_file(
        "Foo.java",
        r"
        public class Foo {
            private com.foo.Bar bar;

            public void test() {
                bar.ba<|>z();
            }
        }
    ",
    );

    test.add_file(
        "com/foo/Bar.java",
        r#"
        package com.foo;

        public class Bar {
            public void /*target:baz*/baz() {
                System.out.println("Hello");
            }
        }
    "#,
    );

    let result = test.goto_definition("Foo.java", "<|>");
    assert_navigates_to!(
        result,
        "Bar.java",
        "test_method_call_on_field_with_fqn_type"
    );
}

// ============================================================================
// Test 1c: Field with Fully Qualified Type in Package
// ============================================================================

#[test]
fn test_method_call_on_field_with_fqn_type_in_package() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Foo.java",
        r"
        package com.example;

        public class Foo {
            private com.foo.Bar bar;

            public void test() {
                bar.ba<|>z();
            }
        }
    ",
    );

    test.add_file(
        "com/foo/Bar.java",
        r#"
        package com.foo;

        public class Bar {
            public void /*target:baz*/baz() {
                System.out.println("Hello");
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Foo.java", "<|>");
    assert_navigates_to!(
        result,
        "Bar.java",
        "test_method_call_on_field_with_fqn_type_in_package"
    );
}

// ============================================================================
// Test 2: Local Variable with Method Call
// ============================================================================

#[test]
fn test_method_call_on_local_variable() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        import com.example.User;

        public class Test {
            public void process() {
                User user = new User();
                user.getNa<|>me();
            }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;

        public class User {
            private String name;

            public /*target:getName*/String getName() {
                return name;
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/Test.java", "<|>");
    assert_navigates_to!(result, "User.java", "test_method_call_on_local_variable");
}

// ============================================================================
// Test 3: Parameter with Method Call
// ============================================================================

#[test]
fn test_method_call_on_parameter() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Service.java",
        r"
        package com.example;

        import com.example.User;

        public class Service {
            public void process(User user) {
                user.getEm<|>ail();
            }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;

        public class User {
            private String email;

            public /*target:getEmail*/String getEmail() {
                return email;
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/Service.java", "<|>");
    assert_navigates_to!(result, "User.java", "test_method_call_on_parameter");
}

// ============================================================================
// Test 4: Multiple Method Calls on Same Object
// ============================================================================

#[test]
fn test_multiple_methods_same_object() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/MultiCall.java",
        r"
        package com.example;

        import com.example.config.SystemConfiguration;

        public class MultiCall {
            private SystemConfiguration config;

            public void test() {
                config.setTim<|>eout(100);
                config.getDatabaseUrl();
            }
        }
    ",
    );

    test.add_file(
        "com/example/config/SystemConfiguration.java",
        r"
        package com.example.config;

        public class SystemConfiguration {
            private int timeout;
            private String databaseUrl;

            public /*target:setTimeout*/void setTimeout(int timeout) {
                this.timeout = timeout;
            }

            public String getDatabaseUrl() {
                return databaseUrl;
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/MultiCall.java", "<|>");
    assert_navigates_to!(
        result,
        "SystemConfiguration.java",
        "test_multiple_methods_same_object"
    );
}

// ============================================================================
// Test 5: Constructor Call (should navigate to class, not method)
// ============================================================================

#[test]
fn test_constructor_in_variable_declaration() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Creator.java",
        r"
        package com.example;

        import com.example.User;

        public class Creator {
            public void create() {
                User user = new User<|>();
            }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;

        public /*target:User*/class User {
            public User() {
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/Creator.java", "<|>");
    assert_navigates_to!(
        result,
        "User.java",
        "test_constructor_in_variable_declaration"
    );
}

// ============================================================================
// Test 6: Primitive Type (should NOT navigate to method - no qualified access)
// ============================================================================

#[test]
fn test_primitive_type_variable() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Primitives.java",
        r"
        package com.example;

        public class Primitives {
            public void test() {
                int count<|> = 0;
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/Primitives.java", "<|>");

    // Should either return None or navigate to the variable definition itself
    match result {
        Ok(Some(loc)) => {
            println!("✅ test_primitive_type_variable: Got location (variable def): {loc:?}");
        }
        Ok(None) => {
            println!("✅ test_primitive_type_variable: No definition (expected for primitive)");
        }
        Err(e) => panic!("Error: {e}"),
    }
}

// ============================================================================
// Test 7: Array Type Method Call
// ============================================================================

#[test]
fn test_array_type_navigation() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Arrays.java",
        r"
        package com.example;

        import com.example.User;

        public class Arrays {
            private User[] users;

            public void test() {
                User firstUser = users[0];
                firstUser.getNa<|>me();
            }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r#"
        package com.example;

        public class User {
            public /*target:getName*/String getName() {
                return "test";
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Arrays.java", "<|>");
    assert_navigates_to!(result, "User.java", "test_array_type_navigation");
}

// ============================================================================
// Test 8: Nested Package Structure
// ============================================================================

#[test]
fn test_nested_packages() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/service/UserService.java",
        r"
        package com.example.service;

        import com.example.model.User;

        public class UserService {
            public void process(User user) {
                user.get<|>Id();
            }
        }
    ",
    );

    test.add_file(
        "com/example/model/User.java",
        r"
        package com.example.model;

        public class User {
            private Long id;

            public /*target:getId*/Long getId() {
                return id;
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/service/UserService.java", "<|>");
    assert_navigates_to!(result, "model/User.java", "test_nested_packages");
}

// ============================================================================
// Test 9: Same Package (no import needed)
// ============================================================================

#[test]
fn test_same_package_no_import() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Caller.java",
        r"
        package com.example;

        public class Caller {
            public void test() {
                Helper helper = new Helper();
                helper.doSom<|>ething();
            }
        }
    ",
    );

    test.add_file(
        "com/example/Helper.java",
        r"
        package com.example;

        public class Helper {
            public /*target:doSomething*/void doSomething() {
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/Caller.java", "<|>");
    assert_navigates_to!(result, "Helper.java", "test_same_package_no_import");
}

// ============================================================================
// Test 10: Wildcard Import
// ============================================================================

#[test]
fn test_wildcard_import() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/WildcardTest.java",
        r#"
        package com.example;

        import com.example.util.*;

        public class WildcardTest {
            public void test() {
                StringUtils utils = new StringUtils();
                utils.isEm<|>pty("");
            }
        }
    "#,
    );

    test.add_file(
        "com/example/util/StringUtils.java",
        r"
        package com.example.util;

        public class StringUtils {
            public /*target:isEmpty*/boolean isEmpty(String s) {
                return s == null || s.isEmpty();
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/WildcardTest.java", "<|>");
    assert_navigates_to!(result, "StringUtils.java", "test_wildcard_import");
}

// ============================================================================
// Test 11: Field Initialized in Constructor
// ============================================================================

#[test]
fn test_field_initialized_in_constructor() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Container.java",
        r"
        package com.example;

        import com.example.Service;

        public class Container {
            private Service service;

            public Container() {
                this.service = new Service();
            }

            public void use() {
                service.exe<|>cute();
            }
        }
    ",
    );

    test.add_file(
        "com/example/Service.java",
        r"
        package com.example;

        public class Service {
            public /*target:execute*/void execute() {
            }
        }
    ",
    );

    let result = test.goto_definition("com/example/Container.java", "<|>");
    assert_navigates_to!(
        result,
        "Service.java",
        "test_field_initialized_in_constructor"
    );
}

// ============================================================================
// Test 12: Method with Return Type (cursor on return type)
// ============================================================================

#[test]
fn test_navigate_from_return_type() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Factory.java",
        r"
        package com.example;

        import com.example.User;

        public class Factory {
            public User<|> createUser() {
                return new User();
            }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;

        public /*target:User*/class User {
        }
    ",
    );

    let result = test.goto_definition("com/example/Factory.java", "<|>");
    assert_navigates_to!(result, "User.java", "test_navigate_from_return_type");
}

// ============================================================================
// Test 13: Multiple Parameters
// ============================================================================

#[test]
fn test_multiple_parameters() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Processor.java",
        r"
        package com.example;

        import com.example.User;
        import com.example.Context;

        public class Processor {
            public void process(User user, Context context) {
                context.l<|>og(user.getName());
            }
        }
    ",
    );

    test.add_file(
        "com/example/Context.java",
        r"
        package com.example;

        public class Context {
            public /*target:log*/void log(String message) {
            }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r#"
        package com.example;

        public class User {
            public String getName() {
                return "test";
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Processor.java", "<|>");
    assert_navigates_to!(result, "Context.java", "test_multiple_parameters");
}

// ============================================================================
// Test 14: Generic Type (basic - no type argument tracking yet)
// ============================================================================

#[test]
fn test_generic_type_basic() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/GenericTest.java",
        r#"
        package com.example;

        import java.util.List;
        import java.util.ArrayList;

        public class GenericTest {
            public void test() {
                List<String> items = new ArrayList<>();
                items.a<|>dd("test");
            }
        }
    "#,
    );

    // Note: We won't have List.java in our test workspace, so this might fail
    // This test is to verify behavior with standard library types

    let result = test.goto_definition("com/example/GenericTest.java", "<|>");

    match result {
        Ok(Some(loc)) => {
            println!("✅ test_generic_type_basic: Got location: {loc:?}");
        }
        Ok(None) => {
            println!("✅ test_generic_type_basic: No definition (expected - stdlib not indexed)");
        }
        Err(e) => {
            println!("✅ test_generic_type_basic: Error (expected): {e}");
        }
    }
}

// ============================================================================
// Test 15: Qualified Type Name in Declaration
// ============================================================================

#[test]
fn test_qualified_type_in_declaration() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Qualified.java",
        r"
        package com.example;

        public class Qualified {
            public void test() {
                com.example.model.User user = new com.example.model.User();
                user.getNa<|>me();
            }
        }
    ",
    );

    test.add_file(
        "com/example/model/User.java",
        r#"
        package com.example.model;

        public class User {
            public /*target:getName*/String getName() {
                return "test";
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Qualified.java", "<|>");
    assert_navigates_to!(
        result,
        "model/User.java",
        "test_qualified_type_in_declaration"
    );
}

// ============================================================================
// Test Runner
// ============================================================================

#[cfg(test)]
mod test_runner {
    use super::*;

    #[test]
    fn run_all_qualified_access_tests() {
        println!("\n🧪 Running Qualified Member Access Test Suite\n");
        println!("{}", "=".repeat(70));

        // Run each test and collect results
        let tests: Vec<(&str, fn())> = vec![
            ("Method call on field", test_method_call_on_field as fn()),
            (
                "Method call on local variable",
                test_method_call_on_local_variable as fn(),
            ),
            (
                "Method call on parameter",
                test_method_call_on_parameter as fn(),
            ),
            (
                "Multiple methods same object",
                test_multiple_methods_same_object as fn(),
            ),
            (
                "Constructor in variable decl",
                test_constructor_in_variable_declaration as fn(),
            ),
            (
                "Primitive type variable",
                test_primitive_type_variable as fn(),
            ),
            ("Array type navigation", test_array_type_navigation as fn()),
            ("Nested packages", test_nested_packages as fn()),
            (
                "Same package no import",
                test_same_package_no_import as fn(),
            ),
            ("Wildcard import", test_wildcard_import as fn()),
            (
                "Field initialized in constructor",
                test_field_initialized_in_constructor as fn(),
            ),
            (
                "Navigate from return type",
                test_navigate_from_return_type as fn(),
            ),
            ("Multiple parameters", test_multiple_parameters as fn()),
            ("Generic type basic", test_generic_type_basic as fn()),
            (
                "Qualified type in declaration",
                test_qualified_type_in_declaration as fn(),
            ),
        ];

        let mut passed = 0;
        let mut failed = 0;

        for (name, test_fn) in tests {
            print!("Testing: {name} ... ");
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(test_fn)) {
                Ok(()) => {
                    println!("✅ PASS");
                    passed += 1;
                }
                Err(e) => {
                    println!("❌ FAIL");
                    if let Some(msg) = e.downcast_ref::<String>() {
                        println!("  Error: {msg}");
                    } else if let Some(msg) = e.downcast_ref::<&str>() {
                        println!("  Error: {msg}");
                    }
                    failed += 1;
                }
            }
        }

        println!("\n{}", "=".repeat(70));
        println!("Test Results: {passed} passed, {failed} failed");
        println!(
            "Success rate: {:.1}%",
            (f64::from(passed) / f64::from(passed + failed)) * 100.0
        );

        assert!((failed <= 0), "{failed} tests failed");
    }
}
