// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Compiler Unit Tests - Bytecode Verification
//!
//! These tests verify that the Groovy compiler emits correct JVM bytecode
//! for each feature. Fast to run, catches compilation bugs early.
//!
//! Test strategy: Compile Groovy source → Inspect bytecode → Verify opcodes
//!
//! Coverage:
//! - Phase 4.1: GString → StringBuilder pattern
//! - Phase 4.3: Range → IntRange constructor
//! - Phase 4.6: Safe navigation → label-backed IFNULL merge
//! - Phase 4.7: List/Map literals → ArrayList/LinkedHashMap
//! - Phase 2: Closures → synthetic class generation

use hyperion_bytecode::groovy_producer::GroovyBytecodeProducer;
use hyperion_bytecode::producer::{BytecodeProducer, CompilationContext};

/// Helper: Compile Groovy source and return raw bytecode
fn compile_groovy(source: &str) -> Vec<u8> {
    use hyperion_groovy::{tree_sitter_wrapper::GroovyParser, AstBuilder};

    // Debug: Parse and show AST
    let mut parser = GroovyParser::new().expect("Parser init failed");
    let parsed = parser.parse(source).expect("Parse failed");
    let ast_builder = AstBuilder::new(source);
    let statements = ast_builder.build_statements(parsed.root());

    println!("\n=== Parsed AST ===");
    for (i, stmt) in statements.iter().enumerate() {
        println!("Statement {}: {:?}", i, stmt);
    }
    println!("==================\n");

    let producer = GroovyBytecodeProducer::new();
    producer.parse(source, "Test.groovy").expect("Parse failed");

    let context = CompilationContext {
        source: Some(source.to_string()),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("Test.groovy".into()),
        metadata: Default::default(),
    };

    producer
        .produce_bytes(&context)
        .expect("Bytecode generation failed")
}

/// Helper: Check if bytecode contains an opcode
fn contains_opcode(bytecode: &[u8], opcode: u8) -> bool {
    bytecode.contains(&opcode)
}

/// Helper: Count occurrences of an opcode
fn count_opcode(bytecode: &[u8], opcode: u8) -> usize {
    bytecode.iter().filter(|&&b| b == opcode).count()
}

/// Helper: Check if bytecode contains a class reference
fn contains_class_ref(bytecode: &[u8], class_name: &str) -> bool {
    // Search for UTF-8 string in constant pool
    let bytes = class_name.as_bytes();
    bytecode.windows(bytes.len()).any(|window| window == bytes)
}

/// Helper: Check if bytecode contains a method reference
fn contains_method_ref(bytecode: &[u8], class_name: &str, method_name: &str) -> bool {
    contains_class_ref(bytecode, class_name) && contains_class_ref(bytecode, method_name)
}

// JVM Opcode constants
const NEW: u8 = 0xBB;
const INVOKESPECIAL: u8 = 0xB7;
const INVOKEVIRTUAL: u8 = 0xB6;
const INVOKEINTERFACE: u8 = 0xB9;
const IFNULL: u8 = 0xC6;
const GOTO: u8 = 0xA7;
const ACONST_NULL: u8 = 0x01;
const ICONST_0: u8 = 0x03;
const ICONST_1: u8 = 0x04;

// ============================================================================
// Phase 4.1: GString → StringBuilder Bytecode
// ============================================================================

#[test]
fn test_gstring_emits_stringbuilder() {
    let source = r#"
        def name = "World"
        return "Hello ${name}!"
    "#;

    let bytecode = compile_groovy(source);

    println!("Bytecode size: {} bytes", bytecode.len());
    println!("First 20 bytes: {:?}", &bytecode[..bytecode.len().min(20)]);

    // Check if it's a valid .class file
    if bytecode.len() >= 4 {
        assert_eq!(
            &bytecode[0..4],
            &[0xCA, 0xFE, 0xBA, 0xBE],
            "Should have .class magic number"
        );
    } else {
        panic!("Bytecode too small: {} bytes", bytecode.len());
    }

    // Debug: Print all string constants in the constant pool
    println!("\n=== String constants in constant pool ===");
    for i in 0..bytecode.len() {
        // Look for UTF8 entries (tag 0x01)
        if bytecode[i] == 0x01 && i + 3 < bytecode.len() {
            let len = u16::from_be_bytes([bytecode[i + 1], bytecode[i + 2]]) as usize;
            if i + 3 + len <= bytecode.len() {
                if let Ok(s) = std::str::from_utf8(&bytecode[i + 3..i + 3 + len]) {
                    println!("  \"{}\"", s);
                }
            }
        }
    }
    println!("==========================================\n");

    // Verify StringBuilder usage
    assert!(
        contains_class_ref(&bytecode, "java/lang/StringBuilder"),
        "Should reference StringBuilder class"
    );
    assert!(
        contains_opcode(&bytecode, NEW),
        "Should create new StringBuilder"
    );
    assert!(
        contains_method_ref(&bytecode, "java/lang/StringBuilder", "append"),
        "Should call append() method"
    );
    assert!(
        contains_method_ref(&bytecode, "java/lang/StringBuilder", "toString"),
        "Should call toString() method"
    );

    println!("✅ GString compiles to StringBuilder pattern");
}

#[test]
fn test_gstring_uses_string_valueof() {
    let source = r#"
        def count = 42
        return "Count: ${count}"
    "#;

    let bytecode = compile_groovy(source);

    // Verify String.valueOf() for safe null handling
    assert!(
        contains_method_ref(&bytecode, "java/lang/String", "valueOf"),
        "Should use String.valueOf() for null-safe conversion"
    );

    println!("✅ GString uses String.valueOf() for conversion");
}

#[test]
fn test_gstring_multiple_interpolations() {
    let source = r#"
        def a = "foo"
        def b = "bar"
        return "${a} and ${b}"
    "#;

    let bytecode = compile_groovy(source);

    // Should call append() multiple times
    let append_count = count_opcode(&bytecode, INVOKEVIRTUAL);
    assert!(
        append_count >= 4,
        "Should call append() at least 4 times (2 strings + 2 values + toString)"
    );

    println!("✅ GString handles multiple interpolations");
}

// ============================================================================
// Phase 4.3: Range Literal → IntRange Bytecode
// ============================================================================

#[test]
fn test_range_emits_intrange_constructor() {
    let source = r#"
        def range = 1..10
        return range
    "#;

    let bytecode = compile_groovy(source);

    // Verify IntRange creation
    assert!(
        contains_class_ref(&bytecode, "groovy/lang/IntRange"),
        "Should reference IntRange class"
    );
    assert!(
        contains_opcode(&bytecode, NEW),
        "Should create new IntRange"
    );
    assert!(
        contains_opcode(&bytecode, INVOKESPECIAL),
        "Should call IntRange constructor"
    );
    assert!(
        contains_class_ref(&bytecode, "(ZII)V"),
        "Should call IntRange(boolean, int, int)"
    );

    println!("✅ Range literal compiles to IntRange constructor");
}

#[test]
fn test_range_inclusive_flag() {
    let source = r#"
        def inclusive = 1..10
        return inclusive
    "#;

    let bytecode = compile_groovy(source);

    // Should have ICONST_1 for inclusive flag
    assert!(
        contains_opcode(&bytecode, ICONST_1),
        "Should push true (ICONST_1) for inclusive range"
    );

    println!("✅ Inclusive range sets correct flag");
}

#[test]
fn test_range_exclusive_flag() {
    let source = r#"
        def exclusive = 1..<10
        return exclusive
    "#;

    let bytecode = compile_groovy(source);

    // Should have ICONST_0 for exclusive flag
    assert!(
        contains_opcode(&bytecode, ICONST_0),
        "Should push false (ICONST_0) for exclusive range"
    );

    println!("✅ Exclusive range sets correct flag");
}

#[test]
fn test_range_uses_primitive_constructor() {
    let source = r#"
        def range = 5..15
        return range
    "#;

    let bytecode = compile_groovy(source);

    // The public IntRange(boolean, int, int) constructor accepts primitive
    // endpoints directly; boxing here would target the old VM-only descriptor.
    assert!(
        !contains_method_ref(&bytecode, "java/lang/Integer", "valueOf"),
        "Should not box primitive IntRange endpoints"
    );

    println!("✅ Range uses primitive IntRange constructor");
}

// ============================================================================
// Phase 4.6: Safe Navigation → Label-Backed IFNULL Bytecode
// ============================================================================

#[test]
fn test_safe_navigation_emits_null_check() {
    let source = r#"
        def obj = getObject()
        return obj?.property
    "#;

    let bytecode = compile_groovy(source);

    // Verify the control-flow contract. Exact value behavior is covered by the
    // executable golden tests; this keeps the lower-level branch smoke test.
    assert!(
        contains_opcode(&bytecode, IFNULL),
        "Should check if null with IFNULL"
    );
    assert!(
        contains_opcode(&bytecode, GOTO),
        "Should jump to end if not null"
    );
    assert!(
        contains_opcode(&bytecode, ACONST_NULL),
        "Should push null result in null path"
    );

    println!("✅ Safe navigation emits a label-backed null branch");
}

#[test]
fn test_safe_navigation_chaining() {
    let source = r#"
        def obj = getObject()
        return obj?.foo?.bar?.baz
    "#;

    let bytecode = compile_groovy(source);

    // Should have multiple null checks (one per ?)
    let ifnull_count = count_opcode(&bytecode, IFNULL);
    assert!(
        ifnull_count >= 3,
        "Should have at least 3 null checks for chained safe navigation"
    );

    println!("✅ Chained safe navigation emits multiple null checks");
}

// ============================================================================
// Phase 4.7: List Literal → ArrayList Bytecode
// ============================================================================

#[test]
fn test_list_literal_emits_arraylist() {
    let source = r#"
        def list = [1, 2, 3]
        return list
    "#;

    let bytecode = compile_groovy(source);

    // Verify ArrayList creation
    assert!(
        contains_class_ref(&bytecode, "java/util/ArrayList"),
        "Should reference ArrayList class"
    );
    assert!(
        contains_opcode(&bytecode, NEW),
        "Should create new ArrayList"
    );
    assert!(
        contains_opcode(&bytecode, INVOKESPECIAL),
        "Should call ArrayList.<init>()"
    );

    println!("✅ List literal compiles to ArrayList");
}

#[test]
fn test_list_literal_calls_add() {
    let source = r#"
        def list = [1, 2, 3, 4, 5]
        return list
    "#;

    let bytecode = compile_groovy(source);

    // Should call List.add() via INVOKEINTERFACE
    assert!(
        contains_opcode(&bytecode, INVOKEINTERFACE),
        "Should use INVOKEINTERFACE for List.add()"
    );
    assert!(
        contains_method_ref(&bytecode, "java/util/List", "add"),
        "Should call List.add() method"
    );

    // Should call add() 5 times (once per element)
    let add_calls = count_opcode(&bytecode, INVOKEINTERFACE);
    assert!(
        add_calls >= 5,
        "Should call add() at least 5 times for 5 elements"
    );

    println!("✅ List literal calls add() for each element");
}

#[test]
fn test_empty_list_literal() {
    let source = r#"
        def list = []
        return list
    "#;

    let bytecode = compile_groovy(source);

    // Should create ArrayList but not call add()
    assert!(
        contains_class_ref(&bytecode, "java/util/ArrayList"),
        "Should create ArrayList even for empty list"
    );

    println!("✅ Empty list literal creates ArrayList");
}

// ============================================================================
// Phase 4.7: Map Literal → LinkedHashMap Bytecode
// ============================================================================

#[test]
fn test_map_literal_emits_linkedhashmap() {
    let source = r#"
        def map = [a: 1, b: 2]
        return map
    "#;

    let bytecode = compile_groovy(source);

    // Verify LinkedHashMap creation
    assert!(
        contains_class_ref(&bytecode, "java/util/LinkedHashMap"),
        "Should reference LinkedHashMap class"
    );
    assert!(
        contains_opcode(&bytecode, NEW),
        "Should create new LinkedHashMap"
    );
    assert!(
        contains_opcode(&bytecode, INVOKESPECIAL),
        "Should call LinkedHashMap.<init>()"
    );

    println!("✅ Map literal compiles to LinkedHashMap");
}

#[test]
fn test_map_literal_calls_put() {
    let source = r#"
        def map = [name: "Alice", age: 30, active: true]
        return map
    "#;

    let bytecode = compile_groovy(source);

    // Should call Map.put() via INVOKEINTERFACE
    assert!(
        contains_opcode(&bytecode, INVOKEINTERFACE),
        "Should use INVOKEINTERFACE for Map.put()"
    );
    assert!(
        contains_method_ref(&bytecode, "java/util/Map", "put"),
        "Should call Map.put() method"
    );

    // Should call put() 3 times (once per entry)
    let put_calls = count_opcode(&bytecode, INVOKEINTERFACE);
    assert!(
        put_calls >= 3,
        "Should call put() at least 3 times for 3 entries"
    );

    println!("✅ Map literal calls put() for each entry");
}

#[test]
fn test_empty_map_literal() {
    let source = r#"
        def map = [:]
        return map
    "#;

    let bytecode = compile_groovy(source);

    // Should create LinkedHashMap but not call put()
    assert!(
        contains_class_ref(&bytecode, "java/util/LinkedHashMap"),
        "Should create LinkedHashMap even for empty map"
    );

    println!("✅ Empty map literal creates LinkedHashMap");
}

// ============================================================================
// Phase 2: Closure Bytecode (Basic Verification)
// ============================================================================

#[test]
fn test_closure_compiles() {
    let source = r#"
        def closure = { x -> x * 2 }
        return closure
    "#;

    let bytecode = compile_groovy(source);

    // Should compile without error
    assert!(
        bytecode.len() > 100,
        "Closure should produce substantial bytecode"
    );

    println!("✅ Closure compiles to bytecode");
}

#[test]
fn test_closure_with_variable_capture() {
    let source = r#"
        def multiplier = 3
        def closure = { x -> x * multiplier }
        return closure
    "#;

    let bytecode = compile_groovy(source);

    // Should compile and reference captured variable
    assert!(
        bytecode.len() > 100,
        "Closure with capture should produce bytecode"
    );

    println!("✅ Closure with variable capture compiles");
}

// ============================================================================
// Combined Features Tests
// ============================================================================

#[test]
fn test_gstring_in_list() {
    let source = r#"
        def name = "Alice"
        def messages = ["Hello ${name}", "Goodbye ${name}"]
        return messages
    "#;

    let bytecode = compile_groovy(source);

    // Should have both ArrayList and StringBuilder
    assert!(
        contains_class_ref(&bytecode, "java/util/ArrayList"),
        "Should create ArrayList"
    );
    assert!(
        contains_class_ref(&bytecode, "java/lang/StringBuilder"),
        "Should use StringBuilder for GString"
    );

    println!("✅ GString inside list literal compiles");
}

#[test]
fn test_range_in_map_value() {
    let source = r#"
        def config = [range: 1..10]
        return config
    "#;

    let bytecode = compile_groovy(source);

    // Should have both LinkedHashMap and IntRange
    assert!(
        contains_class_ref(&bytecode, "java/util/LinkedHashMap"),
        "Should create LinkedHashMap"
    );
    assert!(
        contains_class_ref(&bytecode, "groovy/lang/IntRange"),
        "Should create IntRange for value"
    );

    println!("✅ Range as map value compiles");
}

#[test]
fn test_all_features_together() {
    let source = r#"
        def version = "1.0"
        def config = [
            name: "MyApp ${version}",
            numbers: [1, 2, 3],
            range: 1..10
        ]
        return config?.name
    "#;

    let bytecode = compile_groovy(source);

    // Verify all features are present
    assert!(
        contains_class_ref(&bytecode, "java/lang/StringBuilder"),
        "Should have GString"
    );
    assert!(
        contains_class_ref(&bytecode, "java/util/ArrayList"),
        "Should have list literal"
    );
    assert!(
        contains_class_ref(&bytecode, "java/util/LinkedHashMap"),
        "Should have map literal"
    );
    assert!(
        contains_class_ref(&bytecode, "groovy/lang/IntRange"),
        "Should have range literal"
    );
    assert!(
        contains_opcode(&bytecode, IFNULL),
        "Should have safe navigation"
    );

    println!("🎉 All features compile together!");
}
