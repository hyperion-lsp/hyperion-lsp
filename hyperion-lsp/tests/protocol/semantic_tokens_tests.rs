// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for semantic tokens (syntax highlighting)
//!
//! Semantic tokens provide rich syntax highlighting based on semantic analysis,
//! enabling editors to distinguish between classes, methods, fields, variables, etc.

use hyperion_lsp::semantic_tokens::SemanticTokensProvider;
use hyperion_semantics::db::{Database, FileId};
use parking_lot::Mutex;
use std::sync::Arc;
use tower_lsp::lsp_types::SemanticToken;

/// Token type indices (must match the order in semantic_tokens.rs TOKEN_TYPES)
mod token_types {
    pub const CLASS: u32 = 0;
    pub const INTERFACE: u32 = 1;
    pub const ENUM: u32 = 2;
    pub const TYPE_PARAMETER: u32 = 3;
    pub const METHOD: u32 = 4;
    pub const PROPERTY: u32 = 5;
    pub const VARIABLE: u32 = 6;
    pub const PARAMETER: u32 = 7;
    pub const KEYWORD: u32 = 8;
    pub const MODIFIER: u32 = 9;
    pub const COMMENT: u32 = 10;
    pub const STRING: u32 = 11;
    pub const NUMBER: u32 = 12;
}

/// Token modifier bit flags (must match the order in semantic_tokens.rs TOKEN_MODIFIERS)
mod token_modifiers {
    pub const DECLARATION: u32 = 1 << 0;
    pub const STATIC: u32 = 1 << 3;
    pub const ABSTRACT: u32 = 1 << 4;
}

/// Helper to get semantic tokens for Java source code
fn get_semantic_tokens(source: &str) -> Vec<SemanticToken> {
    let host = hyperion_lsp::semantic_host::SemanticHost::for_tests();
    let file_id = FileId::new(0);
    host.with_db(|db| {
        db.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
    });

    let provider = SemanticTokensProvider::new();

    let result = provider.semantic_tokens(&host.read_now(), file_id, source);
    match result {
        Ok(tokens) => tokens,
        Err(e) => {
            eprintln!("Semantic tokens error: {e}");
            vec![]
        }
    }
}

/// Find tokens of a specific type
fn tokens_of_type(tokens: &[SemanticToken], token_type: u32) -> Vec<&SemanticToken> {
    tokens
        .iter()
        .filter(|t| t.token_type == token_type)
        .collect()
}

/// Check if any token has a specific modifier
const fn has_modifier(token: &SemanticToken, modifier: u32) -> bool {
    (token.token_modifiers_bitset & modifier) != 0
}

// ============================================================================
// Legend Tests
// ============================================================================

#[test]
fn test_legend_contains_required_types() {
    let legend = SemanticTokensProvider::legend();

    // Verify we have the basic token types
    assert!(
        legend.token_types.len() >= 10,
        "Should have at least 10 token types"
    );
    assert!(
        legend.token_modifiers.len() >= 4,
        "Should have at least 4 modifiers"
    );
}

// ============================================================================
// Class Declaration Tests
// ============================================================================

#[test]
fn test_class_declaration_token() {
    let source = "class MyClass {}";
    let tokens = get_semantic_tokens(source);

    let class_tokens = tokens_of_type(&tokens, token_types::CLASS);
    assert!(!class_tokens.is_empty(), "Should find class token");

    // Should have DECLARATION modifier
    assert!(
        has_modifier(class_tokens[0], token_modifiers::DECLARATION),
        "Class should have DECLARATION modifier"
    );
}

#[test]
fn test_interface_declaration_token() {
    let source = "interface MyInterface {}";
    let tokens = get_semantic_tokens(source);

    let interface_tokens = tokens_of_type(&tokens, token_types::INTERFACE);
    assert!(!interface_tokens.is_empty(), "Should find interface token");
}

#[test]
fn test_enum_declaration_token() {
    let source = "enum MyEnum { VALUE1, VALUE2 }";
    let tokens = get_semantic_tokens(source);

    let enum_tokens = tokens_of_type(&tokens, token_types::ENUM);
    assert!(!enum_tokens.is_empty(), "Should find enum token");
}

// ============================================================================
// Method Declaration Tests
// ============================================================================

#[test]
fn test_method_declaration_token() {
    let source = r"
class Test {
    void myMethod() {}
}
";
    let tokens = get_semantic_tokens(source);

    let method_tokens = tokens_of_type(&tokens, token_types::METHOD);
    assert!(!method_tokens.is_empty(), "Should find method token");
    assert!(
        has_modifier(method_tokens[0], token_modifiers::DECLARATION),
        "Method should have DECLARATION modifier"
    );
}

#[test]
fn test_static_method_has_static_modifier() {
    let source = r"
class Test {
    static void staticMethod() {}
}
";
    let tokens = get_semantic_tokens(source);

    let method_tokens = tokens_of_type(&tokens, token_types::METHOD);
    assert!(!method_tokens.is_empty(), "Should find method token");
    assert!(
        has_modifier(method_tokens[0], token_modifiers::STATIC),
        "Static method should have STATIC modifier"
    );
}

#[test]
fn test_abstract_method_has_abstract_modifier() {
    let source = r"
abstract class Test {
    abstract void abstractMethod();
}
";
    let tokens = get_semantic_tokens(source);

    let method_tokens = tokens_of_type(&tokens, token_types::METHOD);
    assert!(!method_tokens.is_empty(), "Should find method token");
    assert!(
        has_modifier(method_tokens[0], token_modifiers::ABSTRACT),
        "Abstract method should have ABSTRACT modifier"
    );
}

// ============================================================================
// Parameter Tests
// ============================================================================

#[test]
fn test_parameter_token() {
    let source = r"
class Test {
    void method(String param) {}
}
";
    let tokens = get_semantic_tokens(source);

    let param_tokens = tokens_of_type(&tokens, token_types::PARAMETER);
    assert!(!param_tokens.is_empty(), "Should find parameter token");
    assert!(
        has_modifier(param_tokens[0], token_modifiers::DECLARATION),
        "Parameter should have DECLARATION modifier"
    );
}

#[test]
fn test_multiple_parameters() {
    let source = r"
class Test {
    void method(String first, int second, Object third) {}
}
";
    let tokens = get_semantic_tokens(source);

    let param_tokens = tokens_of_type(&tokens, token_types::PARAMETER);
    assert_eq!(param_tokens.len(), 3, "Should find 3 parameter tokens");
}

// ============================================================================
// Variable Declaration Tests
// ============================================================================

#[test]
fn test_local_variable_token() {
    let source = r"
class Test {
    void method() {
        String localVar = null;
    }
}
";
    let tokens = get_semantic_tokens(source);

    let var_tokens = tokens_of_type(&tokens, token_types::VARIABLE);
    assert!(!var_tokens.is_empty(), "Should find variable token");
}

// ============================================================================
// Delta Encoding Tests
// ============================================================================

#[test]
fn test_tokens_are_sorted() {
    let source = r"
class First {}
class Second {}
class Third {}
";
    let tokens = get_semantic_tokens(source);

    // Tokens should be in order (delta encoding depends on this)
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for (i, token) in tokens.iter().enumerate() {
        // Reconstruct absolute position
        let abs_line = prev_line + token.delta_line;
        let abs_start = if token.delta_line == 0 {
            prev_start + token.delta_start
        } else {
            token.delta_start
        };

        // Each token should come after or at same position as previous
        assert!(
            abs_line > prev_line || (abs_line == prev_line && abs_start >= prev_start),
            "Token {} should come after previous token (line {}, col {} vs prev line {}, col {})",
            i,
            abs_line,
            abs_start,
            prev_line,
            prev_start
        );

        prev_line = abs_line;
        prev_start = abs_start;
    }
}

#[test]
fn test_multiline_class() {
    let source = r"
class Test {
    int field;
    void method() {}
}
";
    let tokens = get_semantic_tokens(source);

    // Should have tokens: class (Test), variable (field), method (method)
    assert!(
        tokens.len() >= 3,
        "Should have at least 3 tokens, got {}",
        tokens.len()
    );
}

// ============================================================================
// Complex Scenarios
// ============================================================================

#[test]
fn test_nested_class() {
    let source = r"
class Outer {
    class Inner {}
}
";
    let tokens = get_semantic_tokens(source);

    let class_tokens = tokens_of_type(&tokens, token_types::CLASS);
    assert_eq!(
        class_tokens.len(),
        2,
        "Should find 2 class tokens (Outer and Inner)"
    );
}

#[test]
fn test_class_with_all_member_types() {
    let source = r"
class Complete {
    private int field;

    public Complete(int param) {}

    public void method(String arg) {
        int local = 0;
    }
}
";
    let tokens = get_semantic_tokens(source);

    // Should have: class, field, constructor param, method, method param, local var
    assert!(
        tokens.len() >= 4,
        "Should have multiple tokens for complete class"
    );

    let class_tokens = tokens_of_type(&tokens, token_types::CLASS);
    assert!(!class_tokens.is_empty(), "Should have class token");

    let method_tokens = tokens_of_type(&tokens, token_types::METHOD);
    assert!(!method_tokens.is_empty(), "Should have method token");

    let param_tokens = tokens_of_type(&tokens, token_types::PARAMETER);
    assert!(!param_tokens.is_empty(), "Should have parameter tokens");
}

#[test]
fn test_interface_with_abstract_methods() {
    let source = r"
interface Service {
    void process();
    String getName();
}
";
    let tokens = get_semantic_tokens(source);

    let interface_tokens = tokens_of_type(&tokens, token_types::INTERFACE);
    assert!(!interface_tokens.is_empty(), "Should find interface token");

    let method_tokens = tokens_of_type(&tokens, token_types::METHOD);
    assert_eq!(method_tokens.len(), 2, "Should find 2 method tokens");
}

#[test]
fn test_enum_with_methods() {
    let source = r"
enum Status {
    ACTIVE, INACTIVE;

    public boolean isActive() {
        return this == ACTIVE;
    }
}
";
    let tokens = get_semantic_tokens(source);

    let enum_tokens = tokens_of_type(&tokens, token_types::ENUM);
    assert!(!enum_tokens.is_empty(), "Should find enum token");

    let method_tokens = tokens_of_type(&tokens, token_types::METHOD);
    assert!(
        !method_tokens.is_empty(),
        "Should find method token in enum"
    );
}

// ============================================================================
// Position Verification Tests (Text Extraction)
// ============================================================================

/// Helper to extract text at delta-encoded position
fn extract_text_at_token(
    source: &str,
    tokens: &[SemanticToken],
    token_index: usize,
) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();

    // Decode delta positions to absolute
    let mut abs_line = 0u32;
    let mut abs_col = 0u32;

    for (i, token) in tokens.iter().enumerate() {
        if token.delta_line > 0 {
            abs_line += token.delta_line;
            abs_col = token.delta_start;
        } else {
            abs_col += token.delta_start;
        }

        if i == token_index {
            let line_text = lines.get(abs_line as usize)?;
            let start = abs_col as usize;
            let end = start + token.length as usize;

            if end <= line_text.len() {
                return Some(line_text[start..end].to_string());
            } else {
                return None;
            }
        }
    }
    None
}

/// Convert token type to readable name
const fn token_type_name(token_type: u32) -> &'static str {
    match token_type {
        token_types::CLASS => "CLASS",
        token_types::INTERFACE => "INTERFACE",
        token_types::ENUM => "ENUM",
        token_types::TYPE_PARAMETER => "TYPE_PARAMETER",
        token_types::METHOD => "METHOD",
        token_types::PROPERTY => "PROPERTY",
        token_types::VARIABLE => "VARIABLE",
        token_types::PARAMETER => "PARAMETER",
        token_types::KEYWORD => "KEYWORD",
        token_types::MODIFIER => "MODIFIER",
        token_types::COMMENT => "COMMENT",
        token_types::STRING => "STRING",
        token_types::NUMBER => "NUMBER",
        _ => "OTHER",
    }
}

#[test]
fn test_extracted_text_matches_expected_simple_class() {
    let source = "class MyClass {}";
    let tokens = get_semantic_tokens(source);

    assert!(!tokens.is_empty(), "Should have tokens");

    let class_token_idx = tokens
        .iter()
        .position(|t| t.token_type == token_types::CLASS)
        .expect("Should have class token");

    let extracted =
        extract_text_at_token(source, &tokens, class_token_idx).expect("Should extract text");

    assert_eq!(
        extracted, "MyClass",
        "Extracted text should be 'MyClass', got '{}'",
        extracted
    );
}

#[test]
fn test_extracted_text_matches_expected_method() {
    let source = r"class Test {
    void myMethod() {}
}";
    let tokens = get_semantic_tokens(source);

    let method_token_idx = tokens
        .iter()
        .position(|t| t.token_type == token_types::METHOD)
        .expect("Should have method token");

    let extracted =
        extract_text_at_token(source, &tokens, method_token_idx).expect("Should extract text");

    assert_eq!(
        extracted, "myMethod",
        "Extracted text should be 'myMethod', got '{}'",
        extracted
    );
}

#[test]
fn test_extracted_text_all_tokens_valid() {
    let source = r"class Test {
    private int count;
    public void process(String input) {
        int temp = 0;
    }
}";
    let tokens = get_semantic_tokens(source);
    let lines: Vec<&str> = source.lines().collect();

    println!("Source lines:");
    for (i, line) in lines.iter().enumerate() {
        println!("  {}: '{}'", i, line);
    }

    println!("\nVerifying all tokens extract valid text:");
    let mut abs_line = 0u32;
    let mut abs_col = 0u32;

    for (i, token) in tokens.iter().enumerate() {
        if token.delta_line > 0 {
            abs_line += token.delta_line;
            abs_col = token.delta_start;
        } else {
            abs_col += token.delta_start;
        }

        let line_text = lines.get(abs_line as usize);
        let extracted = line_text.and_then(|lt| {
            let start = abs_col as usize;
            let end = start + token.length as usize;
            if end <= lt.len() {
                Some(&lt[start..end])
            } else {
                None
            }
        });

        println!(
            "  #{}: type={}, pos=({},{}), len={}, extracted={:?}",
            i,
            token_type_name(token.token_type),
            abs_line,
            abs_col,
            token.length,
            extracted
        );

        assert!(
            extracted.is_some(),
            "Token {} at ({},{}) with len {} should be extractable from line {:?}",
            i,
            abs_line,
            abs_col,
            token.length,
            line_text
        );
    }
}

// ============================================================================
// Cumulative Drift Detection Tests
// ============================================================================

#[test]
fn test_no_cumulative_drift_multiple_identical_methods() {
    // Critical test: identical methods should all extract correctly
    let source = r"class Test {
    void startup() {}
    void startup() {}
    void startup() {}
    void startup() {}
    void startup() {}
    void startup() {}
    void startup() {}
    void startup() {}
    void startup() {}
    void startup() {}
}";
    let tokens = get_semantic_tokens(source);
    let lines: Vec<&str> = source.lines().collect();

    let method_count = tokens
        .iter()
        .filter(|t| t.token_type == token_types::METHOD)
        .count();

    assert_eq!(method_count, 10, "Should have 10 method tokens");

    println!("Testing drift across 10 identical methods:");

    let mut abs_line = 0u32;
    let mut abs_col = 0u32;
    let mut method_num = 0;

    for token in &tokens {
        if token.delta_line > 0 {
            abs_line += token.delta_line;
            abs_col = token.delta_start;
        } else {
            abs_col += token.delta_start;
        }

        if token.token_type == token_types::METHOD {
            method_num += 1;
            let line_text = lines.get(abs_line as usize).expect("Line should exist");
            let start = abs_col as usize;
            let end = start + token.length as usize;

            let extracted = if end <= line_text.len() {
                &line_text[start..end]
            } else {
                "OUT_OF_BOUNDS"
            };

            println!(
                "  method #{}: line={}, col={}, extracted='{}'",
                method_num, abs_line, abs_col, extracted
            );

            assert_eq!(
                extracted, "startup",
                "Method {} at line {} should extract 'startup', got '{}' (col={}, line='{}')",
                method_num, abs_line, extracted, abs_col, line_text
            );
        }
    }
}

#[test]
fn test_no_drift_with_parameters() {
    // Test drift when methods have parameters (multiple tokens per line)
    let source = r"class Test {
    void startup(String[] args) {}
    void startup(String[] args) {}
    void startup(String[] args) {}
    void startup(String[] args) {}
    void startup(String[] args) {}
}";
    let tokens = get_semantic_tokens(source);
    let lines: Vec<&str> = source.lines().collect();

    println!("Testing drift with parameters (multiple tokens per line):");

    let mut abs_line = 0u32;
    let mut abs_col = 0u32;

    for (i, token) in tokens.iter().enumerate() {
        if token.delta_line > 0 {
            abs_line += token.delta_line;
            abs_col = token.delta_start;
        } else {
            abs_col += token.delta_start;
        }

        let line_text = lines.get(abs_line as usize).unwrap_or(&"");
        let start = abs_col as usize;
        let end = start + token.length as usize;

        let extracted = if end <= line_text.len() {
            &line_text[start..end]
        } else {
            "OUT_OF_BOUNDS"
        };

        // A parameter's declared type is a type reference in its own right, so
        // `String` in `String[] args` is a legitimate CLASS token alongside the
        // enclosing `Test`. What this test pins is that delta encoding does not
        // drift, i.e. every emitted span still lands on the identifier it names.
        let expected: &[&str] = match token.token_type {
            token_types::CLASS => &["Test", "String"],
            token_types::METHOD => &["startup"],
            token_types::PARAMETER => &["args"],
            _ => continue,
        };

        println!(
            "  #{}: type={}, pos=({},{}), expected one of {:?}, got='{}'",
            i,
            token_type_name(token.token_type),
            abs_line,
            abs_col,
            expected,
            extracted
        );

        assert!(
            expected.contains(&extracted),
            "Token {} ({}) should be one of {:?}, got '{}'",
            i,
            token_type_name(token.token_type),
            expected,
            extracted
        );
    }
}

#[test]
fn test_no_drift_realistic_pattern() {
    // Repeated static methods must not accumulate token-position drift.
    let source = r"class CatalogShelfLauncher {
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
}";
    let tokens = get_semantic_tokens(source);
    let lines: Vec<&str> = source.lines().collect();

    println!("Testing realistic pattern (private static void):");

    let mut abs_line = 0u32;
    let mut abs_col = 0u32;
    let mut method_count = 0;

    for token in &tokens {
        if token.delta_line > 0 {
            abs_line += token.delta_line;
            abs_col = token.delta_start;
        } else {
            abs_col += token.delta_start;
        }

        if token.token_type == token_types::METHOD {
            method_count += 1;
            let line_text = lines.get(abs_line as usize).expect("Line should exist");
            let start = abs_col as usize;
            let end = start + token.length as usize;

            let extracted = if end <= line_text.len() {
                &line_text[start..end]
            } else {
                panic!(
                    "Method {} OUT OF BOUNDS: col={}, len={}, line_len={}, line='{}'",
                    method_count,
                    abs_col,
                    token.length,
                    line_text.len(),
                    line_text
                );
            };

            println!(
                "  method #{}: line={}, col={}, extracted='{}'",
                method_count, abs_line, abs_col, extracted
            );

            assert_eq!(
                extracted, "startup",
                "Method {} should be 'startup', got '{}' (drift detected!)",
                method_count, extracted
            );
        }
    }

    assert_eq!(method_count, 6, "Should have found 6 methods");
}

// ============================================================================
// UTF-16 Position Tests
// ============================================================================

#[test]
fn test_ascii_positions_correct() {
    // ASCII should work correctly even without UTF-16 handling
    let source = "class Test { void method() {} }";
    let tokens = get_semantic_tokens(source);

    // Find the method token
    let method_idx = tokens
        .iter()
        .position(|t| t.token_type == token_types::METHOD)
        .expect("Should have method token");

    let extracted = extract_text_at_token(source, &tokens, method_idx).expect("Should extract");

    assert_eq!(extracted, "method");
}

#[test]
fn test_position_at_end_of_line() {
    // Test tokens near end of line
    let source = "class A {}";
    let tokens = get_semantic_tokens(source);

    assert_ne!(tokens, []);

    let extracted = extract_text_at_token(source, &tokens, 0).expect("Should extract");
    assert_eq!(extracted, "A");
}

#[test]
fn test_position_with_varying_indentation() {
    // Test that tokens work with different indentation levels
    let source = r"class Test {
  void a() {}
    void b() {}
      void c() {}
        void d() {}
}";
    let tokens = get_semantic_tokens(source);
    let lines: Vec<&str> = source.lines().collect();

    let method_names = ["a", "b", "c", "d"];
    let mut method_idx = 0;

    let mut abs_line = 0u32;
    let mut abs_col = 0u32;

    for token in &tokens {
        if token.delta_line > 0 {
            abs_line += token.delta_line;
            abs_col = token.delta_start;
        } else {
            abs_col += token.delta_start;
        }

        if token.token_type == token_types::METHOD {
            let line_text = lines[abs_line as usize];
            let start = abs_col as usize;
            let end = start + token.length as usize;
            let extracted = &line_text[start..end];

            assert_eq!(
                extracted, method_names[method_idx],
                "Method {} should be '{}', got '{}'",
                method_idx, method_names[method_idx], extracted
            );
            method_idx += 1;
        }
    }

    assert_eq!(method_idx, 4, "Should find all 4 methods");
}

// Replicating the function from semantic_tokens.rs to test it directly
fn compute_line_col(text: &str, offset: usize) -> (u32, u32) {
    let mut line = 0;
    let mut col = 0;
    let mut current_offset = 0;

    for ch in text.chars() {
        if current_offset >= offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }

        current_offset += ch.len_utf8();
    }

    (line, col)
}

#[test]
fn test_offset_to_line_col_identical_lines() {
    // Test that offset_to_line_col returns the same column for identical content on different lines
    let source = "class A {\n    void m() {}\n    void m() {}\n    void m() {}\n}";

    let lines: Vec<&str> = source.lines().collect();
    println!("Source breakdown:");
    let mut offset = 0;
    for (i, line) in lines.iter().enumerate() {
        println!(
            "  Line {}: offset={}, len={}, content='{}'",
            i,
            offset,
            line.len(),
            line
        );
        offset += line.len() + 1; // +1 for \n
    }

    // Method 'm' appears at column 9 on lines 1, 2, 3
    // "    void m" - m is at col 9 (0-indexed)
    let method_col = 9;

    println!("\nMethod positions:");
    let mut line_start = 10; // Line 1 starts at byte 10 (after "class A {\n")
    for line_num in 1..=3 {
        let method_offset = line_start + method_col;
        println!(
            "  Line {}: method at byte offset {}",
            line_num, method_offset
        );

        // Compute line/col using the same algorithm as semantic_tokens.rs
        let (line, col) = compute_line_col(source, method_offset);
        println!(
            "    compute_line_col({}) = ({}, {})",
            method_offset, line, col
        );

        // Extract char at offset to verify
        let char_at_offset: String = source.chars().skip(method_offset).take(1).collect();
        println!("    char at offset: '{}'", char_at_offset);

        assert_eq!(
            col, method_col as u32,
            "Line {} method should be at column {}, got column {}",
            line_num, method_col, col
        );

        line_start += 15 + 1; // Each line is 15 chars + \n
    }
}

#[test]
fn test_byte_offset_verification() {
    // Manually verify that byte offsets are what we expect
    let source = r"class CatalogShelfLauncher {
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
}";

    println!("\n=== BYTE OFFSET VERIFICATION ===");
    println!("Source bytes:");

    // Calculate expected positions
    let lines: Vec<&str> = source.lines().collect();
    let mut line_start = 0;

    for (i, line) in lines.iter().enumerate() {
        println!(
            "  Line {}: starts at byte {}, len={}, content='{}'",
            i,
            line_start,
            line.len(),
            line
        );

        // Find 'startup' in this line
        if let Some(pos) = line.find("startup") {
            let expected_byte_offset = line_start + pos;
            println!(
                "    'startup' at column {} -> expected byte offset {}",
                pos, expected_byte_offset
            );
        }

        line_start += line.len() + 1; // +1 for newline
    }

    // Now verify compute_line_col gives correct results for expected offsets
    println!("\nVerifying compute_line_col:");

    // Line 1: startup should be at byte 29 + 24 = 53
    let expected_offsets = [
        (53, 1, 24),  // Line 1 startup
        (120, 2, 24), // Line 2 startup - this is what it SHOULD be
    ];

    for (offset, expected_line, expected_col) in expected_offsets {
        let (line, col) = compute_line_col(source, offset);
        let char_at: String = source.chars().skip(offset).take(7).collect();
        println!(
            "  byte_offset={}: computed=({},{}), expected=({},{}), text='{}'",
            offset, line, col, expected_line, expected_col, char_at
        );
        assert_eq!(
            (line, col),
            (expected_line, expected_col),
            "Position mismatch at byte offset {}",
            offset
        );
    }
}

/// Test tokens BEFORE delta encoding to see raw absolute positions
#[test]
fn test_raw_absolute_positions() {
    use hyperion_semantics::db::ParseDatabase;

    // Use same source as test_byte_offset_verification - 2 methods
    let source = r"class CatalogShelfLauncher {
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
}";

    println!("\n=== PARSER BYTE OFFSET TEST ===");
    println!("Source len: {} bytes", source.len());
    println!("Expected startup positions: byte 53 (line 1), byte 120 (line 2)");

    // Show raw bytes
    println!("\nRaw bytes around expected positions:");
    println!("  bytes 50-60: {:?}", &source.as_bytes()[50..60]);
    println!("  bytes 115-125: {:?}", &source.as_bytes()[115..125]);

    // Show chars
    let chars: Vec<char> = source.chars().collect();
    println!("\nChars at key positions:");
    for offset in [53, 120, 121] {
        let byte_val = source.as_bytes().get(offset);
        let context: String = source
            .chars()
            .skip(offset.saturating_sub(3))
            .take(10)
            .collect();
        println!(
            "  byte {}: {:?} (byte={:?}), context: '{}'",
            offset,
            chars.get(offset),
            byte_val,
            context
        );
    }

    let db = Arc::new(Mutex::new(Database::default()));
    let file_id = {
        let mut db_lock = db.lock();
        let file_id = FileId::new(0);
        db_lock.register_file(
            file_id,
            Arc::new(source.to_string()),
            hyperion_workspace::Language::Java,
        );
        file_id
    };

    // Parse to get syntax tree
    let root = {
        let db_lock = db.lock();
        let snapshot = db_lock.snapshot();
        let parse_tree = snapshot.parse_file(file_id);
        parse_tree.syntax()
    };

    // Find all METHOD_DECL nodes and their 'startup' identifiers
    use hyperion_parser::syntax::SyntaxKind;

    println!("\n=== RAW SYNTAX POSITIONS ===");
    println!("Source:\n{}", source);
    println!("\nFinding METHOD_DECL nodes:");

    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            // Find the method name (identifier followed by '(')
            let tokens: Vec<_> = node
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
                .collect();

            for (i, token) in tokens.iter().enumerate() {
                if token.kind() == SyntaxKind::IDENT {
                    if let Some(next) = tokens.get(i + 1) {
                        if next.kind() == SyntaxKind::L_PAREN {
                            let range = token.text_range();
                            let byte_start = usize::from(range.start());
                            let (line, col) = compute_line_col(source, byte_start);

                            println!(
                                "  METHOD '{}' at byte_offset={}, computed_pos=({}, {})",
                                token.text(),
                                byte_start,
                                line,
                                col
                            );

                            // Verify extracted text matches
                            let lines: Vec<&str> = source.lines().collect();
                            if let Some(line_text) = lines.get(line as usize) {
                                let start = col as usize;
                                let end = start + token.text().len();
                                if end <= line_text.len() {
                                    let extracted = &line_text[start..end];
                                    println!(
                                        "    extracted at ({},{}): '{}'",
                                        line, col, extracted
                                    );
                                    assert_eq!(
                                        extracted,
                                        token.text(),
                                        "Position mismatch! byte_offset={}",
                                        byte_start
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn test_debug_all_tokens_realistic_pattern() {
    // Debug test to see ALL tokens and their positions
    let source = r"class CatalogShelfLauncher {
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
    private static void startup(String[] args) throws Exception {}
}";
    let tokens = get_semantic_tokens(source);
    let lines: Vec<&str> = source.lines().collect();

    println!("\n=== DEBUG: All tokens for realistic pattern ===");
    println!("Source:");
    for (i, line) in lines.iter().enumerate() {
        println!("  Line {}: '{}'", i, line);
    }

    println!("\nTokens (raw delta format from LSP):");
    for (i, token) in tokens.iter().enumerate() {
        println!(
            "  #{}: delta_line={}, delta_start={}, len={}, type={}",
            i,
            token.delta_line,
            token.delta_start,
            token.length,
            token_type_name(token.token_type)
        );
    }

    println!("\nDecoded absolute positions:");
    let mut abs_line = 0u32;
    let mut abs_col = 0u32;

    for (i, token) in tokens.iter().enumerate() {
        // Decode delta to absolute
        if token.delta_line > 0 {
            abs_line += token.delta_line;
            abs_col = token.delta_start;
        } else {
            abs_col += token.delta_start;
        }

        let line_text = lines.get(abs_line as usize).unwrap_or(&"<NO LINE>");
        let start = abs_col as usize;
        let end = start + token.length as usize;
        let extracted = if end <= line_text.len() {
            &line_text[start..end]
        } else {
            "<OUT_OF_BOUNDS>"
        };

        println!(
            "  #{}: type={:<10} abs=({:2},{:2}) len={} extracted='{}'",
            i,
            token_type_name(token.token_type),
            abs_line,
            abs_col,
            token.length,
            extracted
        );
    }
}

#[test]
fn class_token_uses_utf16_columns_after_unicode_and_all_line_endings() {
    for newline in ["\n", "\r\n", "\r"] {
        let source = format!("// header{newline}/* 😀界 */ class Café {{}}");
        let tokens = get_semantic_tokens(&source);
        let mut line = 0;
        let mut character = 0;
        let mut classes = Vec::new();
        for token in tokens {
            if token.delta_line == 0 {
                character += token.delta_start;
            } else {
                line += token.delta_line;
                character = token.delta_start;
            }
            if token.token_type == token_types::CLASS {
                classes.push((line, character, token.length));
            }
        }
        assert_eq!(classes, vec![(1, 16, 4)], "newline: {newline:?}");
    }
}
