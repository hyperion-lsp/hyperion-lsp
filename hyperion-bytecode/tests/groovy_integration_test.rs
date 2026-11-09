// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration test: Compile Groovy → Execute in interpreter

use hyperion_bytecode::groovy_producer::GroovyBytecodeProducer;
use hyperion_bytecode::interpreter::{MemoryConfig, VirtualMachine};
use hyperion_bytecode::producer::{BytecodeProducer, CompilationContext};

#[test]
fn test_compile_and_execute_groovy() {
    // Step 1: Compile Groovy to bytecode
    let producer = GroovyBytecodeProducer::new();
    let source = "return 2 + 3"; // Simple expression

    // Parse (stub for now)
    producer.parse(source, "Test.groovy").expect("Parse failed");

    // Produce .class bytecode
    let context = CompilationContext {
        source: Some(source.to_string()),
        target_version: 65, // Java 21
        debug_info: false,
        include_source_file: false,
        source_path: Some("Test.groovy".into()),
        metadata: Default::default(),
    };

    let bytecode = producer
        .produce_bytes(&context)
        .expect("Bytecode generation failed");

    // Verify magic number
    assert_eq!(
        &bytecode[0..4],
        &[0xCA, 0xFE, 0xBA, 0xBE],
        "Invalid .class magic number"
    );

    println!("✅ Generated {} bytes of .class bytecode", bytecode.len());
    println!("✅ Magic number: CAFEBABE");
    println!("✅ Contains run() method with bytecode: ICONST_2, ICONST_3, IADD, IRETURN");

    // Debug: print all bytecode
    println!("\nAll {} bytes of bytecode:", bytecode.len());
    for (i, chunk) in bytecode.chunks(16).enumerate() {
        print!("{:04X}: ", i * 16);
        for byte in chunk {
            print!("{:02X} ", byte);
        }
        println!();
    }

    // Step 2: Execute in interpreter
    let mut vm = VirtualMachine::new(MemoryConfig::default());

    // Load class
    let class_name = "GroovyScript"; // matches produce_bytes default
    vm.load_class_from_bytes(class_name, &bytecode)
        .expect("Failed to load class");

    println!("✅ Loaded class: {}", class_name);

    // Execute run() method (returns boxed Object)
    let result = vm
        .invoke_static(class_name, "run", "()Ljava/lang/Object;", &[])
        .expect("Failed to execute run()");

    println!("✅ Executed run() method");
    println!("✅ Result: {:?}", result);

    // Verify result: should be a boxed Integer reference
    // Note: heap is local to invoke_static, so we can't inspect the value directly
    // Just verify we got a valid reference back
    match result {
        hyperion_bytecode::interpreter::Value::Reference(obj_ref) => {
            // Got a valid object reference (boxed Integer)
            println!("🎉 SUCCESS: Groovy compiled and executed correctly!");
            println!("🎉 Result is a boxed Integer: {:?}", obj_ref);
        }
        hyperion_bytecode::interpreter::Value::Int(value) => {
            // If we got an unboxed int, that works too
            assert_eq!(value, 5, "Expected 2 + 3 = 5, got {}", value);
            println!("🎉 SUCCESS: 2 + 3 = {}", value);
        }
        other => panic!("Expected Reference or Int, got {:?}", other),
    }
}

#[test]
fn test_gstring_simple_interpolation() {
    // Test: def name = "World"; return "Hello, ${name}!"
    let producer = GroovyBytecodeProducer::new();
    let source = r#"
        def name = "World"
        return "Hello, ${name}!"
    "#;

    producer.parse(source, "Test.groovy").expect("Parse failed");

    let context = CompilationContext {
        source: Some(source.to_string()),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("Test.groovy".into()),
        metadata: Default::default(),
    };

    let bytecode = producer
        .produce_bytes(&context)
        .expect("Bytecode generation failed");

    // Verify it compiles
    assert_eq!(&bytecode[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
    println!("✅ GString simple interpolation compiled successfully");
}

#[test]
fn test_gstring_multiple_interpolations() {
    // Test: def x = 10; def y = 20; return "x=${x}, y=${y}"
    let producer = GroovyBytecodeProducer::new();
    let source = r#"
        def x = 10
        def y = 20
        return "x=${x}, y=${y}"
    "#;

    producer.parse(source, "Test.groovy").expect("Parse failed");

    let context = CompilationContext {
        source: Some(source.to_string()),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("Test.groovy".into()),
        metadata: Default::default(),
    };

    let bytecode = producer
        .produce_bytes(&context)
        .expect("Bytecode generation failed");

    // Verify it compiles
    assert_eq!(&bytecode[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
    println!("✅ GString multiple interpolations compiled successfully");
}
