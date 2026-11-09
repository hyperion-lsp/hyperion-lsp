// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// Integration tests for Gradle code completion functionality
///
/// Tests code completion suggestions when editing Gradle DSL (*.gradle, build.gradle files)
/// These tests verify that the Gradle symbol extractor can identify completable symbols
/// which are then used by the completion provider.
use hyperion_lsp::groovy_support::GradleSymbolExtractor;

#[test]
fn gradle_completion_extracts_dependency_methods() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"dependencies {
    implementation 'junit:junit:4.13'
    api 'com.example:lib:1.0'
    testImplementation 'org.mockito:mockito-core:3.6.0'
}";

    let result = extractor.extract(source);

    // Should extract dependency methods
    let names: Vec<String> = result
        .symbols
        .iter()
        .map(|s| s.symbol.name.clone())
        .collect();
    assert!(
        names.contains(&"implementation".to_string()),
        "Should extract 'implementation' method"
    );
    assert!(
        names.contains(&"api".to_string()),
        "Should extract 'api' method"
    );
    assert!(
        names.contains(&"testImplementation".to_string()),
        "Should extract 'testImplementation' method"
    );

    // All extracted symbols should be Gradle methods
    for sym in &result.symbols {
        assert!(
            sym.symbol.gradle_method.is_some(),
            "Symbol '{}' should be a known Gradle method",
            sym.symbol.name
        );
    }
}

#[test]
fn gradle_completion_extracts_repository_methods() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"repositories {
    mavenCentral()
    google()
    mavenLocal()
}";

    let result = extractor.extract(source);

    let names = result
        .symbols
        .iter()
        .map(|symbol| symbol.symbol.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["repositories", "mavenCentral", "google", "mavenLocal"]
    );

    // Methods that are extracted should be repository methods
    // (Some methods might not be extracted by the regex-based extractor)
    // Skip the 'repositories' block name itself
    for sym in &result.symbols {
        if sym.symbol.name == "repositories" {
            continue; // Skip the block declaration
        }
        let method = sym
            .symbol
            .gradle_method
            .as_ref()
            .expect("repository calls must carry Gradle metadata");
        assert_eq!(method.block, "repositories");
    }
}

#[test]
fn gradle_completion_extracts_plugin_methods() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"plugins {
    id 'java'
    id 'org.springframework.boot' version '2.6.0'
}";

    let result = extractor.extract(source);

    // Should extract plugin id method
    assert!(
        result.symbols.iter().any(|s| s.symbol.name == "id"),
        "Should extract 'id' method"
    );

    // Plugin methods should be identified
    assert!(
        result.symbols.iter().any(|s| s.symbol.name == "id"),
        "Should find 'id' symbol for completion"
    );
}

#[test]
fn gradle_completion_extracts_android_methods() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"android {
    compileSdkVersion 31
    defaultConfig {
        applicationId 'com.example.app'
        minSdkVersion 21
    }
    buildTypes {
        release {
            minifyEnabled false
        }
    }
}";

    let result = extractor.extract(source);

    let names = result
        .symbols
        .iter()
        .map(|symbol| symbol.symbol.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "android",
            "compileSdkVersion",
            "defaultConfig",
            "applicationId",
            "minSdkVersion",
            "buildTypes",
            "release",
            "minifyEnabled"
        ]
    );

    // Verify that whatever is extracted are recognized as Gradle methods
    for sym in &result.symbols {
        assert!(
            sym.symbol.gradle_method.is_some(),
            "Android property '{}' should be a known Gradle method",
            sym.symbol.name
        );
    }
}

#[test]
fn gradle_completion_extracts_task_methods() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"tasks {
    register('myTask') {
        doLast {
            println 'Task executed'
        }
    }
    named('build') {
        dependsOn 'myTask'
    }
}";

    let result = extractor.extract(source);

    // Should extract task registration methods
    let names: Vec<String> = result
        .symbols
        .iter()
        .map(|s| s.symbol.name.clone())
        .collect();
    assert!(
        names.contains(&"register".to_string()),
        "Should extract 'register' task method"
    );
    assert!(
        names.contains(&"named".to_string()),
        "Should extract 'named' task method"
    );
}

#[test]
fn gradle_completion_extracts_multiple_block_types() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"plugins {
    id 'java'
}

dependencies {
    implementation 'junit:junit:4.13'
}

repositories {
    mavenCentral()
}

tasks {
    register('build') {
    }
}";

    let result = extractor.extract(source);

    // Should extract symbols from all block types
    let names: Vec<String> = result
        .symbols
        .iter()
        .map(|s| s.symbol.name.clone())
        .collect();
    assert!(
        names.contains(&"id".to_string()),
        "Should extract plugin id"
    );
    assert!(
        names.contains(&"implementation".to_string()),
        "Should extract dependency method"
    );
    assert!(
        names.contains(&"mavenCentral".to_string()),
        "Should extract repository method"
    );
    assert!(
        names.contains(&"register".to_string()),
        "Should extract task method"
    );
}

#[test]
fn gradle_completion_symbol_ranges_are_valid() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"dependencies {
    implementation 'junit:junit:4.13'
    api 'com.example:lib:1.0'
    compileOnly 'javax.servlet:servlet-api:2.5'
}";

    let result = extractor.extract(source);

    // All symbols should have valid ranges
    for (i, sym) in result.symbols.iter().enumerate() {
        assert!(
            sym.range.start.line <= sym.range.end.line,
            "Symbol {} ({}): start line {} > end line {}",
            i,
            sym.symbol.name,
            sym.range.start.line,
            sym.range.end.line
        );

        if sym.range.start.line == sym.range.end.line {
            assert!(
                sym.range.start.character <= sym.range.end.character,
                "Symbol {} ({}): on same line, start char {} > end char {}",
                i,
                sym.symbol.name,
                sym.range.start.character,
                sym.range.end.character
            );
        }

        // Range should contain the symbol name
        assert!(
            !sym.symbol.name.is_empty(),
            "Symbol {i} should have non-empty name"
        );
    }
}

#[test]
fn gradle_completion_preserves_indented_symbol_columns() {
    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract("    implementation 'junit:junit:4.13'");
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].range.start.character, 4);
    assert_eq!(result.symbols[0].range.end.character, 18);
}

#[test]
fn gradle_completion_with_comments_and_whitespace() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"
// Configure dependencies
dependencies {
    // Main dependencies
    implementation 'junit:junit:4.13'  // Unit testing

    // Repository-specific dependencies
    api 'com.example:lib:1.0'
}
";

    let result = extractor.extract(source);

    // Should extract methods despite comments and whitespace
    let names: Vec<String> = result
        .symbols
        .iter()
        .map(|s| s.symbol.name.clone())
        .collect();
    assert!(
        names.contains(&"implementation".to_string()),
        "Should find 'implementation' despite comments"
    );
    assert!(
        names.contains(&"api".to_string()),
        "Should find 'api' despite comments"
    );
}

#[test]
fn gradle_completion_nested_blocks() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"dependencies {
    implementation('junit:junit:4.13') {
        exclude group: 'org.junit'
    }
}";

    let result = extractor.extract(source);

    // Should extract the outer method even with nested configuration
    assert!(
        result
            .symbols
            .iter()
            .any(|s| s.symbol.name == "implementation"),
        "Should extract 'implementation' from nested block"
    );
}

#[test]
fn gradle_completion_common_methods_have_documentation() {
    let extractor = GradleSymbolExtractor::new();
    let common_methods = vec![
        ("implementation", "dependencies"),
        ("api", "dependencies"),
        ("mavenCentral", "repositories"),
        ("google", "repositories"),
        ("id", "plugins"),
    ];

    for (method_name, block) in common_methods {
        let source = format!(r"{block} {{ {method_name} }}");
        let result = extractor.extract(&source);

        let sym = result
            .symbols
            .iter()
            .find(|symbol| symbol.symbol.name == method_name)
            .unwrap_or_else(|| panic!("must extract documented Gradle method '{method_name}'"));
        let gradle_method = sym
            .symbol
            .gradle_method
            .as_ref()
            .unwrap_or_else(|| panic!("'{method_name}' must carry Gradle metadata"));
        assert!(
            !gradle_method.description.is_empty(),
            "Method '{method_name}' should have documentation"
        );
    }
}

#[test]
fn gradle_completion_grouping_by_block() {
    let extractor = GradleSymbolExtractor::new();
    let source = r"
dependencies {
    implementation 'junit:junit:4.13'
}

repositories {
    mavenCentral()
}

android {
    compileSdkVersion 31
}
";

    let result = extractor.extract(source);

    // Count symbols by block type
    let deps_methods: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| {
            s.symbol
                .gradle_method
                .as_ref()
                .is_some_and(|m| m.block == "dependencies")
        })
        .collect();
    let repo_methods: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| {
            s.symbol
                .gradle_method
                .as_ref()
                .is_some_and(|m| m.block == "repositories")
        })
        .collect();
    let android_methods: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| {
            s.symbol
                .gradle_method
                .as_ref()
                .is_some_and(|m| m.block == "android")
        })
        .collect();

    assert_eq!(deps_methods.len(), 1, "implementation must be grouped");
    assert_eq!(repo_methods.len(), 1, "mavenCentral must be grouped");
    assert_eq!(
        android_methods.len(),
        1,
        "compileSdkVersion must be grouped"
    );

    // Verify that extracted methods are in the expected blocks
    for method in deps_methods {
        assert!(
            method
                .symbol
                .gradle_method
                .as_ref()
                .is_some_and(|m| m.block == "dependencies"),
            "Method should be in dependencies block"
        );
    }
    for method in repo_methods {
        assert!(
            method
                .symbol
                .gradle_method
                .as_ref()
                .is_some_and(|m| m.block == "repositories"),
            "Method should be in repositories block"
        );
    }
    for method in android_methods {
        assert!(
            method
                .symbol
                .gradle_method
                .as_ref()
                .is_some_and(|m| m.block == "android"),
            "Method should be in android block"
        );
    }
}
