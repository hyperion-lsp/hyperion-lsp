// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for Gradle DSL symbol extraction
//!
//! These tests verify that the symbol extractor correctly handles typical Gradle build files.

use hyperion_groovy::SymbolExtractor;

#[test]
fn extract_simple_build_gradle() {
    let source = r#"
plugins {
    id 'java'
    id 'org.springframework.boot' version '2.7.0'
}

dependencies {
    implementation 'com.google.guava:guava:31.1-jre'
    testImplementation 'junit:junit:4.13.2'
    runtimeOnly 'org.postgresql:postgresql:42.3.1'
}

repositories {
    mavenCentral()
    mavenLocal()
}
"#;

    let extractor = SymbolExtractor::new();
    let result = extractor.extract(source);

    // Should recognize all Gradle DSL methods
    assert!(result.symbols.iter().any(|s| s.name == "implementation"));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.name == "testImplementation"));
    assert!(result.symbols.iter().any(|s| s.name == "runtimeOnly"));
    assert!(result.symbols.iter().any(|s| s.name == "mavenCentral"));

    // Should extract dependencies
    assert!(!result.dependencies.is_empty());

    // All symbols should be marked as Gradle API
    for symbol in &result.symbols {
        assert!(
            symbol.is_gradle_api,
            "Symbol {} should be recognized as Gradle API",
            symbol.name
        );
    }
}

#[test]
fn extract_multi_module_root_build_gradle() {
    let source = r#"
plugins {
    id 'java'
}

allprojects {
    group = "com.example"
    version = "1.0.0"
}

subprojects {
    apply plugin: 'java'

    repositories {
        mavenCentral()
    }

    dependencies {
        implementation 'com.google.guava:guava:31.1-jre'
    }
}
"#;

    let extractor = SymbolExtractor::new();
    let result = extractor.extract(source);

    // Should recognize project configuration
    assert!(result.symbols.iter().any(|s| s.name == "allprojects"));
    assert!(result.symbols.iter().any(|s| s.name == "subprojects"));
}

#[test]
fn parse_dependency_coordinates() {
    let extractor = SymbolExtractor::new();

    // Test various Maven coordinate formats
    let sources = vec![
        (
            "implementation 'org.slf4j:slf4j-api:1.7.32'",
            "org.slf4j",
            "slf4j-api",
            "1.7.32",
        ),
        (
            "implementation 'com.fasterxml.jackson.core:jackson-databind:2.13.0'",
            "com.fasterxml.jackson.core",
            "jackson-databind",
            "2.13.0",
        ),
        (
            "testImplementation 'junit:junit:4.13.2'",
            "junit",
            "junit",
            "4.13.2",
        ),
    ];

    for (source, expected_group, expected_artifact, expected_version) in sources {
        let result = extractor.extract(source);
        assert!(
            !result.dependencies.is_empty(),
            "Failed to parse: {}",
            source
        );
        let dep = &result.dependencies[0];
        assert_eq!(dep.group, expected_group, "Group mismatch in: {}", source);
        assert_eq!(
            dep.artifact, expected_artifact,
            "Artifact mismatch in: {}",
            source
        );
        assert_eq!(
            dep.version, expected_version,
            "Version mismatch in: {}",
            source
        );
    }
}

#[test]
fn recognize_gradle_methods() {
    let extractor = SymbolExtractor::new();

    let gradle_methods = vec![
        "implementation 'junit:junit:4.13'",
        "testImplementation 'junit:junit:4.13'",
        "api 'com.example:lib:1.0'",
        "compileOnly 'javax.servlet:javax.servlet-api:3.1.0'",
        "runtimeOnly 'org.postgresql:postgresql:42.3.1'",
        "mavenCentral()",
        "mavenLocal()",
        "register('task', DefaultTask)",
        "doLast { println 'Done' }",
    ];

    for method_line in gradle_methods {
        let result = extractor.extract(method_line);
        assert!(
            !result.symbols.is_empty(),
            "Failed to recognize: {}",
            method_line
        );
        let symbol = &result.symbols[0];
        assert!(
            symbol.is_gradle_api,
            "{} should be recognized as Gradle API",
            method_line
        );
    }
}

#[test]
fn ignore_comments_and_empty_lines() {
    let source = r#"
// This is a comment
dependencies {
    // Dependency comment
    implementation 'junit:junit:4.13' // inline comment

    // Another comment
}
"#;

    let extractor = SymbolExtractor::new();
    let result = extractor.extract(source);

    // Should still extract despite comments
    assert!(result.symbols.iter().any(|s| s.name == "implementation"));
    assert!(!result.dependencies.is_empty());
    assert!(result.errors.is_empty());
}

#[test]
fn symbol_has_location_info() {
    let source = "implementation 'junit:junit:4.13'";

    let extractor = SymbolExtractor::new();
    let result = extractor.extract(source);

    assert!(!result.symbols.is_empty());
    let symbol = &result.symbols[0];

    // Should have location information
    assert_eq!(symbol.line, 1);
    assert!(symbol.column < 1000); // Reasonable upper bound for column
    assert_eq!(symbol.name.as_str(), "implementation");
}
