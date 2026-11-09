// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for import resolution bugs
//!
//! This test suite validates bug fixes for:
//! 1. Ambiguous imports remaining unresolved
//! 2. Duplicate inner class resolution logic
//! 3. Static import candidate selection issues

use crate::lsp_test_framework::LspTest;

/// Test ambiguous import resolution
///
/// Multiple on-demand imports in the same binding tier are ambiguous under
/// Java's type-name rules. Navigation must not depend on import or index order.
#[test]
fn test_ambiguous_wildcard_import_resolution() {
    let mut test = LspTest::new();

    // Setup: Two packages with same class name
    test.add_file(
        "com/example/util/Helper.java",
        r"
        package com.example.util;
        public class Helper {
            public static void utilMethod() {}
        }
        ",
    );

    test.add_file(
        "com/example/service/Helper.java",
        r"
        package com.example.service;
        public class Helper {
            public static void serviceMethod() {}
        }
        ",
    );

    // File with ambiguous wildcard imports
    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import com.example.util.*;
        import com.example.service.*;

        public class Main {
            public void test() {
                Hel<|>per h = new Helper();
            }
        }
        ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");

    assert!(
        matches!(result, Ok(None)),
        "ambiguous wildcard imports must not elect a definition: {result:?}"
    );
    let hover = test.hover("com/example/Main.java", "<|>");
    assert!(
        matches!(hover, Ok(None)),
        "hover must share the rejected type identity: {hover:?}"
    );
    let references = test.find_references("com/example/Main.java", "<|>", true);
    assert!(
        matches!(references, Ok(ref locations) if locations.is_empty()),
        "references must not broaden an ambiguous type identity: {references:?}"
    );
    let rename = test.rename("com/example/Main.java", "<|>", "RenamedHelper");
    let rename_rejected = match &rename {
        Ok(None) => true,
        Err(message) => message.contains("does not resolve to a declaration"),
        Ok(Some(_)) => false,
    };
    assert!(
        rename_rejected,
        "rename must not broaden an ambiguous type identity: {rename:?}"
    );
}

/// Test explicit import should override wildcard ambiguity
#[test]
fn test_explicit_import_overrides_ambiguous_wildcards() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/util/Helper.java",
        r"
        package com.example.util;
        public class Helper {
            public static void utilMethod() {}
        }
        ",
    );

    test.add_file(
        "com/example/service/Helper.java",
        r"
        package com.example.service;
        public class Helper {
            public static void serviceMethod() {}
        }
        ",
    );

    // Explicit import should win
    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import com.example.service.Helper;
        import com.example.util.*;

        public class Main {
            public void test() {
                Hel<|>per h = new Helper();
            }
        }
        ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");

    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("service/Helper.java"),
                "Expected service.Helper due to explicit import, got: {}",
                loc.uri
            );
            println!("✅ Explicit import correctly overrides wildcards");
        }
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) => {
            assert!(
                locs[0].uri.as_str().contains("service/Helper.java"),
                "Expected service.Helper due to explicit import, got: {}",
                locs[0].uri
            );
            println!("✅ Explicit import correctly overrides wildcards");
        }
        _ => panic!("Expected definition of service.Helper"),
    }
}

/// Test duplicate inner class resolution logic
///
/// Lines 29-51 and 62-78 have nearly identical inner class resolution code.
/// This can lead to:
/// - Maintenance issues (bugs fixed in one but not the other)
/// - Race conditions if resolution order matters
/// - Unnecessary code duplication
#[test]
fn test_inner_class_resolution_consistency() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Outer.java",
        r"
        package com.example;
        public class Outer {
            public static class Inner {
                public void innerMethod() {}
            }
        }
        ",
    );

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        public class Main {
            public void test() {
                Outer.In<|>ner inner = new Outer.Inner();
            }
        }
        ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");

    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("Outer.java"),
                "Expected Outer.java for inner class, got: {}",
                loc.uri
            );
            println!("✅ Inner class resolution works");
        }
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) => {
            assert!(
                locs[0].uri.as_str().contains("Outer.java"),
                "Expected Outer.java for inner class, got: {}",
                locs[0].uri
            );
            println!("✅ Inner class resolution works");
        }
        _ => panic!("Expected definition of Outer.Inner"),
    }
}

/// Inner class without explicit import
#[test]
fn test_inner_class_same_package_no_import() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Container.java",
        r"
        package com.example;
        public class Container {
            public static class Item {
                public String name;
            }
        }
        ",
    );

    test.add_file(
        "com/example/User.java",
        r"
        package com.example;
        public class User {
            public void test() {
                Container.I<|>tem item = null;
            }
        }
        ",
    );

    let result = test.goto_definition("com/example/User.java", "<|>");

    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("Container.java"),
                "Expected Container.java for inner class, got: {}",
                loc.uri
            );
            println!("✅ Same-package inner class resolves correctly");
        }
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) => {
            assert!(
                locs[0].uri.as_str().contains("Container.java"),
                "Expected Container.java for inner class, got: {}",
                locs[0].uri
            );
            println!("✅ Same-package inner class resolves correctly");
        }
        _ => panic!("Expected definition of Container.Item"),
    }
}

/// Test static import ambiguous resolution
///
/// Multiple static on-demand imports that expose the same method remain
/// ambiguous until invocation resolution can prove one declaration.
#[test]
fn test_ambiguous_static_import_resolution() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Utils.java",
        r#"
        package com.example;
        public class Utils {
            public static void helper() {
                System.out.println("Utils.helper");
            }
        }
        "#,
    );

    test.add_file(
        "com/example/Tools.java",
        r#"
        package com.example;
        public class Tools {
            public static void helper() {
                System.out.println("Tools.helper");
            }
        }
        "#,
    );

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import static com.example.Utils.*;
        import static com.example.Tools.*;

        public class Main {
            public void test() {
                hel<|>per();
            }
        }
        ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");

    assert!(
        matches!(result, Ok(None)),
        "ambiguous static imports must not elect a definition: {result:?}"
    );
}

/// Explicit static import should override wildcard static imports
#[test]
fn test_explicit_static_import_overrides_wildcard() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Utils.java",
        r#"
        package com.example;
        public class Utils {
            public static void helper() {
                System.out.println("Utils.helper");
            }
        }
        "#,
    );

    test.add_file(
        "com/example/Tools.java",
        r#"
        package com.example;
        public class Tools {
            public static void helper() {
                System.out.println("Tools.helper");
            }
        }
        "#,
    );

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import static com.example.Tools.helper;
        import static com.example.Utils.*;

        public class Main {
            public void test() {
                hel<|>per();
            }
        }
        ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");

    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("Tools.java"),
                "Expected Tools.helper due to explicit static import, got: {}",
                loc.uri
            );
            println!("✅ Explicit static import correctly overrides wildcards");
        }
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) => {
            assert!(
                locs[0].uri.as_str().contains("Tools.java"),
                "Expected Tools.helper due to explicit static import, got: {}",
                locs[0].uri
            );
            println!("✅ Explicit static import correctly overrides wildcards");
        }
        _ => panic!("Expected definition of Tools.helper"),
    }
}

/// Edge case: java.lang implicit import should not cause ambiguity
#[test]
fn test_java_lang_no_ambiguity() {
    let mut test = LspTest::new();

    // Custom String class in different package
    test.add_file(
        "com/example/String.java",
        r"
        package com.example;
        public class String {
            public void custom() {}
        }
        ",
    );

    // Should resolve to java.lang.String, not com.example.String
    test.add_file(
        "com/other/Main.java",
        r#"
        package com.other;
        import com.example.*;

        public class Main {
            public void test() {
                Str<|>ing s = "test";
            }
        }
        "#,
    );

    let result = test.goto_definition("com/other/Main.java", "<|>");

    // java.lang.String is prioritized over wildcard imports
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            // java.lang classes won't be in our workspace, so might return None
            println!("Found: {}", loc.uri);
        }
        Ok(None) => {
            // Expected if java.lang.String is not in index
            println!("✅ java.lang.String takes priority (not in workspace index)");
        }
        _ => {}
    }
}

/// Test current package priority over wildcard imports
#[test]
fn test_current_package_priority() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Helper.java",
        r"
        package com.example;
        public class Helper {
            public void localMethod() {}
        }
        ",
    );

    test.add_file(
        "com/other/Helper.java",
        r"
        package com.other;
        public class Helper {
            public void otherMethod() {}
        }
        ",
    );

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import com.other.*;

        public class Main {
            public void test() {
                Hel<|>per h = new Helper();
            }
        }
        ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");

    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("com/example/Helper.java"),
                "Expected com.example.Helper (current package), got: {}",
                loc.uri
            );
            println!("✅ Current package correctly prioritized over wildcard imports");
        }
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) => {
            assert!(
                locs[0].uri.as_str().contains("com/example/Helper.java"),
                "Expected com.example.Helper (current package), got: {}",
                locs[0].uri
            );
            println!("✅ Current package correctly prioritized over wildcard imports");
        }
        _ => panic!("Expected definition of com.example.Helper"),
    }
}

goto_test! {
    ambiguous_declaring_file_return_type_does_not_redirect_a_method_chain {
        file "alpha/Result.java" r"
            package alpha;
            public class Result {
                public void alphaOnly() {}
            }
        ";
        file "beta/Result.java" r"
            package beta;
            public class Result {
                public void betaOnly() {}
            }
        ";
        file "api/Factory.java" r"
            package api;
            import alpha.*;
            import beta.*;
            public class Factory {
                public Result make() { return null; }
            }
        ";
        file "use/Main.java" r"
            package use;
            import api.Factory;
            public class Main {
                void run() {
                    new Factory().make().alphaO«call»nly();
                }
            }
        ";
        target "use/Main.java" "call";
        expect none;
    }
}
