// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end contracts for the deliberately small Kotlin evaluator producer.

use hyperion_bytecode::interpreter::{MemoryConfig, VirtualMachine};
use hyperion_bytecode::kotlin_producer::KotlinBytecodeProducer;
use hyperion_bytecode::producer::{BytecodeProducer, CompilationContext};

fn compile(source: &str, path: &str) -> Vec<u8> {
    let producer = KotlinBytecodeProducer::new();
    producer.parse(source, path).expect("source should parse");
    producer
        .produce_bytes(&CompilationContext {
            source: Some(source.to_string()),
            source_path: Some(path.into()),
            ..Default::default()
        })
        .expect("supported evaluator source should compile")
}

fn evaluate(class_name: &str, bytes: &[u8]) -> String {
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes(class_name, bytes)
        .unwrap_or_else(|error| panic!("producer emitted an invalid {class_name}: {error:?}"));
    let value = vm
        .invoke_static(class_name, "__eval__", "()Ljava/lang/Object;", &[])
        .expect("synthetic evaluator method should execute");
    vm.display_value(&value)
}

#[test]
fn emits_a_loadable_file_class_that_returns_a_literal() {
    let bytes = compile("fun __eval__(): Any? { return 100 }", "path/to/GetValue.kt");

    assert_eq!(&bytes[..4], &0xCAFEBABE_u32.to_be_bytes());
    assert_eq!(u16::from_be_bytes([bytes[6], bytes[7]]), 65);
    assert_eq!(evaluate("GetValueKt", &bytes), "100");
}

#[test]
fn emits_executable_arithmetic_and_reference_results() {
    for (source, expected) in [
        ("fun __eval__(): Any? { return 10 + 5 * 2 }", "20"),
        ("fun __eval__(): Any? { return \"hello\" }", "hello"),
        ("fun __eval__(): Any? { return true }", "true"),
        ("fun __eval__(): Any? { return null }", "null"),
    ] {
        let bytes = compile(source, "Evaluate.kt");
        assert_eq!(evaluate("EvaluateKt", &bytes), expected, "source: {source}");
    }
}

/// A snippet with no value is kotlin.Unit in Kotlin. It is refused, not returned as `null`.
#[test]
fn a_snippet_with_no_value_is_refused_not_returned_as_null() {
    let producer = KotlinBytecodeProducer::new();
    let source = "fun __eval__(): Any? { }";
    producer
        .parse(source, "Empty.kt")
        .expect("source should parse");
    let error = producer
        .produce_bytes(&CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Empty.kt".into()),
            ..Default::default()
        })
        .expect_err("a snippet with no value must not compile to a null result");
    assert!(format!("{error:?}").contains("kotlin.Unit"), "{error:?}");
}

#[test]
fn does_not_advertise_unimplemented_language_wide_features() {
    assert!(KotlinBytecodeProducer::new()
        .supported_features()
        .is_empty());
}

#[test]
fn rejects_general_kotlin_instead_of_emitting_a_no_op_shell() {
    let producer = KotlinBytecodeProducer::new();
    let error = producer
        .parse("data class User(val id: Int, val name: String)", "User.kt")
        .expect_err("the evaluator must not masquerade as a general Kotlin compiler");

    assert!(
        error.to_string().contains("general Kotlin compilation"),
        "unexpected error: {error}"
    );
}
