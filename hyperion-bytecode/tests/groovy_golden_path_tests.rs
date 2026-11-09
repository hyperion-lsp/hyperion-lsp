// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Golden Path Integration Tests
//!
//! These tests validate that core Groovy features work end-to-end:
//! Groovy source → AST → Bytecode → Execution → Correct result
//!
//! Focus areas:
//! - GString interpolation (Phase 4.1)
//! - Gradle DSL with closures (Phase 3)
//! - Safe navigation (Phase 4.6)
//! - List/Map literals (Phase 4.7)
//! - Range literals (Phase 4.3)

use cafebabe::attributes::AttributeData;
use cafebabe::constant_pool::ConstantPoolItem as CafeConstantPoolItem;
use cafebabe::parse_class;
use hyperion_bytecode::gradle::GradleBuildConfig;
use hyperion_bytecode::gradle_executor::GradleExecutor;
use hyperion_bytecode::groovy_producer::GroovyBytecodeProducer;
use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use hyperion_bytecode::producer::{BytecodeError, BytecodeProducer, CompilationContext};

/// Helper: Compile and execute Groovy source, return result
fn compile_and_execute(source: &str) -> Value {
    compile_and_execute_with_vm(source).1
}

fn compile_and_display(source: &str) -> String {
    let (vm, value) = compile_and_execute_with_vm(source);
    vm.display_value(&value)
}

fn compile_and_execute_with_vm(source: &str) -> (VirtualMachine, Value) {
    let producer = GroovyBytecodeProducer::new();

    // Parse
    producer.parse(source, "Test.groovy").expect("Parse failed");

    // Compile to bytecode (all classes including closures)
    let context = CompilationContext {
        source: Some(source.to_string()),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("Test.groovy".into()),
        metadata: Default::default(),
    };

    let all_classes = producer
        .produce_all_classes(&context)
        .expect("Bytecode generation failed");

    // Execute
    let mut vm = VirtualMachine::new(MemoryConfig::default());

    // Inspect all classes (main class + closure classes) before defining the
    // complete set as one order-independent batch.
    for (class_name, bytecode) in &all_classes {
        if std::env::var("HYPERION_DEBUG_GROOVY_GOLDEN").is_ok() {
            eprintln!(
                "DEBUG: Loading class {} ({} bytes)",
                class_name,
                bytecode.len()
            );
            // Check for invokedynamic opcodes
            let invokedynamic_count = bytecode.iter().filter(|&&b| b == 0xBA).count();
            if invokedynamic_count > 0 {
                eprintln!(
                    "DEBUG: {} contains {} invokedynamic instructions",
                    class_name, invokedynamic_count
                );
            }
        }
    }
    let definitions: Vec<_> = all_classes
        .iter()
        .map(|(name, bytecode)| (name.clone(), bytecode.clone()))
        .collect();
    let report = vm.load_classes_from_bytes(&definitions);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert!(report.is_loaded("Test"));

    if std::env::var("HYPERION_DEBUG_GROOVY_GOLDEN").is_ok() {
        if let Some(bytes) = all_classes.get("Test") {
            if let Ok(class_file) = parse_class(bytes) {
                if let Some(method) = class_file
                    .methods
                    .iter()
                    .find(|m| m.name == "run" && m.descriptor.to_string() == "()Ljava/lang/Object;")
                {
                    if let Some(AttributeData::Code(code)) = method
                        .attributes
                        .iter()
                        .find_map(|a| matches!(a.data, AttributeData::Code(_)).then_some(&a.data))
                    {
                        eprintln!(
                            "DEBUG: Test.run max_locals={} max_stack={} code_len={}",
                            code.max_locals,
                            code.max_stack,
                            code.code.len()
                        );
                        eprintln!(
                            "DEBUG: Test.run first_bytes={:02X?}",
                            code.code.iter().copied().take(32).collect::<Vec<_>>()
                        );
                    }
                }
            }
        }
    }

    // The main class is "Test" (derived from "Test.groovy")
    let result = vm
        .invoke_static("Test", "run", "()Ljava/lang/Object;", &[])
        .expect("Failed to execute run()");
    (vm, result)
}

#[test]
fn control_expressions_use_groovy_truth_and_object_safe_merges() {
    let cases = [
        ("return false ? 1 : 'fallback'", "fallback"),
        ("return true ? 1 : 'fallback'", "1"),
        ("return 0 ?: 42", "42"),
        ("return '' ?: 'fallback'", "fallback"),
        ("return [] ?: 'empty'", "empty"),
        ("return [1] ?: 'empty'", "[1]"),
        ("return !0", "true"),
        ("return 0 || 'value'", "true"),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn statement_conditions_use_groovy_truth() {
    let cases = [
        (
            "def result = 2\nif (\"\") { result = 1 } else { result = 3 }\nreturn result",
            "3",
        ),
        (
            "def result = 2\nif (\"value\") { result = 1 }\nreturn result",
            "1",
        ),
        ("def result = 2\nif (0L) { result = 1 }\nreturn result", "2"),
        (
            "def result = 2\nif ([1]) { result = 1 }\nreturn result",
            "1",
        ),
        (
            "def result = 2\nwhile ([]) { result = 1 }\nreturn result",
            "2",
        ),
        (
            "def condition = \"value\"\ndef result = 2\nwhile (condition) { result = 7; condition = \"\" }\nreturn result",
            "7",
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn error_wrapped_loop_control_keeps_its_semantics() {
    let cases = [
        (
            "def result = 2\nwhile (true) { result = 7; break }\nreturn result",
            "7",
        ),
        (
            "def condition = true\ndef result = 2\nwhile (condition) { condition = false; continue; result = 9 }\nreturn result",
            "2",
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn parsed_for_loops_execute_their_init_body_and_update() {
    let cases = [
        (
            "def result = 0\nfor (def i = 0; i < 3; i = i + 1) { result = result + i }\nreturn result",
            "3",
        ),
        (
            "def result = 0\nfor (item in [1, 2, 3]) { result = item }\nreturn result",
            "3",
        ),
        (
            "def item = 99\ndef seen = 0\nfor (item in [1, 2]) { seen = item }\nreturn item",
            "99",
        ),
        (
            "def i = 99\ndef seen = 0\nfor (def i = 0; i < 2; i = i + 1) { seen = i }\nreturn i",
            "99",
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn lexical_statement_bodies_restore_shadowed_bindings() {
    let cases = [
        (
            "def value = 1\nif (true) { def value = 2 }\nreturn value",
            "1",
        ),
        (
            "def value = 1\ndef condition = true\nwhile (condition) { def value = 2; condition = false }\nreturn value",
            "1",
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn primitive_for_in_iterables_fail_closed() {
    let context = CompilationContext {
        source: Some("for (item in 1) { return item }".to_string()),
        source_path: Some("ForIn.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("primitive for-in receiver must fail closed");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("requires an Iterable reference")
    ));
}

#[test]
fn multiple_variable_declarations_fail_closed() {
    let context = CompilationContext {
        source: Some("def first = 1, second = 2\nreturn first + second".to_string()),
        source_path: Some("Declarations.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("multi-variable declaration must fail closed");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("multiple variables")
    ));
}

#[test]
fn non_throwable_values_fail_closed_at_athrow() {
    for source in ["throw 1", "throw \"boom\"", "throw [1]"] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Throw.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("non-Throwable value must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature }
                    if feature.contains("requires a Throwable reference")
            ),
            "source: {source}; error: {error:?}"
        );
    }

    let context = CompilationContext {
        source: Some("throw new RuntimeException()".to_string()),
        source_path: Some("Throw.groovy".into()),
        ..CompilationContext::default()
    };
    GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect("recognized Throwable construction should compile");
}

#[test]
fn finally_preserves_wide_return_values() {
    assert_eq!(
        compile_and_display(
            "try { return 2147483648 } finally { def neighboringFinallyLocal = 7 }"
        ),
        "2147483648"
    );
}

#[test]
fn assignment_expressions_preserve_their_values() {
    for (source, expected) in [
        ("def value = 1\nreturn value = 2", "2"),
        ("def value = 1\nreturn value = 2147483648", "2147483648"),
        ("def value = 1\ndef copy = (value = 3)\nreturn copy", "3"),
    ] {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn error_bearing_aggregates_fail_instead_of_changing_contents_or_arity() {
    for source in [
        "def foo = null\nreturn [1, foo?.bar]",
        "def foo = null\nreturn consume(foo?.bar)",
        "def foo = null\nreturn [key: foo?.bar]",
    ] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Aggregate.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("error-bearing aggregate must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature }
                    if feature.contains("tree-sitter could not preserve")
            ),
            "source: {source}; error: {error:?}"
        );
    }

    assert_eq!(compile_and_display("return [:]"), "{}");
}

#[test]
fn unrecoverable_parser_errors_fail_closed() {
    let context = CompilationContext {
        source: Some("if ('') { return 1 }".to_string()),
        source_path: Some("ParserError.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("unrecoverable parser error must fail closed");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("tree-sitter could not parse")
    ));
}

#[test]
fn local_slots_preserve_wide_values_and_neighboring_variables() {
    let cases = [
        (
            "def wide = 2147483648; def neighbor = 7; return wide",
            "2147483648",
        ),
        (
            "def value = 1; def neighbor = 7; value = 2147483648; return neighbor",
            "7",
        ),
        ("def marker = 99; def range = 1..3; return marker", "99"),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn typed_locals_adapt_initializers_and_initialize_empty_declarations() {
    let cases = [
        ("long value = 1; return value", "1"),
        ("double value = 2L; return value", "2.0"),
        ("boolean flag = true; return flag", "true"),
        ("Object boxed = 7; return boxed", "7"),
        ("long value = 1; value = 2; return value", "2"),
        ("boolean flag = false; flag = true; return flag", "true"),
        (
            "Object boxed = 7; boxed = 2147483648; return boxed",
            "2147483648",
        ),
        ("long value = 1; return value = 2", "2"),
        ("int zero; return zero", "0"),
        ("def empty; return empty", "null"),
        ("String text = null; return text", "null"),
        ("String text = (null); return text", "null"),
        ("String text = null; text = ((null)); return text", "null"),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn incompatible_typed_local_values_fail_closed() {
    for source in [
        "int value = 1L",
        "boolean flag = 1",
        "String text = 1",
        "int value = 1; value = 2L",
        "boolean flag = true; flag = 1",
        "String text = null; text = 1",
    ] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("TypedLocal.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("incompatible typed value must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature }
                    if feature.contains("typed local cannot adapt initializer")
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn null_return_paths_execute_with_valid_stack_metadata() {
    for source in ["// no executable statements", "return"] {
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
        let classes = producer
            .produce_all_classes(&context)
            .expect("Bytecode generation failed");
        let class = parse_class(classes.get("Test").expect("missing script class"))
            .expect("invalid script class");
        let code = class
            .methods
            .iter()
            .find(|method| method.name == "run")
            .and_then(|method| {
                method
                    .attributes
                    .iter()
                    .find_map(|attribute| match &attribute.data {
                        AttributeData::Code(code) => Some(code),
                        _ => None,
                    })
            })
            .expect("missing run Code attribute");

        assert_eq!(code.max_stack, 1, "ACONST_NULL requires one stack slot");
        assert!(matches!(
            compile_and_execute(source),
            Value::Reference(hyperion_bytecode::interpreter::ObjectRef::Null)
        ));
    }
}

#[test]
fn unsupported_closure_bodies_fail_instead_of_erasing_behavior() {
    let cases = [
        (
            "def f = {\n    def x = 1\n    return x\n}\nreturn f.call()",
            "unsupported statement in generated closure",
        ),
        (
            "def f = {\n    if (true) { return 1 }\n    return 2\n}\nreturn f.call()",
            "unsupported statement in generated closure",
        ),
        (
            "def f = {\n    -1\n}\nreturn f.call()",
            "unsupported expression in generated closure",
        ),
        (
            "def f = {\n    'a' + 'b'\n}\nreturn f.call()",
            "requires integer operands",
        ),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Test.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported closure lowering must fail closed");
        assert!(matches!(
            error,
            BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
        ));
    }
}

#[test]
fn class_declarations_fail_instead_of_disappearing() {
    let context = CompilationContext {
        source: Some("class Widget { def value() { 42 } }".to_string()),
        source_path: Some("Widget.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("unsupported class lowering must fail closed");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("Groovy class declarations")
    ));
}

#[test]
fn casts_and_instanceof_fail_instead_of_erasing_their_operator() {
    let cases = [
        (
            "def value = 'text'; return (String) value",
            "cast expressions",
        ),
        (
            "def value = 'text'; return value instanceof String",
            "instanceof expressions",
        ),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Types.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported type operator must fail closed");
        assert!(matches!(
            error,
            BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
        ));
    }
}

#[test]
fn unsupported_primary_expressions_fail_instead_of_reducing_to_a_child() {
    let cases = [
        ("def counter = 1; return counter++", "increment/decrement"),
        ("return new int[3]", "array creation"),
        ("return String.class", "class literal"),
        ("return this", "explicit Groovy `this`"),
        ("return String::valueOf", "method reference"),
        ("return 0x1.0p1", "hexadecimal floating-point"),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Primary.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported primary expression must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn unsupported_statements_fail_instead_of_reducing_to_a_child() {
    let cases = [
        ("assert false", "assert statements"),
        ("do { value = 1 } while (false)", "do-while statements"),
        ("label: return 1", "labeled statements"),
        ("synchronized(this) { return 1 }", "synchronized statements"),
        (
            "try (resource = open()) { return 1 }",
            "try-with-resources statements",
        ),
        ("yield 1", "yield statements"),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Statements.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported statement must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn unsupported_declarations_fail_instead_of_becoming_identifiers() {
    let cases = [
        ("package example\nreturn 1", "package declarations"),
        ("import java.util.List\nreturn 1", "import declarations"),
        ("interface Widget {}", "interface declarations"),
        ("enum Mode { ON }", "enum declarations"),
        ("@interface Marker {}", "annotation type declarations"),
        ("record Pair(int value) {}", "record declarations"),
        ("module example {}", "module declarations"),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Declarations.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported declaration must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn unsupported_assignment_targets_fail_instead_of_disappearing() {
    let cases = [
        ("def obj = [:]; obj.value = 1", "property assignment"),
        ("def values = [1]; values[0] = 2", "index assignment"),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Assignment.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported assignment target must fail closed");
        assert!(matches!(
            error,
            BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
        ));
    }
}

#[test]
fn unsupported_operators_fail_instead_of_erasing_semantics() {
    let cases = [
        ("return 1 in [1]", "binary operator `in`"),
        ("return ~1", "unary operator `~`"),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Operator.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported operator must fail closed");
        assert!(matches!(
            error,
            BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
        ));
    }
}

#[test]
fn arithmetic_promotes_both_operand_directions() {
    let cases = [
        ("return 2 + 3000000000L", "3000000002"),
        ("return 3000000000L + 2", "3000000002"),
        ("return 2 + 0.5d", "2.5"),
        ("return 2L + 0.5f", "2.5"),
        ("return 2.0f + 0.5d", "2.5"),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn floating_point_suffixes_preserve_runtime_width() {
    assert_eq!(
        compile_and_display("float value = 1.5f; return value"),
        "1.5"
    );
    assert_eq!(
        compile_and_display("return Math.round(2147483648.0f)"),
        "2147483647",
        "float suffix must select Math.round(F)I instead of Math.round(D)J"
    );
    assert_eq!(
        compile_and_display("return Math.round(2147483648.0d)"),
        "2147483648"
    );
}

#[test]
fn non_decimal_literals_preserve_twos_complement_width() {
    for (source, expected) in [
        ("return 0xFFFFFFFF", "-1"),
        ("return 0b11111111111111111111111111111111", "-1"),
        ("return 037777777777", "-1"),
        ("return 0xFFFFFFFFL", "4294967295"),
        ("return 0xFFFFFFFFFFFFFFFFL", "-1"),
        ("def value = 42i; return value", "42"),
    ] {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn declared_reference_types_use_valid_erased_jvm_descriptors() {
    assert_eq!(
        compile_and_display("java.lang.String text = 'ok'; return text"),
        "ok"
    );
    assert_eq!(
        compile_and_display("String[] values; def read = { -> values }; return read.call()"),
        "null"
    );
    assert_eq!(
        compile_and_display(
            "java.util.List<String> values; def read = { -> values }; return read.call()"
        ),
        "null"
    );
}

#[test]
fn fully_qualified_catch_types_use_internal_jvm_names() {
    assert_eq!(
        compile_and_display(
            "try { return 1 } catch (java.lang.RuntimeException error) { return 2 }"
        ),
        "1"
    );

    let context = CompilationContext {
        source: Some("try { return 1 } catch (java.lang.String value) { return 2 }".to_string()),
        source_path: Some("CatchType.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("non-Throwable catch must fail closed");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("catch requires a Throwable class")
    ));
}

#[test]
fn fully_qualified_constructor_names_use_internal_jvm_names() {
    let rendered = compile_and_display("return new java.lang.RuntimeException()");
    assert!(
        rendered.contains("RuntimeException"),
        "unexpected object rendering: {rendered}"
    );
}

#[test]
fn explicit_throw_uses_the_throwing_pc_for_handler_lookup() {
    assert_eq!(
        compile_and_display(
            "try { throw new java.lang.RuntimeException() } catch (java.lang.RuntimeException error) { return 7 }"
        ),
        "7"
    );
}

#[test]
fn unsupported_integer_domains_and_widths_fail_closed() {
    for (source, expected) in [
        ("return 2147483648i", "forced 32-bit"),
        ("return 0x1FFFFFFFF", "add an L suffix"),
        ("return 1g", "BigInteger literals"),
    ] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("IntegerLiteral.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported integer literal must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn unsupported_or_unrepresentable_floating_literals_fail_closed() {
    for (source, expected) in [
        ("return 1.5g", "BigDecimal literals"),
        ("return 1e999d", "outside the finite"),
        ("return 1e-999f", "outside the finite"),
    ] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("FloatLiteral.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unrepresentable floating literal must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn comparisons_promote_both_operands_before_opcode_selection() {
    let cases = [
        ("return 2 < 3000000000L", "true"),
        ("return 3000000000L > 2", "true"),
        ("return 2 < 2.5d", "true"),
        ("return 2L < 2.5f", "true"),
        ("return 2.0f == 2.0d", "true"),
        ("return true == true", "true"),
        ("return true != false", "true"),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn comparisons_without_faithful_lowering_fail_closed() {
    let cases = [
        (
            "return 'same' == 'same'",
            "reference value comparison is not yet supported",
        ),
        (
            "return true < false",
            "comparison requires numeric operands or two booleans for equality",
        ),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Comparison.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported comparison must fail closed");
        assert!(matches!(
            error,
            BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
        ));
    }
}

#[test]
fn bitwise_operators_respect_integral_widths_and_shift_counts() {
    let cases = [
        ("return 5 & 3L", "1"),
        ("return 5L | 2", "7"),
        ("return 1L << 33", "8589934592"),
        ("return 1 << 33L", "2"),
    ];

    for (source, expected) in cases {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

#[test]
fn nonintegral_bitwise_operands_fail_closed() {
    let cases = [
        (
            "return 1.0d & 1",
            "bitwise operators require integral operands",
        ),
        (
            "return true | false",
            "bitwise operators require integral operands",
        ),
        ("return 1 << 2.0d", "shifts require integral operands"),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Bitwise.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("nonintegral bitwise operation must fail closed");
        assert!(matches!(
            error,
            BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
        ));
    }
}

#[test]
fn ranges_without_int_range_lowering_fail_closed() {
    for source in ["return 1L..3L", "return 1.0d..3.0d", "return 'a'..'c'"] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Range.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unsupported range endpoint must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature }
                    if feature.contains("IntRange lowering requires int endpoints")
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn nonnumeric_arithmetic_fails_before_opcode_selection() {
    let cases = [
        (
            "return true - false",
            "arithmetic requires numeric operands",
        ),
        (
            "return -missing",
            "unary negation requires a numeric operand",
        ),
        (
            "return -(1 == 1)",
            "unary negation requires a numeric operand",
        ),
        (
            "return true / false",
            "arithmetic requires numeric operands",
        ),
        ("return 1.0 % true", "arithmetic requires numeric operands"),
    ];

    for (source, expected) in cases {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Arithmetic.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("nonnumeric arithmetic must fail closed");
        assert!(matches!(
            error,
            BytecodeError::UnsupportedFeature { feature } if feature.contains(expected)
        ));
    }
}

#[test]
fn closure_object_boundaries_preserve_receiver_and_return_values() {
    let identity = r#"
        def f = { value -> value }
        return f.call("round-trip")
    "#;
    assert_eq!(compile_and_display(identity), "round-trip");

    let string_receiver = r#"
        def f = { value -> value.toUpperCase() }
        return f.call("trustworthy")
    "#;
    assert_eq!(compile_and_display(string_receiver), "TRUSTWORTHY");

    let arithmetic_return = r#"
        def f = { value -> value * 2 }
        return f.call(21)
    "#;
    let (vm, result) = compile_and_execute_with_vm(arithmetic_return);
    assert_eq!(vm.display_value(&result), "42");
    let Value::Reference(hyperion_bytecode::interpreter::ObjectRef::Object(id)) = result else {
        panic!("Object-returning closure leaked a primitive result");
    };
    assert_eq!(
        vm.object_class_name(id).as_deref(),
        Some("java/lang/Integer")
    );
}

#[test]
fn groovy_invokedynamic_descriptors_include_their_receiver() {
    let source = r#"
        def render = { value -> value.toUpperCase() }
        return render.call("seven")
    "#;
    let context = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some("Test.groovy".into()),
        ..CompilationContext::default()
    };
    let classes = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect("receiver-aware Groovy bytecode");

    let mut dynamic_sites = 0;
    for (class_name, bytes) in &classes {
        let class = parse_class(bytes).expect("valid generated class");
        for entry in class.constantpool_iter() {
            if let CafeConstantPoolItem::InvokeDynamic(site) = entry {
                dynamic_sites += 1;
                assert!(
                    site.name_and_type
                        .descriptor
                        .starts_with("(Ljava/lang/Object;"),
                    "{class_name}.{} omits its receiver: {}",
                    site.name_and_type.name,
                    site.name_and_type.descriptor
                );
            }
        }
    }
    assert!(
        dynamic_sites >= 2,
        "expected script and closure dynamic sites"
    );

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let definitions: Vec<_> = classes
        .iter()
        .map(|(name, bytecode)| (name.clone(), bytecode.clone()))
        .collect();
    let report = vm.load_classes_from_bytes(&definitions);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert!(report.is_loaded("Test"));
    let value = vm
        .invoke_static("Test", "run", "()Ljava/lang/Object;", &[])
        .expect("execute receiver-aware dynamic calls");
    assert_eq!(vm.display_value(&value), "SEVEN");
    assert_eq!(
        compile_and_display("def value = 7; return value.intValue()"),
        "7"
    );
}

#[test]
fn nested_closures_capture_outer_parameters_and_transitive_locals() {
    let outer_parameter = r#"
        def outer = { value -> { -> value } }
        def inner = outer.call(7)
        return inner.call()
    "#;
    assert_eq!(compile_and_display(outer_parameter), "7");

    let transitive_local = r#"
        def captured = "stable"
        def outer = { -> { -> captured.toUpperCase() } }
        def inner = outer.call()
        return inner.call()
    "#;
    assert_eq!(compile_and_display(transitive_local), "STABLE");
}

#[test]
fn method_calls_preserve_closure_argument_position() {
    let closure_first = r#"
        def combine = { producer, addend -> producer.call() + addend }
        return combine.call({ -> 40 }, 2)
    "#;
    assert_eq!(compile_and_display(closure_first), "42");

    let two_closures = r#"
        def combine = { left, right -> left.call() + right.call() }
        return combine.call({ -> 20 }, { -> 22 })
    "#;
    assert_eq!(compile_and_display(two_closures), "42");
}

#[test]
fn static_math_calls_use_value_compatible_descriptors() {
    assert_eq!(compile_and_display("return Math.max(3L, 8L)"), "8");
    assert_eq!(compile_and_display("return Math.max(3.5d, 8.5d)"), "8.5");
    assert_eq!(compile_and_display("return Math.max(3, 8L)"), "8");
    assert_eq!(compile_and_display("return Math.max(3L, 8.5d)"), "8.5");
    assert_eq!(compile_and_display("return Math.sqrt(81)"), "9.0");
    assert_eq!(compile_and_display("return Math.pow(2, 10)"), "1024.0");
}

#[test]
fn unknown_static_calls_fail_instead_of_guessing_a_descriptor() {
    let context = CompilationContext {
        source: Some("return CustomFactory.create(\"value\")".to_string()),
        source_path: Some("Test.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("unknown static signatures must fail closed");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("unsupported static Groovy call: CustomFactory.create")
    ));
}

#[test]
fn unresolved_constructor_calls_fail_instead_of_guessing_a_descriptor() {
    for (source, constructor) in [
        ("return new ArrayList(16)", "new ArrayList(...)"),
        (
            "return new StringBuilder(\"value\")",
            "new StringBuilder(...)",
        ),
    ] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Test.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("unresolved constructor signatures must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature }
                    if feature.contains("constructor resolution is unavailable for Groovy call")
                        && feature.contains(constructor)
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn spread_navigation_fails_instead_of_becoming_a_null_return() {
    for source in ["return [1, 2]*.toString()", "return [1, 2]*.class"] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Test.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("spread navigation must remain visible and fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature }
                    if feature.contains("Groovy spread navigation (*.member)")
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn oversized_control_flow_fails_instead_of_wrapping_branch_offsets() {
    let body = "0;\n".repeat(20_000);
    let source = format!("if (false) {{\n{body}}}\nreturn 1");
    let context = CompilationContext {
        source: Some(source),
        source_path: Some("Test.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("out-of-range short branches must fail before serialization");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("exceeds the signed 16-bit JVM branch range")
    ));
}

/// Helper: Execute Gradle script and return configuration
fn execute_gradle_script(source: &str) -> GradleBuildConfig {
    let mut executor = GradleExecutor::new();

    executor
        .execute_source(source)
        .expect("Gradle script execution failed")
}

// ============================================================================
// GOLDEN PATH TEST 1: GString Interpolation
// ============================================================================

#[test]
fn golden_test_gstring_interpolation() {
    let source = r#"
        def name = "World"
        def count = 42
        return "Hello ${name}, count is ${count}!"
    "#;

    assert_eq!(compile_and_display(source), "Hello World, count is 42!");

    assert_eq!(compile_and_display("return \"value=${1 + 2}\""), "value=3");

    assert_eq!(
        compile_and_display(
            "def marker = 7; def wide = 2147483648; def ratio = 1.5; return \"${marker}:${wide}:${ratio}\""
        ),
        "7:2147483648:1.5"
    );
}

#[test]
fn unsupported_gstring_interpolation_fails_closed() {
    let context = CompilationContext {
        source: Some("def counter = 1\nreturn \"${counter++}\"".to_string()),
        source_path: Some("GString.groovy".into()),
        ..CompilationContext::default()
    };
    let error = GroovyBytecodeProducer::new()
        .produce_all_classes(&context)
        .expect_err("unsupported interpolation must fail closed");
    assert!(matches!(
        error,
        BytecodeError::UnsupportedFeature { feature }
            if feature.contains("increment/decrement")
    ));
}

#[test]
fn power_validates_operands_and_executes_numeric_cases() {
    for (source, expected) in [
        ("return 2 ** 3", "8"),
        ("return 2L ** 3", "8"),
        ("return 2 ** -1", "0.5"),
    ] {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }

    for source in [
        "return true ** 2",
        "return \"value\" ** 2",
        "return 2 ** false",
        "return null ** 2",
    ] {
        let context = CompilationContext {
            source: Some(source.to_string()),
            source_path: Some("Power.groovy".into()),
            ..CompilationContext::default()
        };
        let error = GroovyBytecodeProducer::new()
            .produce_all_classes(&context)
            .expect_err("nonnumeric power operand must fail closed");
        assert!(
            matches!(
                &error,
                BytecodeError::UnsupportedFeature { feature }
                    if feature.contains("power requires primitive numeric operands")
            ),
            "source: {source}; error: {error:?}"
        );
    }
}

#[test]
fn string_concatenation_preserves_assignment_values() {
    for (source, expected) in [
        ("return 1 + \"x\"", "1x"),
        ("return \"x\" + 2147483648", "x2147483648"),
        ("return \"x\" + true", "xtrue"),
        ("def value = 1\nreturn (value = 2) + \"x\"", "2x"),
        ("def value = 1\nreturn \"x\" + (value = 2)", "x2"),
    ] {
        assert_eq!(compile_and_display(source), expected, "source: {source}");
    }
}

// ============================================================================
// GOLDEN PATH TEST 2: Gradle Dependencies DSL
// ============================================================================

#[test]
fn golden_test_gradle_dependencies_dsl() {
    let build_gradle = r#"
        dependencies {
            implementation 'com.google.guava:guava:32.1.2'
            testImplementation 'junit:junit:4.13.2'
        }
    "#;

    let config = execute_gradle_script(build_gradle);

    // Verify dependencies were captured
    assert_eq!(
        config.dependency_list.len(),
        2,
        "Should have 2 dependencies"
    );

    // Verify first dependency
    let guava = &config.dependency_list[0];
    assert_eq!(guava.group, "com.google.guava");
    assert_eq!(guava.artifact, "guava");
    assert_eq!(guava.version, "32.1.2");
    assert_eq!(guava.configuration, "implementation");

    // Verify second dependency
    let junit = &config.dependency_list[1];
    assert_eq!(junit.group, "junit");
    assert_eq!(junit.artifact, "junit");
    assert_eq!(junit.version, "4.13.2");
    assert_eq!(junit.configuration, "testImplementation");

    println!("✅ Gradle dependencies DSL works!");
}

// ============================================================================
// GOLDEN PATH TEST 3: Safe Navigation Operator
// ============================================================================

#[test]
fn golden_test_safe_navigation_with_null() {
    let source = r#"
        def user = null
        return user?.name?.toUpperCase()
    "#;

    let result = compile_and_execute(source);

    match result {
        Value::Reference(hyperion_bytecode::interpreter::ObjectRef::Null) => {
            println!("✅ Safe navigation returned null correctly");
        }
        other => panic!("Expected null, got {:?}", other),
    }
}

#[test]
fn golden_test_safe_navigation_with_value() {
    let source = r#"
        def user = [profile: [name: "Alice"]]
        return user?.profile?.name
    "#;

    assert_eq!(compile_and_display(source), "Alice");
}

// ============================================================================
// GOLDEN PATH TEST 4: List Literals
// ============================================================================

#[test]
fn golden_test_list_literal_creation() {
    let source = r#"
        def numbers = [1, 2, 3, 4, 5]
        return numbers
    "#;

    let (vm, result) = compile_and_execute_with_vm(source);
    assert_eq!(vm.display_value(&result), "[1, 2, 3, 4, 5]");
    let Value::Reference(hyperion_bytecode::interpreter::ObjectRef::Object(id)) = result else {
        panic!("expected heap-backed ArrayList")
    };
    assert_eq!(
        vm.object_class_name(id).as_deref(),
        Some("java/util/ArrayList")
    );

    let (vm, element) = compile_and_execute_with_vm("return [1, 2, 3].get(1)");
    assert_eq!(vm.display_value(&element), "2");
    let Value::Reference(hyperion_bytecode::interpreter::ObjectRef::Object(id)) = element else {
        panic!("List.add(Object) accepted an unboxed primitive")
    };
    assert_eq!(
        vm.object_class_name(id).as_deref(),
        Some("java/lang/Integer")
    );
    assert_eq!(compile_and_display("return [1, 2, 3][1]"), "2");
}

// ============================================================================
// GOLDEN PATH TEST 5: Map Literals
// ============================================================================

#[test]
fn golden_test_map_literal_creation() {
    let source = r#"
        def config = [name: "test", version: "1.0", enabled: true]
        return config
    "#;

    let (vm, result) = compile_and_execute_with_vm(source);
    assert_eq!(
        vm.display_value(&result),
        "{name=test, version=1.0, enabled=true}"
    );
    let Value::Reference(hyperion_bytecode::interpreter::ObjectRef::Object(id)) = result else {
        panic!("expected heap-backed LinkedHashMap")
    };
    assert_eq!(
        vm.object_class_name(id).as_deref(),
        Some("java/util/LinkedHashMap")
    );

    let (vm, value) = compile_and_execute_with_vm("return [answer: 42].get(\"answer\")");
    assert_eq!(vm.display_value(&value), "42");
    let Value::Reference(hyperion_bytecode::interpreter::ObjectRef::Object(id)) = value else {
        panic!("Map.put(Object,Object) accepted an unboxed primitive")
    };
    assert_eq!(
        vm.object_class_name(id).as_deref(),
        Some("java/lang/Integer")
    );
    assert_eq!(compile_and_display("return [answer: 42][\"answer\"]"), "42");
}

// ============================================================================
// GOLDEN PATH TEST 6: Range Literals
// ============================================================================

#[test]
fn golden_test_range_literal_creation() {
    let source = r#"
        def range = 1..10
        return range
    "#;

    let result = compile_and_execute(source);

    match result {
        Value::Reference(obj_ref) => {
            // Result should be IntRange
            println!("✅ Range literal compiled and executed");
            println!("✅ Result: {:?}", obj_ref);
            // TODO: Verify it's an IntRange with correct bounds
        }
        other => panic!("Expected IntRange reference, got {:?}", other),
    }
}

#[test]
fn golden_test_range_exclusive() {
    let source = r#"
        def range = 1..<10
        return range
    "#;

    let result = compile_and_execute(source);

    match result {
        Value::Reference(_) => {
            println!("✅ Exclusive range literal compiled and executed");
        }
        other => panic!("Expected IntRange reference, got {:?}", other),
    }
}

// ============================================================================
// GOLDEN PATH TEST 7: The Big Kahuna - Everything Together
// ============================================================================

#[test]
fn golden_test_gradle_full_dsl_with_interpolation() {
    let build_gradle = r#"
        plugins {
            id 'java'
        }

        def guavaVersion = '32.1.2'

        repositories {
            mavenCentral()
            mavenLocal()
        }

        dependencies {
            implementation "com.google.guava:guava:${guavaVersion}"
            testImplementation 'junit:junit:4.13.2'
        }
    "#;

    let config = execute_gradle_script(build_gradle);

    // Verify plugins
    assert_eq!(config.plugin_definitions.len(), 1);
    assert_eq!(config.plugin_definitions[0].id, "java");

    // Verify repositories
    assert_eq!(config.repository_list.len(), 2);
    assert_eq!(config.repository_list[0].name, "mavenCentral");
    assert_eq!(config.repository_list[1].name, "mavenLocal");

    // Verify dependencies
    assert_eq!(config.dependency_list.len(), 2);

    // Check GString interpolation worked
    let guava = &config.dependency_list[0];
    assert_eq!(guava.group, "com.google.guava");
    assert_eq!(guava.artifact, "guava");
    assert_eq!(guava.version, "32.1.2", "GString interpolation should work");

    println!("🎉 GOLDEN TEST: Full Gradle DSL with interpolation works!");
}

// ============================================================================
// GOLDEN PATH TEST 8: Closure Variable Capture
// ============================================================================

#[test]
fn golden_test_closure_variable_capture() {
    // Test that closures capture variables from outer scope
    let source = r#"
        def multiplier = 3
        def closure = { x -> x * multiplier }
        return closure.call(5)
    "#;

    assert_eq!(compile_and_display(source), "15");
}

// ============================================================================
// GOLDEN PATH TEST 9: Closure Implicit Return (Last Expression)
// ============================================================================

#[test]
fn golden_test_closure_implicit_return_literal() {
    // { -> 42 }.call() should return 42, not the closure object
    let source = r#"
        def closure = { -> 42 }
        return closure.call()
    "#;

    assert_eq!(compile_and_display(source), "42");
}

#[test]
fn golden_test_closure_implicit_return_string() {
    // { -> "hello" }.call() should return "hello"
    let source = r#"
        def closure = { -> "hello" }
        return closure.call()
    "#;

    assert_eq!(compile_and_display(source), "hello");
}

#[test]
fn golden_test_closure_implicit_return_with_param() {
    // { x -> x * 2 }.call(5) should return 10
    let source = r#"
        def closure = { x -> x * 2 }
        return closure.call(5)
    "#;

    assert_eq!(compile_and_display(source), "10");
}

#[test]
fn golden_test_closure_explicit_return_still_works() {
    // Explicit return should still work unchanged
    let source = r#"
        def closure = { x -> return x + 1 }
        return closure.call(9)
    "#;

    assert_eq!(compile_and_display(source), "10");
}
