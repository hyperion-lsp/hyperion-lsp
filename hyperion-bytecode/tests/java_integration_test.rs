// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for Java compilation

use hyperion_bytecode::java_producer::JavaBytecodeProducer;
use hyperion_bytecode::producer::{BytecodeProducer, CompilationContext};

#[test]
fn test_compile_simple_class() {
    let source = r#"
        public class HelloWorld {
            public static void main(String[] args) {
                int x = 42;
            }
        }
    "#;

    let producer = JavaBytecodeProducer::new();
    let result = producer.parse(source, "HelloWorld.java");
    assert!(result.is_ok(), "Parse failed: {:?}", result);

    let context = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some("HelloWorld.java".into()),
        ..Default::default()
    };

    let bytecode = producer.produce_bytes(&context);
    assert!(
        bytecode.is_ok(),
        "Bytecode generation failed: {:?}",
        bytecode
    );

    let bytes = bytecode.unwrap();
    // Check magic number
    assert_eq!(
        &bytes[0..4],
        &[0xCA, 0xFE, 0xBA, 0xBE],
        "Invalid class file magic"
    );
}

#[test]
fn test_compile_with_binary_expr() {
    let source = r#"
        public class MathTest {
            public static void main(String[] args) {
                int result = 1 + 2;
            }
        }
    "#;

    let producer = JavaBytecodeProducer::new();
    assert!(producer.parse(source, "MathTest.java").is_ok());

    let context = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some("MathTest.java".into()),
        ..Default::default()
    };

    let bytecode = producer.produce_bytes(&context);
    assert!(bytecode.is_ok());

    let bytes = bytecode.unwrap();
    assert_eq!(&bytes[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
}

#[test]
fn test_compile_with_return_statement() {
    let source = r#"
        public class ReturnTest {
            public static void main(String[] args) {
                return;
            }
        }
    "#;

    let producer = JavaBytecodeProducer::new();
    assert!(producer.parse(source, "ReturnTest.java").is_ok());

    let context = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some("ReturnTest.java".into()),
        ..Default::default()
    };

    let bytecode = producer.produce_bytes(&context);
    assert!(bytecode.is_ok());
}

#[test]
fn test_extract_class_name() {
    let source = r#"
        public class TestClassName {
            public static void main(String[] args) {
            }
        }
    "#;

    let producer = JavaBytecodeProducer::new();
    assert!(producer.parse(source, "path/to/TestClassName.java").is_ok());

    // The class name should be extracted from the file path
    let context = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some("path/to/TestClassName.java".into()),
        ..Default::default()
    };

    let bytecode = producer.produce_bytes(&context);
    assert!(bytecode.is_ok());

    // Verify the bytecode contains valid magic number
    let bytes = bytecode.unwrap();
    assert_eq!(&bytes[0..4], &[0xCA, 0xFE, 0xBA, 0xBE]);
}

#[test]
fn test_multiple_statements() {
    let source = r#"
        public class MultiStatement {
            public static void main(String[] args) {
                int a = 1;
                int b = 2;
                int c = a + b;
            }
        }
    "#;

    let producer = JavaBytecodeProducer::new();
    assert!(producer.parse(source, "MultiStatement.java").is_ok());

    let context = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some("MultiStatement.java".into()),
        ..Default::default()
    };

    let bytecode = producer.produce_bytes(&context);
    assert!(bytecode.is_ok());
}

/// ACC_ABSTRACT on each class of a unit, by class name.
fn abstract_flags(source: &str) -> Vec<(String, bool)> {
    let producer = JavaBytecodeProducer::new();
    producer.parse(source, "T.java").expect("parse");
    let mut flags: Vec<(String, bool)> = producer
        .produce_all_classes()
        .expect("every class of the unit compiles")
        .into_iter()
        .map(|(name, bytes)| {
            let class = hyperion_bytecode::parser::parse_class_file(&bytes).expect("class file");
            (name, class.access_flags.bits() & 0x0400 != 0)
        })
        .collect();
    flags.sort();
    flags
}

/// javac marks a class abstract when it is declared abstract, or when it is an enum with an
/// abstract method that its constants implement.
#[test]
fn abstract_classes_and_enums_with_abstract_methods_have_acc_abstract() {
    let source = r#"
public class T {
    public static int run() { return new C().f() + E.X.f(); }
}
abstract class A { abstract int f(); }
abstract class B { int g() { return 0; } }
class C extends A { int f() { return 1; } }
enum E { X { int f() { return 2; } }; abstract int f(); }
enum F { Y { int g() { return 3; } }; int g() { return 0; } }
"#;
    let flags = abstract_flags(source);
    let abstract_of = |name: &str| {
        flags
            .iter()
            .find(|(class, _)| class == name)
            .unwrap_or_else(|| panic!("class {name} not produced: {flags:?}"))
            .1
    };
    assert!(abstract_of("A"), "{flags:?}");
    assert!(abstract_of("B"), "{flags:?}");
    assert!(!abstract_of("C"), "{flags:?}");
    assert!(abstract_of("E"), "{flags:?}");
    assert!(!abstract_of("F"), "{flags:?}");
    assert!(!abstract_of("T"), "{flags:?}");
}
