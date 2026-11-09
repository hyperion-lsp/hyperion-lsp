// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Executor for evaluating JVM code snippets

use crate::snippet::{
    detect_language_from_content, detect_snippet_type, wrap_snippet, SnippetType,
};
use hyperion_bytecode::groovy_producer::GroovyBytecodeProducer;
use hyperion_bytecode::interpreter::{MemoryConfig, ObjectRef, Value, VirtualMachine};
use hyperion_bytecode::producer::{BytecodeProducer, CompilationContext};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Language for evaluation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Java,
    Kotlin,
    Groovy,
}

impl FromStr for Language {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "java" => Ok(Self::Java),
            "kotlin" | "kt" => Ok(Self::Kotlin),
            "groovy" => Ok(Self::Groovy),
            _ => Err(format!("unknown language: {s}")),
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Java => write!(f, "Java"),
            Self::Kotlin => write!(f, "Kotlin"),
            Self::Groovy => write!(f, "Groovy"),
        }
    }
}

/// Errors that can occur during evaluation
#[derive(Debug, Error)]
pub enum EvalError {
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Compilation error: {0}")]
    CompilationError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Options for evaluation
#[derive(Debug, Clone)]
pub struct EvalOptions {
    /// Language to use (None = auto-detect)
    pub language: Option<Language>,

    /// Execution timeout in milliseconds
    pub timeout_ms: u64,

    /// Show verbose output
    pub verbose: bool,

    /// Print result type alongside value
    pub show_type: bool,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            language: None,
            timeout_ms: 5000,
            verbose: false,
            show_type: false,
        }
    }
}

/// Result of evaluation
#[derive(Debug)]
pub struct EvalResult {
    /// The computed value
    pub value: Value,

    /// String representation of the value
    pub display: String,

    /// Type of the result (if available)
    pub result_type: Option<String>,

    /// True if the invoked entry point returns `void` (no value to display).
    pub is_void: bool,
}

impl fmt::Display for EvalResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref ty) = self.result_type {
            let display = &self.display;
            write!(f, "{display} ({ty})")
        } else {
            let display = &self.display;
            write!(f, "{display}")
        }
    }
}

/// Evaluate code and return the result
pub fn eval(code: &str, options: &EvalOptions) -> Result<EvalResult, EvalError> {
    let lang = options
        .language
        .unwrap_or_else(|| detect_language_from_content(code));

    match lang {
        Language::Groovy => eval_groovy(code, options),
        Language::Java => eval_java(code, options),
        Language::Kotlin => eval_kotlin(code, options),
    }
}

/// Evaluate Groovy code
fn eval_groovy(code: &str, options: &EvalOptions) -> Result<EvalResult, EvalError> {
    let snippet_type = detect_snippet_type(code, Language::Groovy);
    let source = wrap_snippet(code, snippet_type, Language::Groovy);

    if options.verbose {
        eprintln!("Snippet type: {snippet_type:?}");
        eprintln!("Wrapped source:\n{source}");
    }

    let producer = GroovyBytecodeProducer::new();

    // Create compilation context
    let context = CompilationContext {
        source: Some(source),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("__Eval__.groovy".into()),
        metadata: HashMap::default(),
    };

    // Compile to bytecode
    let mut all_classes: Vec<_> = producer
        .produce_all_classes(&context)
        .map_err(|e| EvalError::CompilationError(format!("{e:?}")))?
        .into_iter()
        .collect();
    all_classes.sort_by(|left, right| left.0.cmp(&right.0));

    if options.verbose {
        let len = all_classes.len();
        eprintln!("Compiled {len} class(es)");
        for (name, bytecode) in &all_classes {
            let size = bytecode.len();
            eprintln!("  {name} ({size} bytes)");
        }
    }

    // Create VM and load all classes
    let config = MemoryConfig {
        max_instructions: options.timeout_ms * 10_000, // ~10k instructions per ms
        ..MemoryConfig::default()
    };
    let mut vm = VirtualMachine::new(config);

    let load_report = vm.load_classes_from_bytes(&all_classes);
    if !load_report.failures.is_empty() {
        let failures = load_report
            .failures
            .iter()
            .map(|failure| format!("{}: {}", failure.class_name, failure.error))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(EvalError::ExecutionError(format!(
            "Failed to load generated Groovy classes: {failures}"
        )));
    }

    // Execute - Groovy scripts have a run() method returning Object
    let result = vm
        .invoke_static("__Eval__", "run", "()Ljava/lang/Object;", &[])
        .map_err(|e| EvalError::ExecutionError(format!("{e:?}")))?;

    // Format result
    let (display, result_type) = format_value(&result, &vm, options.show_type);

    Ok(EvalResult {
        value: result,
        display,
        result_type,
        is_void: false,
    })
}

/// Evaluate Java code
fn eval_java(code: &str, options: &EvalOptions) -> Result<EvalResult, EvalError> {
    use hyperion_bytecode::java_producer::JavaBytecodeProducer;

    let snippet_type = detect_snippet_type(code, Language::Java);
    let source = wrap_snippet(code, snippet_type, Language::Java);

    if options.verbose {
        eprintln!("Snippet type: {snippet_type:?}");
        eprintln!("Wrapped source:\n{source}");
    }

    let producer = JavaBytecodeProducer::new();

    // Parse and compile
    producer
        .parse(&source, "__Eval__.java")
        .map_err(|e| EvalError::ParseError(format!("{e:?}")))?;

    let context = CompilationContext {
        source: Some(source),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("__Eval__.java".into()),
        metadata: HashMap::default(),
    };

    let bytecode = producer
        .produce_bytes(&context)
        .map_err(|e| EvalError::CompilationError(format!("{e:?}")))?;

    if options.verbose {
        let size = bytecode.len();
        eprintln!("Compiled __Eval__ ({size} bytes)");
    }

    // Create VM and load class
    let config = MemoryConfig {
        max_instructions: options.timeout_ms * 10_000,
        ..MemoryConfig::default()
    };
    let mut vm = VirtualMachine::new(config);

    // For a full class, run its real entry point (main, or first static method).
    // For wrapped expression/statement snippets, run the synthesized __eval__.
    let (class_name, method_name, descriptor) = match snippet_type {
        SnippetType::FullClass => producer.entry_point().ok_or_else(|| {
            EvalError::ExecutionError("no runnable entry point (static method) found".to_string())
        })?,
        SnippetType::Expression => (
            "__Eval__".to_string(),
            "__eval__".to_string(),
            "()Ljava/lang/Object;".to_string(),
        ),
        SnippetType::Statement => (
            "__Eval__".to_string(),
            "__eval__".to_string(),
            "()V".to_string(),
        ),
    };

    // Link every top-level class as one order-independent definition batch.
    // An unrelated sibling remains lazy, but the selected entry class must be
    // present and linked before execution begins.
    let all = producer
        .produce_all_classes()
        .map_err(|e| EvalError::CompilationError(format!("{e:?}")))?;
    let report = vm.load_classes_from_bytes(&all);
    let loaded_primary = report.is_loaded(&class_name);
    let primary_was_emitted = all.iter().any(|(name, _)| name == &class_name);
    if primary_was_emitted && !loaded_primary {
        let detail = report.failure_for(&class_name).map_or(
            "class was not published by the definition batch",
            |failure| failure.error.as_str(),
        );
        return Err(EvalError::ExecutionError(format!(
            "Failed to load class {class_name}: {detail}"
        )));
    }
    // Fall back to the primary bytecode if it was not among the unit classes.
    if !loaded_primary {
        vm.load_class_from_bytes(&class_name, &bytecode)
            .map_err(|e| EvalError::ExecutionError(format!("Failed to load class: {e:?}")))?;
    }

    let result = vm
        .invoke_static(&class_name, &method_name, &descriptor, &[])
        .map_err(|e| EvalError::ExecutionError(format!("{e:?}")))?;

    let (display, result_type) = format_value(&result, &vm, options.show_type);

    Ok(EvalResult {
        value: result,
        display,
        result_type,
        is_void: descriptor.ends_with(")V"),
    })
}

/// Compile Java source to the raw `.class` bytes the VM would load.
///
/// A debugging aid (see `--emit-class`) for inspecting emitted bytecode with
/// `javap -v` when a class fails to load (`ClassFormatError`). Does not execute.
pub fn java_class_bytes(code: &str) -> Result<Vec<u8>, EvalError> {
    use hyperion_bytecode::java_producer::JavaBytecodeProducer;

    let snippet_type = detect_snippet_type(code, Language::Java);
    let source = wrap_snippet(code, snippet_type, Language::Java);
    let producer = JavaBytecodeProducer::new();
    producer
        .parse(&source, "__Eval__.java")
        .map_err(|e| EvalError::ParseError(format!("{e:?}")))?;
    let context = CompilationContext {
        source: Some(source),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("__Eval__.java".into()),
        metadata: HashMap::default(),
    };
    producer
        .produce_bytes(&context)
        .map_err(|e| EvalError::CompilationError(format!("{e:?}")))
}

/// Evaluate Kotlin code
fn eval_kotlin(code: &str, options: &EvalOptions) -> Result<EvalResult, EvalError> {
    use hyperion_bytecode::kotlin_producer::KotlinBytecodeProducer;

    let snippet_type = detect_snippet_type(code, Language::Kotlin);
    let source = wrap_snippet(code, snippet_type, Language::Kotlin);

    if options.verbose {
        eprintln!("Snippet type: {snippet_type:?}");
        eprintln!("Wrapped source:\n{source}");
    }

    let producer = KotlinBytecodeProducer::new();

    // Parse and compile
    producer
        .parse(&source, "__Eval__.kt")
        .map_err(|e| EvalError::ParseError(format!("{e:?}")))?;

    let context = CompilationContext {
        source: Some(source),
        target_version: 65,
        debug_info: false,
        include_source_file: false,
        source_path: Some("__Eval__.kt".into()),
        metadata: HashMap::default(),
    };

    let bytecode = producer
        .produce_bytes(&context)
        .map_err(|e| EvalError::CompilationError(format!("{e:?}")))?;

    if options.verbose {
        let size = bytecode.len();
        eprintln!("Compiled __Eval__ ({size} bytes)");
    }

    // Create VM and load class
    let config = MemoryConfig {
        max_instructions: options.timeout_ms * 10_000,
        ..MemoryConfig::default()
    };
    let mut vm = VirtualMachine::new(config);

    vm.load_class_from_bytes("__Eval__Kt", &bytecode)
        .map_err(|e| EvalError::ExecutionError(format!("Failed to load class: {e:?}")))?;

    // Kotlin top-level functions compile to <ClassName>Kt class with static methods
    let result = vm
        .invoke_static("__Eval__Kt", "__eval__", "()Ljava/lang/Object;", &[])
        .map_err(|e| EvalError::ExecutionError(format!("{e:?}")))?;

    let (display, result_type) = format_value(&result, &vm, options.show_type);

    Ok(EvalResult {
        value: result,
        display,
        result_type,
        is_void: false,
    })
}

/// Format a VM value for display
fn format_value(value: &Value, vm: &VirtualMachine, show_type: bool) -> (String, Option<String>) {
    match value {
        Value::Int(i) => {
            let ty = if show_type {
                Some("int".to_string())
            } else {
                None
            };
            (i.to_string(), ty)
        }
        Value::Long(l) => {
            let ty = if show_type {
                Some("long".to_string())
            } else {
                None
            };
            (format!("{l}L"), ty)
        }
        Value::Float(f) => {
            let ty = if show_type {
                Some("float".to_string())
            } else {
                None
            };
            (format!("{f}f"), ty)
        }
        Value::Double(d) => {
            let ty = if show_type {
                Some("double".to_string())
            } else {
                None
            };
            (d.to_string(), ty)
        }
        Value::Reference(ObjectRef::Null) => {
            let ty = if show_type {
                Some("null".to_string())
            } else {
                None
            };
            ("null".to_string(), ty)
        }
        Value::Reference(ObjectRef::Object(heap_id)) => {
            let display = vm.display_value(value);
            let ty = if show_type {
                // Report the runtime class (slashes → dots) so `-t` is honest
                // about what was produced, e.g. `java.lang.String`.
                Some(
                    vm.object_class_name(*heap_id)
                        .unwrap_or_else(|| "Object".to_string())
                        .replace('/', "."),
                )
            } else {
                None
            };
            (display, ty)
        }
        Value::Reference(ObjectRef::Array(_)) => {
            let display = vm.display_value(value);
            let ty = if show_type {
                Some("Array".to_string())
            } else {
                None
            };
            (display, ty)
        }
        Value::Reference(ObjectRef::MethodHandle(mh_id)) => {
            let ty = if show_type {
                Some("MethodHandle".to_string())
            } else {
                None
            };
            let id = mh_id.0;
            (format!("MethodHandle@{id}"), ty)
        }
        Value::ReturnAddress(addr) => {
            let ty = if show_type {
                Some("returnAddress".to_string())
            } else {
                None
            };
            (format!("@{addr}"), ty)
        }
        Value::CategoryTwoPlaceholder => {
            let ty = if show_type {
                Some("placeholder".to_string())
            } else {
                None
            };
            ("<placeholder>".to_string(), ty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_kotlin_display(code: &str) -> String {
        eval(
            code,
            &EvalOptions {
                language: Some(Language::Kotlin),
                ..Default::default()
            },
        )
        .expect("Kotlin eval should succeed")
        .display
    }

    #[test]
    fn kotlin_eval_executes_literals_and_integer_arithmetic() {
        assert_eq!(eval_kotlin_display("100"), "100");
        assert_eq!(eval_kotlin_display("10 + 5 * 2"), "20");
        assert_eq!(eval_kotlin_display("\"hello\""), "hello");
        assert_eq!(eval_kotlin_display("true"), "true");
        assert_eq!(eval_kotlin_display("false"), "false");
        assert_eq!(eval_kotlin_display("-10 + 3"), "-7");
        assert_eq!(eval_kotlin_display("!false"), "true");
    }

    #[test]
    fn kotlin_eval_rejects_unsupported_code_instead_of_inventing_results() {
        for code in ["println(1)", "val value = 1"] {
            let error = eval(
                code,
                &EvalOptions {
                    language: Some(Language::Kotlin),
                    ..Default::default()
                },
            )
            .expect_err("unsupported Kotlin must not produce a fabricated value");
            assert!(
                matches!(error, EvalError::ParseError(_)),
                "unexpected error for {code:?}: {error}"
            );
        }
    }

    #[test]
    fn java_eval_executes_target_typed_functional_expressions() {
        let result = eval(
            r#"
                class Main {
                    static int run() {
                        java.util.function.Function<String,Integer> parse = Integer::parseInt;
                        java.util.function.Function<Integer,Integer> adjust = value -> {
                            int doubled = value * 2;
                            return doubled + 1;
                        };
                        return adjust.apply(parse.apply("20"));
                    }
                }
            "#,
            &EvalOptions {
                language: Some(Language::Java),
                ..Default::default()
            },
        )
        .expect("Java eval should execute the full class");

        assert_eq!(result.display, "41");
    }

    #[test]
    fn java_eval_preserves_qualified_static_method_return_types() {
        let result = eval(
            r#"class T {
                static int run() { return T.label().length(); }
                static String label() { return "abcdefgh"; }
            }"#,
            &EvalOptions {
                language: Some(Language::Java),
                ..Default::default()
            },
        )
        .expect("qualified static return type should support chaining");

        assert_eq!(result.display, "8");
    }

    #[test]
    fn java_eval_lowers_string_formatted_varargs() {
        let result = eval(
            r#"for (int i = 0; i < 3; i++) System.out.println("Hello world! %d".formatted(i));"#,
            &EvalOptions {
                language: Some(Language::Java),
                ..Default::default()
            },
        )
        .expect("String.formatted varargs should compile and execute");

        assert!(result.is_void);

        let formatted = eval(
            r#""Hello world! %d".formatted(7)"#,
            &EvalOptions {
                language: Some(Language::Java),
                ..Default::default()
            },
        )
        .expect("String.formatted should return its formatted String");
        assert_eq!(formatted.display, "Hello world! 7");
    }

    #[test]
    fn java_eval_rejects_calls_without_an_arity_compatible_overload() {
        let cases = [
            r#"System.out.println("a", "b");"#,
            r#"System.out.print(1, 2);"#,
            "System.out.print();",
            r#"return "x".length(1);"#,
            "return Math.sqrt(1, 2);",
            "class T { static int f() { return 1; } static int run() { return f(2); } }",
            "class T { static int f() { return 1; } static int run() { return T.f(2); } }",
            "class T { int f() { return 1; } static int run() { return new T().f(2); } }",
            "class T { T() {} static int run() { new T(2); return 1; } }",
        ];

        for source in cases {
            let error = eval(
                source,
                &EvalOptions {
                    language: Some(Language::Java),
                    ..Default::default()
                },
            )
            .expect_err("invalid Java call arity must fail compilation");
            assert!(
                matches!(error, EvalError::CompilationError(_)),
                "unexpected error for {source:?}: {error}"
            );
            assert!(error.to_string().contains("accepts"), "{error}");
        }
    }

    #[test]
    fn java_eval_rejects_void_expressions_used_as_arguments() {
        let cases = [
            "class T { static void v() {} static void run() { System.out.println(v()); } }",
            "class T { static void v() {} static void f(Object x) {} static void run() { f(v()); } }",
            "class T { static void v() {} static void run() { Math.abs(v()); } }",
        ];

        for source in cases {
            let error = eval(
                source,
                &EvalOptions {
                    language: Some(Language::Java),
                    ..Default::default()
                },
            )
            .expect_err("void-valued arguments must fail compilation");
            assert!(
                matches!(error, EvalError::CompilationError(_)),
                "unexpected error for {source:?}: {error}"
            );
            assert!(error.to_string().contains("void expression"), "{error}");
        }
    }

    #[test]
    fn test_eval_groovy_arithmetic() {
        let result = eval(
            "2 + 3 * 4",
            &EvalOptions {
                language: Some(Language::Groovy),
                ..Default::default()
            },
        );
        assert!(result.is_ok(), "eval failed: {result:?}");
        let result = result.expect("eval should succeed");
        // Result might be boxed Integer or primitive int
        let display = &result.display;
        assert!(
            result.display == "14" || result.display.contains("14"),
            "Expected 14, got {display}",
        );
    }

    #[test]
    fn test_eval_groovy_string_length() {
        let result = eval(
            "\"hello\".length()",
            &EvalOptions {
                language: Some(Language::Groovy),
                ..Default::default()
            },
        );
        assert!(result.is_ok(), "eval failed: {result:?}");
        let result = result.expect("eval should succeed");
        let display = &result.display;
        assert!(
            result.display == "5" || result.display.contains('5'),
            "Expected 5, got {display}",
        );
    }

    #[test]
    fn groovy_eval_rejects_unlowered_control_flow() {
        for code in [
            "break;",
            "continue;",
            "switch (1) { case 1: return 2; default: return 3 };",
        ] {
            let result = eval(
                code,
                &EvalOptions {
                    language: Some(Language::Groovy),
                    ..Default::default()
                },
            );
            assert!(
                result.is_err(),
                "unsupported control flow produced a value for {code:?}: {result:?}"
            );
            let error = result.unwrap_err();
            assert!(
                matches!(error, EvalError::CompilationError(_)),
                "unexpected error for {code:?}: {error}"
            );
        }
    }

    fn eval_groovy_display(code: &str) -> String {
        eval(
            code,
            &EvalOptions {
                language: Some(Language::Groovy),
                ..Default::default()
            },
        )
        .expect("eval should succeed")
        .display
    }

    #[test]
    fn test_display_string_result() {
        // A String result must render its contents, not an `Object@N` handle.
        assert_eq!(eval_groovy_display("\"hello\".toUpperCase()"), "HELLO");
    }

    #[test]
    fn test_display_list_result() {
        assert_eq!(eval_groovy_display("[1, 2, 3]"), "[1, 2, 3]");
    }

    #[test]
    fn test_display_map_result() {
        // HashMap iteration order is unspecified; assert on the rendered shape.
        let d = eval_groovy_display("[a:1]");
        assert_eq!(d, "{a=1}");
    }

    #[test]
    fn test_chained_string_concat() {
        // Regression: a `+` chain of 3+ operands previously recompiled the
        // left operand, corrupting the operand stack and yielding only the
        // last piece (`"a"+"b"+"c"` -> "c"). All operands must survive.
        assert_eq!(eval_groovy_display("\"a\"+\"b\"+\"c\""), "abc");
        assert_eq!(eval_groovy_display("\"a\"+\"b\"+1"), "ab1");
        assert_eq!(eval_groovy_display("\"a\"+1+\"b\""), "a1b");
        assert_eq!(eval_groovy_display("\"a\"+\"b\"+1+2"), "ab12");
    }

    #[test]
    fn test_number_first_string_concat() {
        assert_eq!(eval_groovy_display("1+\"a\""), "1a");
        assert_eq!(eval_groovy_display("10L+\" items\""), "10 items");
        assert_eq!(eval_groovy_display("1+2+\" items\""), "3 items");
        assert_eq!(eval_groovy_display("1+(\"a\"+2)"), "1a2");
    }

    #[test]
    fn test_concat_renders_non_string_operands() {
        // The runtime `append` path must stringify booleans and collections,
        // not drop them (previously rendered as "").
        assert_eq!(eval_groovy_display("\"v\"+true"), "vtrue");
        assert_eq!(eval_groovy_display("\"x=\"+[1, 2, 3]"), "x=[1, 2, 3]");
    }

    #[test]
    fn test_long_literal_not_truncated() {
        // Regression: `parse_int_literal` ignored the `L` suffix/magnitude and
        // emitted `IntLiteral`, which truncated via `as i32`
        // (`10000000000L` -> 1410065408). Long literals must round-trip.
        assert_eq!(eval_groovy_display("10000000000L"), "10000000000");
        assert_eq!(eval_groovy_display("10000000000"), "10000000000");
        assert_eq!(eval_groovy_display("\"L=\"+10000000000L"), "L=10000000000");
    }

    #[test]
    fn test_double_literal_and_boxing() {
        // Regression: 8-byte (long/double) constant-pool entries reserve a
        // phantom second slot that was being serialized, corrupting the class
        // file (ClassFormatError); and `Double.valueOf(D)` boxing was missing.
        assert_eq!(eval_groovy_display("3.5"), "3.5");
        assert_eq!(eval_groovy_display("4.0"), "4.0");
        assert_eq!(eval_groovy_display("1.5 + 2.5"), "4.0");
    }

    #[test]
    fn test_comparison_displays_as_boolean() {
        // A Groovy comparison is a boolean: it must display as `true`/`false`,
        // not `1`/`0` (the result is now typed Boolean so it boxes correctly).
        assert_eq!(eval_groovy_display("2 > 1"), "true");
        assert_eq!(eval_groovy_display("3 < 2"), "false");
        assert_eq!(eval_groovy_display("1 == 1"), "true");
        assert_eq!(eval_groovy_display("\"r=\" + (2 > 1)"), "r=true");
        assert_eq!(eval_groovy_display("3.5 < 4.0"), "true");
        assert_eq!(eval_groovy_display("3.5 > 4.0"), "false");
        assert_eq!(eval_groovy_display("3.5 <= 3.5"), "true");
        assert_eq!(eval_groovy_display("3.5 >= 3.5"), "true");
    }

    #[test]
    fn test_method_results_display_as_booleans() {
        assert_eq!(eval_groovy_display("[1,2,3].isEmpty()"), "false");
        assert_eq!(eval_groovy_display("[].isEmpty()"), "true");
        assert_eq!(eval_groovy_display("\"hello\".startsWith(\"he\")"), "true");
        assert_eq!(eval_groovy_display("\"hello\".endsWith(\"no\")"), "false");
        assert_eq!(eval_groovy_display("\"empty=\"+[].isEmpty()"), "empty=true");
    }

    #[test]
    fn test_long_int_mixed_arithmetic() {
        // Regression: `long * int` emitted LMUL against a [long, int] stack and
        // underflowed (panic). The narrower int operand must be widened (i2l).
        assert_eq!(eval_groovy_display("3000000000 * 2"), "6000000000");
        assert_eq!(eval_groovy_display("10000000000L + 5"), "10000000005");
    }

    #[test]
    fn test_groovy_decimal_division() {
        assert_eq!(eval_groovy_display("10 / 3"), "3.3333333333");
        assert_eq!(eval_groovy_display("10 / 2"), "5");
        assert_eq!(eval_groovy_display("-10 / 3"), "-3.3333333333");
        assert_eq!(eval_groovy_display("10.0 / 4"), "2.5");

        // A decimal result remains numeric in either operand position; it is
        // not merely formatted differently at the top-level return boundary.
        assert_eq!(eval_groovy_display("(10 / 3) * 3"), "9.9999999999");
        assert_eq!(eval_groovy_display("(10 / 3) + 1"), "4.3333333333");
        assert_eq!(eval_groovy_display("1 + (10 / 3)"), "4.3333333333");
        assert_eq!(eval_groovy_display("def x = 10 / 3; x + 1"), "4.3333333333");
        assert_eq!(eval_groovy_display("def x = 10 / 3; 1 + x"), "4.3333333333");
        assert_eq!(eval_groovy_display("-(10 / 3)"), "-3.3333333333");
        assert_eq!(eval_groovy_display("(10 / 3) > 3"), "true");
        assert_eq!(eval_groovy_display("(10 / 3) <= 3"), "false");
        assert_eq!(eval_groovy_display("(10 / 2) == 5"), "true");
        assert_eq!(eval_groovy_display("1.0 / 0"), "Infinity");

        let error = eval(
            "1 / 0",
            &EvalOptions {
                language: Some(Language::Groovy),
                ..Default::default()
            },
        )
        .expect_err("integral division by zero must fail");
        assert!(error.to_string().contains("ArithmeticException"), "{error}");
    }

    #[test]
    fn test_language_from_str() {
        assert_eq!(Language::from_str("java"), Ok(Language::Java));
        assert_eq!(Language::from_str("kotlin"), Ok(Language::Kotlin));
        assert_eq!(Language::from_str("groovy"), Ok(Language::Groovy));
        assert_eq!(Language::from_str("JAVA"), Ok(Language::Java));
        assert_eq!(Language::from_str("kt"), Ok(Language::Kotlin));
        assert!(Language::from_str("unknown").is_err());
    }
}
