// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Context detection utilities for reducing false positives
//!
//! Provides utilities to detect demo/test/seeder contexts and other
//! special-purpose code that should be handled differently by security checks.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::path::Path;

/// Determine if code is in a demo/test/seeder context
///
/// These contexts are often flagged for issues that are intentional:
/// - Demo data seeders use java.util.Random for fake data generation
/// - Test initializers log credentials for developer debugging
/// - Sample code includes hardcoded values for demonstration
///
/// # Detection Strategy
///
/// Checks three indicators in order of specificity:
/// 1. **File path** - Is the file in a test/demo directory?
/// 2. **Class name** - Does the class name contain test/demo/seeder patterns?
/// 3. **Method name** - Does the method name suggest test/demo/initialization?
///
/// # Examples
///
/// ```
/// # use std::path::PathBuf;
/// # use hyperion_security::context::is_demo_or_test_context;
/// // File path detection
/// assert!(is_demo_or_test_context(
///     &PathBuf::from("src/test/java/Foo.java"),
///     "Foo",
///     None
/// ));
///
/// // Class name detection
/// assert!(is_demo_or_test_context(
///     &PathBuf::from("src/main/java/AnalyticsDemoDataSeeder.java"),
///     "AnalyticsDemoDataSeeder",
///     None
/// ));
///
/// // Method name detection
/// assert!(is_demo_or_test_context(
///     &PathBuf::from("src/main/java/DataInitializer.java"),
///     "DataInitializer",
///     Some("seedDemoData")
/// ));
/// ```
pub fn is_demo_or_test_context(
    file_path: &Path,
    class_name: &str,
    method_name: Option<&str>,
) -> bool {
    // Check file path - look for test/demo directories in standard locations
    // These patterns match Maven/Gradle conventions: src/test/java, src/main/java/demo, etc.
    let path_str = file_path.to_string_lossy().to_lowercase();
    if path_str.contains("/src/test/")
        || path_str.contains("/src/tests/")
        || path_str.contains("/test/java/")
        || path_str.contains("/tests/java/")
        || path_str.contains("/java/demo/")
        || path_str.contains("/java/demos/")
        || path_str.contains("/java/examples/")
        || path_str.contains("/java/fixtures/")
        || path_str.contains("/java/fixture/")
        || path_str.contains("/java/sample/")
        || path_str.contains("/java/samples/")
        || path_str.contains("/java/seed/")
        || path_str.contains("/java/seeds/")
    {
        return true;
    }

    // Check class name patterns
    // Use word boundary patterns to avoid false positives like "UserService" matching "test"
    let class_lower = class_name.to_lowercase();

    // Check for suffix patterns (e.g., "FooTest", "BarSeeder")
    if class_lower.ends_with("test")
        || class_lower.ends_with("tests")
        || class_lower.ends_with("testing")
        || class_lower.ends_with("demo")
        || class_lower.ends_with("sample")
        || class_lower.ends_with("example")
        || class_lower.ends_with("seeder")
        || class_lower.ends_with("dataseeder")
        || class_lower.ends_with("fixture")
        || class_lower.ends_with("initializer")
        || class_lower.ends_with("datainitializer")
        || class_lower.ends_with("mock")
        || class_lower.ends_with("stub")
        || class_lower.ends_with("fake")
    {
        return true;
    }

    // Check for prefix patterns (e.g., "TestFoo", "DemoBar")
    if class_lower.starts_with("test")
        || class_lower.starts_with("demo")
        || class_lower.starts_with("sample")
        || class_lower.starts_with("example")
        || class_lower.starts_with("mock")
        || class_lower.starts_with("stub")
        || class_lower.starts_with("fake")
    {
        return true;
    }

    // Check method name patterns (if provided)
    if let Some(method) = method_name {
        let method_lower = method.to_lowercase();
        let demo_method_patterns = [
            "test", "demo", "seed", "init", "setup", "mock", "stub", "fake", "example", "sample",
        ];

        if demo_method_patterns
            .iter()
            .any(|pattern| method_lower.contains(pattern))
        {
            return true;
        }
    }

    false
}

/// Extract class name from a syntax node by walking up to find CLASS_DECL
pub fn get_containing_class_name(node: &SyntaxNode) -> Option<String> {
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if n.kind() == SyntaxKind::CLASS_DECL {
            // Find the class name identifier
            for child in n.children_with_tokens() {
                if child.kind() == SyntaxKind::IDENT {
                    if let Some(token) = child.as_token() {
                        return Some(token.text().to_string());
                    }
                }
            }
        }
        current = n.parent();
    }
    None
}

/// Extract method name from a syntax node by walking up to find METHOD_DECL
pub fn get_containing_method_name(node: &SyntaxNode) -> Option<String> {
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if n.kind() == SyntaxKind::METHOD_DECL {
            // Find the method name identifier
            for child in n.children_with_tokens() {
                if child.kind() == SyntaxKind::IDENT {
                    if let Some(token) = child.as_token() {
                        return Some(token.text().to_string());
                    }
                }
            }
        }
        current = n.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_file_path_detection() {
        // Test directory detection
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/test/java/com/example/Foo.java"),
            "Foo",
            None
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/demo/DemoData.java"),
            "DemoData",
            None
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/examples/Example.java"),
            "Example",
            None
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/fixtures/TestFixture.java"),
            "TestFixture",
            None
        ));

        // Should NOT match package names
        assert!(!is_demo_or_test_context(
            &PathBuf::from("src/main/java/com/example/UserService.java"),
            "UserService",
            None
        ));
    }

    #[test]
    fn test_class_name_detection() {
        // Seeder classes
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/AnalyticsDemoDataSeeder.java"),
            "AnalyticsDemoDataSeeder",
            None
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/UserDataSeeder.java"),
            "UserDataSeeder",
            None
        ));

        // Test classes
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/UserServiceTest.java"),
            "UserServiceTest",
            None
        ));

        // Demo classes
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/DemoController.java"),
            "DemoController",
            None
        ));

        // Initializer classes
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/DataInitializer.java"),
            "DataInitializer",
            None
        ));

        // Mock/Stub classes
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/MockPaymentService.java"),
            "MockPaymentService",
            None
        ));
    }

    #[test]
    fn test_method_name_detection() {
        // Seed methods
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/Setup.java"),
            "Setup",
            Some("seedDemoData")
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/Setup.java"),
            "Setup",
            Some("initTestData")
        ));

        // Demo methods
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/DataLoader.java"),
            "DataLoader",
            Some("loadDemoUsers")
        ));

        // Test methods
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/Service.java"),
            "Service",
            Some("testConnection")
        ));
    }

    #[test]
    fn test_production_code_not_flagged() {
        // Production code should NOT match
        assert!(!is_demo_or_test_context(
            &PathBuf::from("src/main/java/com/example/UserService.java"),
            "UserService",
            Some("createUser")
        ));

        assert!(!is_demo_or_test_context(
            &PathBuf::from("src/main/java/com/example/PaymentController.java"),
            "PaymentController",
            Some("processPayment")
        ));

        assert!(!is_demo_or_test_context(
            &PathBuf::from("src/main/java/com/example/AuthService.java"),
            "AuthService",
            Some("authenticate")
        ));
    }

    #[test]
    fn test_case_insensitive() {
        // Should work regardless of case
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/TEST/java/Foo.java"),
            "Foo",
            None
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/FOO.java"),
            "UserTEST",
            None
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/Foo.java"),
            "Foo",
            Some("DEMO_METHOD")
        ));
    }

    #[test]
    fn test_multiple_indicators() {
        // Multiple indicators should still return true
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/test/java/DemoTest.java"),
            "DemoTest",
            Some("testDemo")
        ));
    }

    #[test]
    fn test_partial_matches() {
        // Partial matches should work
        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/Foo.java"),
            "AnalyticsDemoDataSeeder", // Contains "demo" and "seeder"
            None
        ));

        assert!(is_demo_or_test_context(
            &PathBuf::from("src/main/java/Foo.java"),
            "Foo",
            Some("seedAnalyticsDemo") // Contains "seed" and "demo"
        ));
    }
}
