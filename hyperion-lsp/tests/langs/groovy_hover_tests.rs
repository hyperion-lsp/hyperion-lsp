// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// Integration tests for Gradle hover functionality
///
/// Tests hover information when hovering over Gradle DSL methods and constructs
use hyperion_lsp::groovy_support::GradleSymbolExtractor;
use lsp_types::Position;

// =============================================================================
// LSP-level hover tests using the test macro
// =============================================================================

lsp_hover_test! {
    name: groovy_class_reference_hover,

    files: {
        "com/example/Main.groovy" => r"
            package com.example

            class Main {
                void run() {
                    Us<|>er user = new User()
                }
            }
        ",
        "com/example/User.groovy" => r"
            package com.example

            class User {
                String name
            }
        "
    },

    hover: "com/example/Main.groovy", "<|>",
    contains: ["User"]
}

lsp_hover_test! {
    name: groovy_method_call_hover,

    files: {
        "com/example/Main.groovy" => r#"
            package com.example

            class Main {
                void run() {
                    def helper = new Helper()
                    helper.gre<|>et("World")
                }
            }
        "#,
        "com/example/Helper.groovy" => r#"
            package com.example

            class Helper {
                void greet(String name) {
                    println "Hello, $name"
                }
            }
        "#
    },

    hover: "com/example/Main.groovy", "<|>",
    contains: ["greet"]
}

lsp_hover_test! {
    name: groovy_property_hover,

    files: {
        "com/example/Main.groovy" => r"
            package com.example

            class Main {
                void run() {
                    def user = new User()
                    println user.na<|>me
                }
            }
        ",
        "com/example/User.groovy" => r#"
            package com.example

            class User {
                String name = "John"
            }
        "#
    },

    hover: "com/example/Main.groovy", "<|>",
    contains: ["name"]
}

// Gradle DSL delegate type tests
lsp_hover_test! {
    name: gradle_dependencies_delegate_hover,

    files: {
        "build.gradle" => r"
            dependencies {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["DependencyHandler"]
}

lsp_hover_test! {
    name: gradle_repositories_delegate_hover,

    files: {
        "build.gradle" => r"
            repositories {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["RepositoryHandler"]
}

// Closure implicit 'it' parameter
lsp_hover_test! {
    name: groovy_closure_it_parameter_hover,

    files: {
        "com/example/Main.groovy" => r"
            package com.example

            class Main {
                void run() {
                    [1, 2, 3].each { i<|>t }
                }
            }
        "
    },

    hover: "com/example/Main.groovy", "<|>",
    contains: ["it"]
}

// def type inference - hover shows inferred type
lsp_hover_test! {
    name: groovy_def_variable_hover,

    files: {
        "com/example/Main.groovy" => r"
            package com.example

            class Main {
                void run() {
                    def us<|>er = new User()
                }
            }
        ",
        "com/example/User.groovy" => r"
            package com.example

            class User {
                String name
            }
        "
    },

    hover: "com/example/Main.groovy", "<|>",
    contains: ["User"]
}

// Cross-language hover (Groovy -> Java)
lsp_hover_test! {
    name: groovy_to_java_class_hover,

    files: {
        "com/example/Main.groovy" => r"
            package com.example

            class Main {
                void run() {
                    Java<|>Service service = new JavaService()
                }
            }
        ",
        "com/example/JavaService.java" => r#"
            package com.example;

            public class JavaService {
                public void process() {
                    System.out.println("Processing");
                }
            }
        "#
    },

    hover: "com/example/Main.groovy", "<|>",
    contains: ["JavaService"]
}

// =============================================================================
// GradleSymbolExtractor direct tests
// =============================================================================

#[test]
fn gradle_hover_on_implementation_method() {
    let content = r"dependencies {
    implementation 'junit:junit:4.13'
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Hover over "implementation" on line 2 (0-indexed: line 1)
    let position = Position {
        line: 1,
        character: 4, // Position within "implementation"
    };

    let symbol = extractor.symbol_at(&result.symbols, position);
    assert!(symbol.is_some(), "Should find symbol at hover position");

    let sym = symbol.unwrap();
    assert_eq!(sym.symbol.name, "implementation");
    assert!(
        sym.symbol.gradle_method.is_some(),
        "Should have Gradle method info"
    );

    let method = sym.symbol.gradle_method.unwrap();
    assert_eq!(method.signature.name, "implementation");
    assert!(method.description.contains("implementation dependency"));
}

#[test]
fn gradle_hover_on_mavencentral_method() {
    let content = r"repositories {
    mavenCentral()
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Hover over "mavenCentral" on line 2 (0-indexed: line 1)
    let position = Position {
        line: 1,
        character: 4,
    };

    let symbol = extractor.symbol_at(&result.symbols, position);
    assert!(symbol.is_some());

    let sym = symbol.unwrap();
    assert_eq!(sym.symbol.name, "mavenCentral");
    assert!(sym.symbol.gradle_method.is_some());

    let method = sym.symbol.gradle_method.unwrap();
    assert_eq!(method.block, "repositories");
}

#[test]
fn gradle_hover_on_unknown_method() {
    let content = r"myCustomMethod 'value'";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Hover over "myCustomMethod"
    let position = Position {
        line: 0,
        character: 5,
    };

    let sym = extractor
        .symbol_at(&result.symbols, position)
        .expect("unknown Groovy method must still produce a navigable symbol");
    assert_eq!(sym.symbol.name, "myCustomMethod");
    assert!(sym.symbol.gradle_method.is_none());
}

#[test]
fn gradle_hover_on_dependency_coordinate() {
    let content = r"dependencies {
    implementation 'com.google.guava:guava:30.1.1-jre'
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Hover over the dependency method name
    let position = Position {
        line: 1,
        character: 4,
    };

    let symbol = extractor.symbol_at(&result.symbols, position);
    assert!(symbol.is_some());

    let sym = symbol.unwrap();
    assert_eq!(sym.symbol.name, "implementation");
}

#[test]
fn gradle_hover_multiple_methods() {
    let content = r"dependencies {
    implementation 'junit:junit:4.13'
    testImplementation 'org.mockito:mockito-core:3.6.0'
    api 'com.example:lib:1.0'
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert!(
        result.symbols.len() >= 3,
        "Should extract at least 3 methods, got {}",
        result.symbols.len()
    );

    // Check that we can find the known methods
    let names: Vec<String> = result
        .symbols
        .iter()
        .map(|s| s.symbol.name.clone())
        .collect();
    assert!(names.contains(&"implementation".to_string()));
    assert!(names.contains(&"testImplementation".to_string()));
    assert!(names.contains(&"api".to_string()));

    // Known methods should have gradle_method info
    for sym in result
        .symbols
        .iter()
        .filter(|s| s.symbol.gradle_method.is_some())
    {
        // Verify known methods are recognized
        assert!(
            matches!(
                sym.symbol.name.as_str(),
                "implementation" | "testImplementation" | "api" | "dependencies"
            ),
            "Unexpected method with gradle_method: {}",
            sym.symbol.name
        );
    }
}

#[test]
fn gradle_hover_position_boundaries() {
    let content = "implementation 'junit:junit:4.13'";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert_eq!(result.symbols.len(), 1);
    let sym_info = &result.symbols[0];

    // Hover at start of method name
    let position_start = Position {
        line: 0,
        character: sym_info.range.start.character,
    };
    assert!(extractor
        .symbol_at(&result.symbols, position_start)
        .is_some());

    // Hover at end of method name
    let position_end = Position {
        line: 0,
        character: sym_info.range.end.character - 1,
    };
    assert!(extractor.symbol_at(&result.symbols, position_end).is_some());

    // Hover after method name should find nothing
    let position_after = Position {
        line: 0,
        character: sym_info.range.end.character + 1,
    };
    assert!(extractor
        .symbol_at(&result.symbols, position_after)
        .is_none());
}

#[test]
fn gradle_hover_empty_file() {
    let content = "";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert_eq!(result.symbols.len(), 0);

    let position = Position {
        line: 0,
        character: 0,
    };
    assert!(extractor.symbol_at(&result.symbols, position).is_none());
}

#[test]
fn gradle_hover_with_comments() {
    let content = r"
// This is a comment
dependencies {
    // Inside dependencies block
    implementation 'junit:junit:4.13'  // inline comment
}
";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Should still find the implementation method despite comments
    assert!(
        !result.symbols.is_empty(),
        "Should find at least the implementation method"
    );
    let has_implementation = result
        .symbols
        .iter()
        .any(|s| s.symbol.name == "implementation");
    assert!(has_implementation, "Should find implementation method");
}

#[test]
fn gradle_format_hover_text_known_method() {
    use hyperion_lsp::groovy_support::format_hover_text;

    let content = "implementation 'junit:junit:4.13'";
    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    let sym_info = &result.symbols[0];
    let hover_text = format_hover_text(sym_info);

    // Check that hover text contains expected documentation
    assert!(hover_text.contains("implementation"));
    assert!(
        hover_text.contains("dependencies"),
        "Should mention block context"
    );
    assert!(hover_text.contains("Add implementation dependency"));
}

#[test]
fn gradle_format_hover_text_unknown_method() {
    let content = "customMethod 'value'";
    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert_eq!(result.symbols.len(), 1);
    let sym_info = &result.symbols[0];
    let hover_text = hyperion_lsp::groovy_support::format_hover_text(sym_info);
    assert!(hover_text.contains("customMethod"));
}

// =============================================================================
// Additional Gradle DSL block delegate tests
// =============================================================================

lsp_hover_test! {
    name: gradle_buildscript_delegate_hover,

    files: {
        "build.gradle" => r"
            buildscript {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["ScriptHandler"]
}

lsp_hover_test! {
    name: gradle_publishing_delegate_hover,

    files: {
        "build.gradle" => r"
            publishing {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["PublishingExtension"]
}

lsp_hover_test! {
    name: gradle_java_delegate_hover,

    files: {
        "build.gradle" => r"
            java {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["JavaPluginExtension"]
}

lsp_hover_test! {
    name: gradle_kotlin_delegate_hover,

    files: {
        "build.gradle" => r"
            kotlin {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["KotlinProjectExtension"]
}

lsp_hover_test! {
    name: gradle_android_delegate_hover,

    files: {
        "build.gradle" => r"
            android {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["BaseExtension"]
}

lsp_hover_test! {
    name: gradle_signing_delegate_hover,

    files: {
        "build.gradle" => r"
            signing {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["SigningExtension"]
}

lsp_hover_test! {
    name: gradle_jacoco_delegate_hover,

    files: {
        "build.gradle" => r"
            jacoco {
                dele<|>gate
            }
        "
    },

    hover: "build.gradle", "<|>",
    contains: ["JacocoPluginExtension"]
}
