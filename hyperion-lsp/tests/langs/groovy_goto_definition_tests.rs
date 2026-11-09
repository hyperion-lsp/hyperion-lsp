// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// Integration tests for Gradle goto-definition functionality
///
/// Tests goto-definition navigation when clicking on Gradle DSL methods and constructs
use hyperion_lsp::groovy_support::GradleSymbolExtractor;
use lsp_types::Position;

#[test]
fn gradle_goto_on_implementation_method() {
    let content = r"dependencies {
    implementation 'junit:junit:4.13'
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Goto from "implementation" on line 2 (0-indexed: line 1)
    let position = Position {
        line: 1,
        character: 4, // Position within "implementation"
    };

    let symbol = extractor.symbol_at(&result.symbols, position);
    assert!(symbol.is_some(), "Should find symbol at goto position");

    let sym = symbol.unwrap();
    assert_eq!(sym.symbol.name, "implementation");
    // The range should point to the symbol location
    assert_eq!(sym.range.start.line, 1);
    assert!(sym.range.start.character < sym.range.end.character);
}

#[test]
fn gradle_goto_on_mavencentral_repository() {
    let content = r"repositories {
    mavenCentral()
    google()
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Goto from "mavenCentral" on line 2
    let position = Position {
        line: 1,
        character: 4,
    };

    let symbol = extractor.symbol_at(&result.symbols, position);
    assert!(symbol.is_some());

    let sym = symbol.unwrap();
    assert_eq!(sym.symbol.name, "mavenCentral");
    assert_eq!(sym.range.start.line, 1);
}

#[test]
fn gradle_goto_multiple_methods_in_block() {
    let content = r"dependencies {
    implementation 'junit:junit:4.13'
    testImplementation 'org.mockito:mockito-core:3.6.0'
    api 'com.example:lib:1.0'
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Goto each method and verify they have proper ranges
    let symbols = &result.symbols;
    assert!(
        symbols.len() >= 3,
        "Should find at least 3 methods, got {}",
        symbols.len()
    );

    for (i, sym) in symbols.iter().enumerate() {
        // Verify each symbol has a valid range (start <= end)
        assert!(
            sym.range.start <= sym.range.end,
            "Symbol {} range should be valid: start {:?} <= end {:?}",
            i,
            sym.range.start,
            sym.range.end
        );
        // For multi-line ranges, this might be true; for single-line, char range should be non-empty
        if sym.range.start.line == sym.range.end.line {
            assert!(
                sym.range.start.character <= sym.range.end.character,
                "Symbol {i} should have valid character range on same line"
            );
        }
    }
}

#[test]
fn gradle_goto_task_registration() {
    let content = r"tasks.register('myTask') {
    doLast {
        println 'Task executed'
    }
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Should find the register method
    let position = Position {
        line: 0,
        character: 6, // Position within "register"
    };

    let sym = extractor
        .symbol_at(&result.symbols, position)
        .expect("tasks.register must be navigable");
    assert_eq!(sym.symbol.name, "register");
    assert_eq!(sym.range.start.line, 0);
}

#[test]
fn gradle_goto_plugin_application() {
    let content = r"plugins {
    id 'java'
    id 'org.springframework.boot' version '2.6.0'
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert_eq!(
        result
            .symbols
            .iter()
            .map(|symbol| symbol.symbol.name.as_str())
            .collect::<Vec<_>>(),
        ["plugins", "id", "id", "version"]
    );
    for sym in &result.symbols {
        // Verify ranges are valid
        assert!(
            sym.range.start.character < sym.range.end.character || sym.range.start == sym.range.end
        );
    }
}

#[test]
fn gradle_goto_with_nested_blocks() {
    let content = r"android {
    compileSdkVersion 31
    defaultConfig {
        applicationId 'com.example.app'
        minSdkVersion 21
    }
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert_eq!(
        result
            .symbols
            .iter()
            .map(|symbol| symbol.symbol.name.as_str())
            .collect::<Vec<_>>(),
        [
            "android",
            "compileSdkVersion",
            "defaultConfig",
            "applicationId",
            "minSdkVersion"
        ]
    );
    for sym in &result.symbols {
        assert!(!sym.symbol.name.is_empty());
        assert!(
            sym.range.start.line < 10,
            "Symbol should be within file bounds"
        );
    }
}

#[test]
fn gradle_goto_position_boundaries_for_method() {
    let content = "testImplementation 'junit:junit:4.13'";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert_eq!(result.symbols.len(), 1);
    let sym_info = &result.symbols[0];

    // Test goto at different positions within the method name
    // Start of method
    let position_start = Position {
        line: 0,
        character: sym_info.range.start.character,
    };
    assert!(extractor
        .symbol_at(&result.symbols, position_start)
        .is_some());

    // Middle of method
    let mid_char = sym_info.range.start.character
        + (sym_info.range.end.character - sym_info.range.start.character) / 2;
    let position_mid = Position {
        line: 0,
        character: mid_char,
    };
    assert!(extractor.symbol_at(&result.symbols, position_mid).is_some());

    // End of method
    let position_end = Position {
        line: 0,
        character: sym_info.range.end.character - 1,
    };
    assert!(extractor.symbol_at(&result.symbols, position_end).is_some());
}

#[test]
fn gradle_goto_empty_file() {
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
fn gradle_goto_multiline_dependency_block() {
    let content = r"dependencies {
    implementation(
        'junit:junit:4.13'
    )
}";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    // Should find the implementation method
    let has_implementation = result
        .symbols
        .iter()
        .any(|s| s.symbol.name == "implementation");
    assert!(has_implementation, "Should find implementation method");
}

#[test]
fn gradle_goto_returns_correct_range() {
    let content = "api 'com.example:library:1.0'";

    let extractor = GradleSymbolExtractor::new();
    let result = extractor.extract(content);

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];

    // Verify the range encompasses the method name "api"
    let range = &sym.range;
    assert_eq!(range.start.line, 0);
    assert_eq!(range.end.line, 0);
    assert!(range.end.character > range.start.character);

    // The text within the range should be "api"
    let start_char = range.start.character as usize;
    let end_char = range.end.character as usize;
    let text_in_range = &content[start_char..end_char];
    assert!(text_in_range.contains("api"));
}
