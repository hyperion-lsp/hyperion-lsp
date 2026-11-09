// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0
//! Java EXECUTION tests — compile real Java, run it on the VM, assert on results.
//!
//! Unlike `java_integration_test.rs` (which only checks that bytecode is produced),
//! these tests verify that the produced bytecode actually *executes correctly*.
//! Each test compiles a class with a static method that returns a value, then
//! invokes it through the VirtualMachine and asserts on the returned `Value`.

use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use hyperion_bytecode::java_producer::JavaBytecodeProducer;
use hyperion_bytecode::producer::{BytecodeProducer, CompilationContext};

/// Compile a single Java class and invoke a static method, returning the VM result.
fn run_static(source: &str, class: &str, method: &str, descriptor: &str) -> Result<Value, String> {
    let producer = JavaBytecodeProducer::new();
    producer
        .parse(source, &format!("{class}.java"))
        .map_err(|e| format!("Parse failed: {e:?}"))?;

    let context = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some(format!("{class}.java")),
        ..Default::default()
    };
    let bytecode = producer
        .produce_bytes(&context)
        .map_err(|e| format!("Bytecode generation failed: {e:?}"))?;

    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes(class, &bytecode)
        .map_err(|e| format!("Class load failed: {e:?}"))?;
    vm.invoke_static(class, method, descriptor, &[])
        .map_err(|e| format!("Execution failed: {e:?}"))
}

/// Compile a unit, load *all* its top-level classes, and invoke a static
/// method on the named primary class. Exercises cross-class loading.
fn run_static_multi(source: &str, class: &str, method: &str, descriptor: &str) -> i32 {
    let producer = JavaBytecodeProducer::new();
    producer
        .parse(source, &format!("{class}.java"))
        .expect("parse");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let classes = producer
        .produce_all_classes()
        .expect("every class of the unit compiles");
    let report = vm.load_classes_from_bytes(&classes);
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
    assert!(
        report.is_loaded(class),
        "primary class {class} was not loaded"
    );
    match vm.invoke_static(class, method, descriptor, &[]) {
        Ok(Value::Int(v)) => v,
        Ok(other) => panic!("expected Int, got {other:?}"),
        Err(e) => panic!("{e:?}"),
    }
}

fn run_int(source: &str, method: &str) -> i32 {
    match run_static(source, "T", method, "()I") {
        Ok(Value::Int(v)) => v,
        Ok(other) => panic!("expected Int, got {other:?}"),
        Err(e) => panic!("{e}"),
    }
}

// ── Level 1: a method that just returns a constant ──────────────────────────
#[test]
fn returns_constant() {
    let src = "public class T { public static int run() { return 42; } }";
    assert_eq!(run_int(src, "run"), 42);
}

// ── Level 2: arithmetic with locals ─────────────────────────────────────────
#[test]
fn locals_and_arithmetic() {
    let src = "public class T { public static int run() { int a = 6; int b = 7; return a * b; } }";
    assert_eq!(run_int(src, "run"), 42);
}

// ── Level 3: if/else with comparison ────────────────────────────────────────
#[test]
fn if_else_branch() {
    let src = "public class T { public static int run() { int x = 10; if (x > 5) { return 1; } else { return 0; } } }";
    assert_eq!(run_int(src, "run"), 1);
}

// ── Level 4: while loop accumulation ────────────────────────────────────────
#[test]
fn while_loop_sum() {
    let src = "public class T { public static int run() { int s = 0; int i = 1; while (i <= 10) { s = s + i; i = i + 1; } return s; } }";
    assert_eq!(run_int(src, "run"), 55);
}

// ── Level 5: for loop ───────────────────────────────────────────────────────
#[test]
fn for_loop_sum() {
    let src = "public class T { public static int run() { int s = 0; for (int i = 1; i <= 10; i = i + 1) { s = s + i; } return s; } }";
    assert_eq!(run_int(src, "run"), 55);
}

// ── Level 6: static method call (helper) ────────────────────────────────────
#[test]
fn static_call() {
    let src = "public class T { static int dbl(int n) { return n * 2; } public static int run() { return dbl(21); } }";
    assert_eq!(run_int(src, "run"), 42);
}

// ── Level 7: recursion ──────────────────────────────────────────────────────
#[test]
fn recursion_fib() {
    let src = "public class T { static int fib(int n) { if (n < 2) return n; return fib(n-1) + fib(n-2); } public static int run() { return fib(10); } }";
    assert_eq!(run_int(src, "run"), 55);
}

fn run_long(source: &str, method: &str) -> i64 {
    match run_static(source, "T", method, "()J") {
        Ok(Value::Long(v)) => v,
        Ok(other) => panic!("expected Long, got {other:?}"),
        Err(e) => panic!("{e}"),
    }
}

fn run_double(source: &str, method: &str) -> f64 {
    match run_static(source, "T", method, "()D") {
        Ok(Value::Double(v)) => v,
        Ok(other) => panic!("expected Double, got {other:?}"),
        Err(e) => panic!("{e}"),
    }
}

fn run_string(source: &str, method: &str) -> String {
    let producer = JavaBytecodeProducer::new();
    producer.parse(source, "T.java").expect("parse");
    let ctx = CompilationContext {
        source: Some(source.to_string()),
        source_path: Some("T.java".into()),
        ..Default::default()
    };
    let bytecode = producer.produce_bytes(&ctx).expect("compile");
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    vm.load_class_from_bytes("T", &bytecode).expect("load");
    match vm.invoke_static("T", method, "()Ljava/lang/String;", &[]) {
        Ok(Value::Reference(r)) => {
            let id = r.heap_id().expect("string ref");
            vm.heap_string(id).expect("read string")
        }
        Ok(other) => panic!("expected String ref, got {other:?}"),
        Err(e) => panic!("{e}"),
    }
}

// ── Level 8: long arithmetic ────────────────────────────────────────────────
#[test]
fn long_arithmetic() {
    let src = "public class T { public static long run() { long a = 1000000; long b = 1000000; return a * b; } }";
    assert_eq!(run_long(src, "run"), 1_000_000_000_000);
}

// ── Level 9: string concatenation ───────────────────────────────────────────
#[test]
fn string_concat() {
    let src =
        r#"public class T { public static String run() { int n = 42; return "answer=" + n; } }"#;
    assert_eq!(run_string(src, "run"), "answer=42");
}

// ── Level 10: int array sum ─────────────────────────────────────────────────
#[test]
fn int_array_sum() {
    let src = "public class T { public static int run() { int[] a = new int[5]; for (int i = 0; i < 5; i++) { a[i] = i * i; } int s = 0; for (int i = 0; i < 5; i++) { s = s + a[i]; } return s; } }";
    assert_eq!(run_int(src, "run"), 30); // 0+1+4+9+16
}

// ════════════════════════════════════════════════════════════════════════════
// Targeted regression tests, one (or more) per language feature. Each compiles
// a `static int/long/String run()` and asserts on the executed result so a
// regression surfaces as a wrong value, not just a compile error.
// ════════════════════════════════════════════════════════════════════════════

// Boolean logic in branch context: && / || / !.
#[test]
fn feat_logical_and() {
    let src = "public class T { public static int run() { int a = 5; int b = 3; if (a > 0 && b > 0) { return 1; } return 0; } }";
    assert_eq!(run_int(src, "run"), 1);
}
#[test]
fn feat_logical_and_short_circuits_false() {
    let src = "public class T { public static int run() { int a = 5; int b = -1; if (a > 0 && b > 0) { return 1; } return 0; } }";
    assert_eq!(run_int(src, "run"), 0);
}
#[test]
fn feat_logical_or_and_not() {
    let src = "public class T { public static int run() { int a = 0; if (a > 0 || !(a < 0)) { return 7; } return 0; } }";
    assert_eq!(run_int(src, "run"), 7);
}

// Comparison operators materialised as a boolean value.
#[test]
fn feat_comparison_as_value() {
    let src = "public class T { public static int run() { int a = 4; boolean b = a == 4; if (b) { return 1; } return 0; } }";
    assert_eq!(run_int(src, "run"), 1);
}

// Increment / decrement and compound assignment.
#[test]
fn feat_increments_and_compound_assign() {
    let src = "public class T { public static int run() { int x = 5; x++; ++x; x += 3; x *= 2; x -= 1; return x; } }";
    // 5 -> 6 -> 7 -> 10 -> 20 -> 19
    assert_eq!(run_int(src, "run"), 19);
}
#[test]
fn feat_decrement() {
    let src = "public class T { public static int run() { int x = 10; x--; --x; return x; } }";
    assert_eq!(run_int(src, "run"), 8);
}

// Left-associative operator chains and precedence.
#[test]
fn feat_chain_and_precedence() {
    let src = "public class T { public static int run() { return 2 * 3 + 4 * 5; } }";
    assert_eq!(run_int(src, "run"), 26);
}
#[test]
fn feat_long_addition_chain() {
    let src = "public class T { public static int run() { return 1 + 2 + 3 + 4 + 5 + 6; } }";
    assert_eq!(run_int(src, "run"), 21);
}

// String concatenation chains mixing String and primitives.
#[test]
fn feat_string_concat_chain() {
    let src = r#"public class T { public static String run() { int x = 1; int y = 2; return "x=" + x + ",y=" + y; } }"#;
    assert_eq!(run_string(src, "run"), "x=1,y=2");
}

// Enhanced-for (for-each) over an array, plus array .length via desugaring.
#[test]
fn feat_foreach_over_array() {
    let src = "public class T { public static int run() { int[] a = new int[4]; a[0]=1; a[1]=2; a[2]=3; a[3]=4; int s = 0; for (int v : a) { s += v; } return s; } }";
    assert_eq!(run_int(src, "run"), 10);
}
#[test]
fn feat_array_length() {
    let src =
        "public class T { public static int run() { int[] a = new int[7]; return a.length; } }";
    assert_eq!(run_int(src, "run"), 7);
}

// Casts (including a narrowing long -> int).
#[test]
fn feat_cast_long_to_int() {
    let src =
        "public class T { public static int run() { long l = 5; int x = (int) l; return x; } }";
    assert_eq!(run_int(src, "run"), 5);
}

// Char literals are treated as their integer code point.
#[test]
fn feat_char_literal() {
    let src = "public class T { public static int run() { char c = 'A'; return c; } }";
    assert_eq!(run_int(src, "run"), 65);
}

// `null` literal compiles and runs on a non-dereferencing path.
#[test]
fn feat_null_literal() {
    let src = "public class T { public static int run() { String s = null; return 42; } }";
    assert_eq!(run_int(src, "run"), 42);
}

// `throw` compiles; the non-throwing path executes correctly.
#[test]
fn feat_throw_untaken_path() {
    let src = "public class T { static int half(int n) { if (n < 0) { throw new RuntimeException(); } return n / 2; } public static int run() { return half(8); } }";
    assert_eq!(run_int(src, "run"), 4);
}

// Nested method-call arguments (comma splitting + folding).
#[test]
fn feat_nested_calls() {
    let src = "public class T { static int sq(int n) { return n * n; } static int add(int a, int b) { return a + b; } public static int run() { return add(sq(2), sq(3)); } }";
    assert_eq!(run_int(src, "run"), 13);
}

// long return value and arithmetic precision beyond i32.
#[test]
fn feat_long_return_overflow_range() {
    let src = "public class T { public static long run() { long a = 3000000000; return a + 1; } }";
    assert_eq!(run_long(src, "run"), 3_000_000_001);
}

// Parenthesized operator chains must keep every term (regression: a parenthesized
// 3-term chain dropped its last operand, turning a trinomial recurrence into
// Pascal's triangle — caught by the JVM-differential corpus harness).
#[test]
fn feat_parenthesized_chain_keeps_all_terms() {
    let src = "public class T { public static int run() { int a = 1; int b = 2; int c = 3; return (a + b + c); } }";
    assert_eq!(run_int(src, "run"), 6);
}
#[test]
fn feat_nested_parens() {
    let src = "public class T { public static int run() { int a = 1; int b = 2; int c = 3; return ((a + b) + (b + c)); } }";
    assert_eq!(run_int(src, "run"), 8); // (1+2) + (2+3)
}
#[test]
fn feat_array_index_chain() {
    let src = "public class T { public static int run() { int[] a = new int[5]; a[1+2] = 9; return a[1 + 2]; } }";
    assert_eq!(run_int(src, "run"), 9);
}

// ── Level 12: a "complex application" — objects + arrays + instance methods ──
#[test]
fn complex_application() {
    let src = "public class T { \
        int[] data; int n; \
        T(int size) { this.data = new int[size]; this.n = size; } \
        void fill() { for (int i = 0; i < this.n; i++) { this.data[i] = (i * 7) % 13; } } \
        int sum() { int s = 0; for (int i = 0; i < this.n; i++) { s += this.data[i]; } return s; } \
        int max() { int m = this.data[0]; for (int i = 1; i < this.n; i++) { if (this.data[i] > m) { m = this.data[i]; } } return m; } \
        public static int run() { T t = new T(10); t.fill(); return t.sum() * 100 + t.max(); } }";
    // sum of (i*7)%13 for i in 0..9 = 55; max = 11  → 55*100 + 11 = 5511
    assert_eq!(run_int(src, "run"), 5511);
}

// ── Level 11: objects — fields, constructor, instance method ────────────────
#[test]
fn objects_basic() {
    let src = "public class T { \
        int value; \
        T(int start) { this.value = start; } \
        int next() { this.value = this.value + 1; return this.value; } \
        public static int run() { T c = new T(10); c.next(); c.next(); return c.next(); } }";
    assert_eq!(run_int(src, "run"), 13);
}

// ── array initializers `{ ... }` ─────────────────────────────────────────────
#[test]
fn feat_array_init_int() {
    let src = "public class T { public static int run() { int[] a = {10, 20, 30}; return a[0] + a[1] + a[2]; } }";
    assert_eq!(run_int(src, "run"), 60);
}
#[test]
fn feat_array_init_foreach() {
    let src = "public class T { public static int run() { int[] a = {1, 2, 3, 4, 5}; int s = 0; for (int v : a) { s += v; } return s; } }";
    assert_eq!(run_int(src, "run"), 15);
}
#[test]
fn feat_array_init_double_widens_int_literals() {
    // int literals in a double[] must store via dastore (declared element type).
    let src = "public class T { public static double run() { double[] d = {1, 2, 3}; return d[0] + d[1] + d[2]; } }";
    assert_eq!(run_double(src, "run"), 6.0);
}

// ── var (local type inference) ───────────────────────────────────────────────
#[test]
fn feat_var_int() {
    let src = "public class T { public static int run() { var a = 6; var b = 7; return a * b; } }";
    assert_eq!(run_int(src, "run"), 42);
}
#[test]
fn feat_var_long_widths() {
    // `var` must infer the category-2 width for a long initializer.
    let src = "public class T { public static long run() { var a = 1000000L; var b = 1000000L; return a * b; } }";
    assert_eq!(run_long(src, "run"), 1_000_000_000_000);
}
#[test]
fn feat_var_in_for_loop() {
    let src = "public class T { public static int run() { int s = 0; for (var i = 0; i < 5; i++) { s += i; } return s; } }";
    assert_eq!(run_int(src, "run"), 10);
}

// ── operator chains as a comparison operand (regression) ─────────────────────
// `a + b + c == t` parses the `==` node with the additive chain as flat
// continuation siblings; the binary converter must fold each side as a chain
// rather than recursing into a bare continuation (was "Missing right operand").
#[test]
fn feat_chain_as_comparison_operand() {
    let src = "public class T { public static int run() { int a = 1, b = 2, c = 3; int t = 6; if (a + b + c == t) { return 100; } return 0; } }";
    assert_eq!(run_int(src, "run"), 100);
}
#[test]
fn feat_chain_both_sides_of_comparison() {
    let src = "public class T { public static int run() { if (1 + 2 + 3 == 4 + 2) { return 7; } return 0; } }";
    assert_eq!(run_int(src, "run"), 7);
}

// ── stdlib static fields (Integer.MAX_VALUE, Math.PI, …) ─────────────────────
#[test]
fn feat_integer_max_value() {
    let src =
        "public class T { public static int run() { int m = Integer.MAX_VALUE; return m - 7; } }";
    assert_eq!(run_int(src, "run"), i32::MAX - 7);
}
#[test]
fn feat_integer_min_value() {
    let src = "public class T { public static int run() { return Integer.MIN_VALUE + 5; } }";
    assert_eq!(run_int(src, "run"), i32::MIN + 5);
}
#[test]
fn feat_math_pi() {
    let src = "public class T { public static double run() { return Math.PI; } }";
    assert_eq!(run_double(src, "run"), std::f64::consts::PI);
}
#[test]
fn feat_long_max_value() {
    let src = "public class T { public static long run() { return Long.MAX_VALUE; } }";
    assert_eq!(run_long(src, "run"), i64::MAX);
}
#[test]
fn feat_static_field_in_min_search() {
    // Idiomatic `int best = Integer.MAX_VALUE;` then minimize.
    let src = "public class T { public static int run() { int[] a = {5, 2, 8, 1, 9}; int best = Integer.MAX_VALUE; for (int v : a) { if (v < best) { best = v; } } return best; } }";
    assert_eq!(run_int(src, "run"), 1);
}

// ── stdlib static methods (Math.*, Integer.parseInt/compare) ─────────────────
#[test]
fn feat_math_sqrt_pow() {
    let src = "public class T { public static double run() { return Math.sqrt(16.0) + Math.pow(2.0, 10.0); } }";
    assert_eq!(run_double(src, "run"), 4.0 + 1024.0);
}
#[test]
fn feat_math_pow_int_args_coerced() {
    // int arguments to Math.pow/sqrt must widen to double (descriptor (DD)D).
    let src = "public class T { public static double run() { return Math.pow(2, 10) + Math.sqrt(144); } }";
    assert_eq!(run_double(src, "run"), 1024.0 + 12.0);
}
#[test]
fn feat_math_max_min_abs() {
    let src = "public class T { public static int run() { return Math.max(3, 8) + Math.min(3, 8) + Math.abs(-5); } }";
    // 8 + 3 + 5
    assert_eq!(run_int(src, "run"), 16);
}
#[test]
fn feat_integer_compare() {
    let src = "public class T { public static int run() { return Integer.compare(5, 9) + Integer.compare(9, 5) + Integer.compare(4, 4); } }";
    // -1 + 1 + 0
    assert_eq!(run_int(src, "run"), 0);
}
#[test]
fn feat_math_round_double() {
    let src = "public class T { public static long run() { return Math.round(3.7); } }";
    assert_eq!(run_long(src, "run"), 4);
}

#[test]
fn math_abs_preserves_twos_complement_minimum_values() {
    let int_src =
        "public class T { public static int run() { return Math.abs(Integer.MIN_VALUE); } }";
    let long_src =
        "public class T { public static long run() { return Math.abs(Long.MIN_VALUE); } }";
    assert_eq!(run_int(int_src, "run"), i32::MIN);
    assert_eq!(run_long(long_src, "run"), i64::MIN);
}

#[test]
fn math_round_uses_java_negative_half_semantics() {
    let src = "public class T { public static long run() { return Math.round(-1.5); } }";
    assert_eq!(run_long(src, "run"), -1);
}

#[test]
fn math_float_extrema_preserve_nan_and_signed_zero() {
    let nan_src =
        "public class T { public static double run() { return Math.max(Double.NaN, 1.0); } }";
    let max_zero_src =
        "public class T { public static double run() { return Math.max(-0.0, 0.0); } }";
    let min_zero_src =
        "public class T { public static double run() { return Math.min(-0.0, 0.0); } }";

    assert!(run_double(nan_src, "run").is_nan());
    assert_eq!(run_double(max_zero_src, "run").to_bits(), 0.0f64.to_bits());
    assert_eq!(
        run_double(min_zero_src, "run").to_bits(),
        (-0.0f64).to_bits()
    );
}

#[test]
fn math_floor_division_follows_divisor_sign() {
    let src = "public class T { public static int run() { return Math.floorDiv(-7, -2) * 1000 + Math.floorMod(-7, -2) * 100 + Math.floorDiv(7, -2) * 10 + Math.floorMod(7, -2); } }";
    // 3, -1, -4, -1 respectively.
    assert_eq!(run_int(src, "run"), 2859);
}

#[test]
fn math_add_exact_reports_overflow() {
    let src = "public class T { public static int run() { return Math.addExact(Integer.MAX_VALUE, 1); } }";
    let error = run_static(src, "T", "run", "()I").expect_err("overflow must fail");
    assert!(error.contains("Arithmetic"), "unexpected error: {error}");
}

// ── multi-dimensional arrays (multianewarray) ────────────────────────────────
#[test]
fn feat_multidim_array() {
    let src = "public class T { public static int run() { int[][] m = new int[3][4]; m[1][2] = 7; m[2][3] = 5; return m[1][2] + m[2][3]; } }";
    assert_eq!(run_int(src, "run"), 12);
}
#[test]
fn feat_multidim_array_lengths() {
    let src = "public class T { public static int run() { int[][] m = new int[3][4]; return m.length * 10 + m[0].length; } }";
    // 3 rows, 4 cols → 34
    assert_eq!(run_int(src, "run"), 34);
}
#[test]
fn feat_multidim_array_sum() {
    let src = "public class T { public static int run() { int[][] m = new int[2][2]; for (int i = 0; i < 2; i++) { for (int j = 0; j < 2; j++) { m[i][j] = i * 2 + j; } } int s = 0; for (int i = 0; i < 2; i++) { for (int j = 0; j < 2; j++) { s += m[i][j]; } } return s; } }";
    // 0+1+2+3 = 6
    assert_eq!(run_int(src, "run"), 6);
}

// ── String instance methods ──────────────────────────────────────────────────
#[test]
fn feat_string_methods() {
    let src = "public class T { public static int run() { \
        String s = \"Hello\"; int r = 0; \
        if (s.length() == 5) { r += 1; } \
        if (s.charAt(1) == 'e') { r += 10; } \
        if (s.substring(1, 3).equals(\"el\")) { r += 100; } \
        if (s.toUpperCase().equals(\"HELLO\")) { r += 1000; } \
        if (s.indexOf(\"ll\") == 2) { r += 10000; } \
        return r; } }";
    assert_eq!(run_int(src, "run"), 11111);
}

#[test]
fn string_literals_round_trip_modified_utf8_and_use_utf16_indices() {
    let src = "public class T { public static int run() { \
        String s = \"A😀B\"; \
        if (s.length() != 4) return -1; \
        if ((int)s.charAt(1) != 55357 || (int)s.charAt(2) != 56832) return -2; \
        if (!s.substring(1, 3).equals(\"😀\")) return -3; \
        return s.indexOf(\"B\"); } }";
    assert_eq!(run_int(src, "run"), 3);
}

#[test]
fn feat_string_char_iteration() {
    // toCharArray + char arithmetic — a common parsing idiom.
    let src = "public class T { public static int run() { \
        String s = \"abc\"; char[] cs = s.toCharArray(); int sum = 0; \
        for (int i = 0; i < cs.length; i++) { sum += cs[i]; } return sum; } }";
    // 'a'+'b'+'c' = 97+98+99
    assert_eq!(run_int(src, "run"), 97 + 98 + 99);
}
#[test]
fn feat_string_compare_to() {
    let src = "public class T { public static int run() { \
        String a = \"apple\"; String b = \"apply\"; \
        if (a.compareTo(b) < 0) { return 1; } return 0; } }";
    assert_eq!(run_int(src, "run"), 1);
}

// ── reference equality (== / != null, object identity) ──────────────────────
#[test]
fn feat_null_comparison() {
    let src = "public class T { public static int run() { String s = null; int r = 0; if (s == null) { r += 1; } if (s != null) { r += 10; } return r; } }";
    assert_eq!(run_int(src, "run"), 1);
}
#[test]
fn feat_object_identity() {
    let src = "public class T { static class N { } public static int run() { N a = new N(); N b = a; N c = new N(); int r = 0; if (a == b) { r += 1; } if (a == c) { r += 10; } return r; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

// ── nested classes (compiled as standalone, by simple name) ──────────────────
#[test]
fn feat_nested_class() {
    // `Node` is a nested static class; `new Node(...)` must resolve.
    let src = "public class T { \
        static class Node { int value; int next; Node(int v) { value = v; next = -1; } } \
        public static int run() { Node a = new Node(10); Node b = new Node(20); a.next = b.value; return a.value + a.next; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 30);
}
#[test]
fn feat_nested_linked_list() {
    let src = "public class T { \
        static class ListNode { int val; ListNode next; ListNode(int v) { val = v; } } \
        public static int run() { \
            ListNode head = new ListNode(1); head.next = new ListNode(2); head.next.next = new ListNode(3); \
            int sum = 0; ListNode cur = head; while (cur != null) { sum += cur.val; cur = cur.next; } return sum; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 6);
}

// ── inheritance: extends, inherited fields/methods, virtual dispatch ─────────
#[test]
fn feat_inheritance_inherited_method_and_field() {
    let src = "public class T { public static int run() { Dog d = new Dog(); return d.legs() + d.sound(); } } \
               class Animal { int base; Animal() { base = 4; } int legs() { return base; } int sound() { return 1; } } \
               class Dog extends Animal { int sound() { return 99; } }";
    // legs() inherited from Animal (reads inherited field base=4); sound()
    // overridden in Dog (virtual dispatch) = 99 → 103.
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 103);
}
#[test]
fn feat_inheritance_subclass_declared_before_super() {
    // Dog (depth 1) is declared before Animal (depth 0); load order must still
    // link Animal first so Dog inherits legs().
    let src = "public class T { public static int run() { return new Dog().legs(); } } \
               class Dog extends Animal { } \
               class Animal { int legs() { return 4; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 4);
}
#[test]
fn feat_inheritance_virtual_dispatch_through_base_ref() {
    let src =
        "public class T { public static int run() { Animal a = new Dog(); return a.sound(); } } \
               class Animal { int sound() { return 1; } } \
               class Dog extends Animal { int sound() { return 42; } }";
    // a is statically Animal but dynamically Dog → Dog.sound() = 42.
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 42);
}

// ── multi-class compilation: cross-class references within one file ───────────
#[test]
fn feat_multi_class_cross_reference() {
    let src = "public class T { public static int run() { Helper h = new Helper(); return h.triple(7); } } \
               class Helper { int triple(int n) { return n * 3; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 21);
}
#[test]
fn feat_multi_class_static_call() {
    let src = "public class T { public static int run() { return Util.square(6); } } \
               class Util { static int square(int n) { return n * n; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 36);
}

// ── records ──────────────────────────────────────────────────────────────────
// Canonical constructor + accessors are synthesized from the record header.
#[test]
fn feat_record_accessors() {
    let src = "public record T(int x, int y) { public static int run() { T t = new T(3, 4); return t.x() + t.y(); } }";
    assert_eq!(run_int(src, "run"), 7);
}
#[test]
fn feat_record_with_extra_method() {
    let src = "public record T(int a, int b) { int sum() { return a + b; } public static int run() { T t = new T(10, 20); return t.sum(); } }";
    assert_eq!(run_int(src, "run"), 30);
}

// ── switch (traditional colon form) ──────────────────────────────────────────
#[test]
fn feat_switch_basic() {
    let src = "public class T { static int f(int n) { int r = 0; switch (n) { case 1: r = 10; break; case 2: r = 20; break; default: r = -1; } return r; } public static int run() { return f(2); } }";
    assert_eq!(run_int(src, "run"), 20);
}
#[test]
fn feat_switch_default() {
    let src = "public class T { static int f(int n) { int r = 0; switch (n) { case 1: r = 10; break; default: r = 99; } return r; } public static int run() { return f(7); } }";
    assert_eq!(run_int(src, "run"), 99);
}
#[test]
fn feat_switch_grouped_labels() {
    let src = "public class T { static int f(int n) { int r = 0; switch (n) { case 1: case 2: r = 100; break; case 3: r = 300; break; } return r; } public static int run() { return f(1) + f(2) + f(3); } }";
    // 100 + 100 + 300
    assert_eq!(run_int(src, "run"), 500);
}
#[test]
fn feat_switch_fallthrough() {
    let src = "public class T { static int f(int n) { int r = 0; switch (n) { case 1: r += 1; case 2: r += 2; break; case 3: r += 30; } return r; } public static int run() { return f(1) * 1000 + f(2) * 10 + f(3); } }";
    // f(1): 1 then fall to +2 = 3; f(2): 2; f(3): 30 → 3*1000 + 2*10 + 30 = 3050
    assert_eq!(run_int(src, "run"), 3050);
}

// ── try / catch / finally ────────────────────────────────────────────────────
// Catch a VM-raised exception (idiv by zero → ArithmeticException). This
// exercises the emitted exception table + handler dispatch without needing to
// construct a JDK exception (the VM lacks java.lang.*Exception.<init> — a
// stdlib gap tracked separately).
#[test]
fn feat_try_catch_catches_arithmetic() {
    let src = "public class T { public static int run() { int x = 0; try { x = 10 / 0; } catch (ArithmeticException e) { x = 2; } return x; } }";
    assert_eq!(run_int(src, "run"), 2);
}
#[test]
fn feat_try_normal_path_skips_catch() {
    let src = "public class T { public static int run() { int x = 0; try { x = 5; } catch (ArithmeticException e) { x = 99; } return x; } }";
    assert_eq!(run_int(src, "run"), 5);
}
#[test]
fn feat_finally_runs_on_normal_path() {
    let src = "public class T { public static int run() { int x = 0; try { x = 1; } finally { x = x + 10; } return x; } }";
    assert_eq!(run_int(src, "run"), 11);
}
#[test]
fn feat_finally_runs_after_catch() {
    let src = "public class T { public static int run() { int x = 0; try { x = 1 / 0; } catch (ArithmeticException e) { x = 2; } finally { x = x + 10; } return x; } }";
    assert_eq!(run_int(src, "run"), 12);
}

// ── break / continue / do-while ──────────────────────────────────────────────
#[test]
fn feat_break_in_while() {
    let src = "public class T { public static int run() { int i = 0; while (i < 100) { if (i == 5) { break; } i = i + 1; } return i; } }";
    assert_eq!(run_int(src, "run"), 5);
}
#[test]
fn feat_continue_in_for_sums_evens() {
    let src = "public class T { public static int run() { int s = 0; for (int i = 0; i < 10; i++) { if (i % 2 == 1) { continue; } s += i; } return s; } }";
    // 0+2+4+6+8 = 20
    assert_eq!(run_int(src, "run"), 20);
}
#[test]
fn feat_break_in_for() {
    let src = "public class T { public static int run() { int found = -1; for (int i = 0; i < 10; i++) { if (i * i > 20) { found = i; break; } } return found; } }";
    // first i with i*i > 20 is 5 (25)
    assert_eq!(run_int(src, "run"), 5);
}
#[test]
fn feat_do_while() {
    let src = "public class T { public static int run() { int n = 0; int s = 0; do { s = s + n; n = n + 1; } while (n <= 5); return s; } }";
    // 0+1+2+3+4+5 = 15
    assert_eq!(run_int(src, "run"), 15);
}
#[test]
fn feat_do_while_runs_once_when_false() {
    let src = "public class T { public static int run() { int x = 0; do { x = x + 7; } while (x < 0); return x; } }";
    assert_eq!(run_int(src, "run"), 7);
}
#[test]
fn feat_continue_in_do_while() {
    let src = "public class T { public static int run() { int i = 0; int s = 0; do { i++; if (i == 3) { continue; } s += i; } while (i < 5); return s; } }";
    // i:1,2,(skip3),4,5 → 1+2+4+5 = 12
    assert_eq!(run_int(src, "run"), 12);
}
#[test]
fn feat_nested_loop_break_inner_only() {
    let src = "public class T { public static int run() { int c = 0; for (int i = 0; i < 3; i++) { for (int j = 0; j < 3; j++) { if (j == 1) { break; } c++; } } return c; } }";
    // inner runs j=0 (c++), then break at j=1 → 1 per outer iter × 3 = 3
    assert_eq!(run_int(src, "run"), 3);
}

// ── Ternary `?:` ─────────────────────────────────────────────────────────────
#[test]
fn feat_ternary_basic() {
    let src = "public class T { public static int run() { int x = 7; return x > 5 ? 1 : 0; } }";
    assert_eq!(run_int(src, "run"), 1);
}
#[test]
fn feat_ternary_false_arm() {
    let src = "public class T { public static int run() { int x = 3; return x > 5 ? 100 : 200; } }";
    assert_eq!(run_int(src, "run"), 200);
}
#[test]
fn feat_ternary_nested() {
    let src = "public class T { static int sign(int n) { return n > 0 ? 1 : n < 0 ? -1 : 0; } public static int run() { return sign(-4) * 10 + sign(0) + sign(9); } }";
    // sign(-4)=-1 → -10, sign(0)=0, sign(9)=1 → -10+0+1 = -9
    assert_eq!(run_int(src, "run"), -9);
}
#[test]
fn feat_ternary_in_expression() {
    let src = "public class T { public static int run() { int a = 4; int b = 9; int m = (a > b ? a : b) + 1; return m; } }";
    assert_eq!(run_int(src, "run"), 10);
}
#[test]
fn feat_ternary_double_arms() {
    // Mixed numeric arms join to double; the int arm is coerced.
    let src =
        "public class T { public static double run() { boolean c = false; return c ? 1 : 2.5; } }";
    assert_eq!(run_double(src, "run"), 2.5);
}

// ── double / float arithmetic, mixed-type coercion, and comparisons ──────────
#[test]
fn feat_double_arithmetic() {
    let src = "public class T { public static double run() { double a = 1.5; double b = 2.0; return a * b; } }";
    assert_eq!(run_double(src, "run"), 3.0);
}
#[test]
fn feat_int_double_mixed() {
    let src = "public class T { public static double run() { int n = 3; double d = 0.5; return n + d; } }";
    assert_eq!(run_double(src, "run"), 3.5);
}
#[test]
fn feat_double_division() {
    let src = "public class T { public static double run() { return 7.0 / 2.0; } }";
    assert_eq!(run_double(src, "run"), 3.5);
}
#[test]
fn feat_float_compare() {
    let src = "public class T { public static int run() { float a = 1.5f; float b = 2.5f; if (a < b) { return 1; } return 0; } }";
    assert_eq!(run_int(src, "run"), 1);
}
#[test]
fn feat_double_compare_ge() {
    let src = "public class T { public static int run() { double a = 3.0; double b = 3.0; if (a >= b) { return 7; } return 0; } }";
    assert_eq!(run_int(src, "run"), 7);
}
#[test]
fn feat_double_array() {
    let src = "public class T { public static double run() { double[] a = new double[3]; a[0] = 1.25; a[1] = 2.25; a[2] = 0.5; double s = 0.0; for (int i = 0; i < 3; i++) { s = s + a[i]; } return s; } }";
    assert_eq!(run_double(src, "run"), 4.0);
}
#[test]
fn feat_int_to_double_return() {
    // Returning an int expression from a double method must coerce (i2d).
    let src = "public class T { public static double run() { int x = 5; return x; } }";
    assert_eq!(run_double(src, "run"), 5.0);
}

// ── Implicit-`this`: bare field read/write and instance-method calls ─────────
// Unqualified `count` / `dbl()` must resolve to the current class's field /
// method without an explicit `this.`. Before this, the backend reported
// "undefined variable" / "unsupported method call" — the top compile-other bug.
#[test]
fn feat_bare_instance_field() {
    let src = "public class T { \
        int count; \
        T() { count = 5; } \
        int bump() { count = count + 1; return count; } \
        public static int run() { T t = new T(); t.bump(); return t.bump(); } }";
    // count 5 → bump 6 → bump 7
    assert_eq!(run_int(src, "run"), 7);
}
#[test]
fn feat_bare_static_field() {
    let src = "public class T { \
        static int total; \
        static void add(int n) { total = total + n; } \
        public static int run() { total = 0; add(3); add(4); return total; } }";
    assert_eq!(run_int(src, "run"), 7);
}
#[test]
fn feat_bare_instance_method_call() {
    let src = "public class T { \
        int v; \
        T(int x) { v = x; } \
        int dbl() { return v * 2; } \
        int quad() { return dbl() + dbl(); } \
        public static int run() { T t = new T(5); return t.quad(); } }";
    assert_eq!(run_int(src, "run"), 20);
}

// ── Multi-variable declarations: `int a = 1, b = 2, c;` ──────────────────────
// One declaration declares several variables sharing a type, each with its own
// optional initializer, all visible in the enclosing scope.
#[test]
fn feat_multi_var_decl() {
    let src = "public class T { public static int run() { int a = 1, b = 2, c = a + b; return a + b + c; } }";
    // a=1, b=2, c=3 → 6
    assert_eq!(run_int(src, "run"), 6);
}
#[test]
fn feat_multi_var_decl_uninit() {
    let src = "public class T { public static int run() { int a = 5, b; b = a * 2; return b; } }";
    assert_eq!(run_int(src, "run"), 10);
}

// ── Method overloads: resolve by arity, then by argument type ────────────────
// Before the (name -> Vec<sig>) re-keying, a name-only signature table let
// overloads clobber each other so calls bound to the wrong descriptor.
#[test]
fn feat_overload_resolution() {
    let src = "public class T { \
        static int f(int a) { return a + 1; } \
        static int f(int a, int b) { return a + b; } \
        static int g(int a) { return 100; } \
        static int g(String s) { return 200; } \
        public static int run() { return f(5) + f(2, 3) + g(7); } }";
    // f(5)=6 (arity 1), f(2,3)=5 (arity 2), g(7)=100 (int over String) → 111
    assert_eq!(run_int(src, "run"), 111);
}

#[test]
fn feat_awt_color_and_bufferedimage_roundtrip() {
    // Color(r,g,b).getRGB() packs 0xAARRGGBB; BufferedImage.setRGB/getRGB store
    // and retrieve it losslessly.
    let src = "import java.awt.Color; import java.awt.image.BufferedImage; \
        public class T { \
        public static int run() { \
            BufferedImage img = new BufferedImage(4, 4, BufferedImage.TYPE_INT_RGB); \
            int c = new Color(10, 20, 30).getRGB(); \
            img.setRGB(2, 3, c); \
            if (img.getRGB(2, 3) != c) { return -1; } \
            return ((c >> 16) & 255) * 10000 + ((c >> 8) & 255) * 100 + (c & 255); } }";
    // round-trips, and decodes to r=10,g=20,b=30 → 102030
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 102030);
}

#[test]
fn feat_graphics_draw_rect_outlines_the_rectangle() {
    // drawRect(0, 0, 4, 4) strokes x in 0..=4 and y in 0..=4, so the top edge and the
    // corner are painted and the interior is not: 1 + 2 = 3.
    let src = "import java.awt.Color; import java.awt.Graphics2D; \
        import java.awt.image.BufferedImage; \
        public class T { \
        public static int run() { \
            BufferedImage img = new BufferedImage(5, 5, BufferedImage.TYPE_INT_RGB); \
            Graphics2D g = img.createGraphics(); \
            g.setColor(new Color(255, 255, 255)); \
            g.drawRect(0, 0, 4, 4); \
            int edge = img.getRGB(2, 0) != 0 ? 1 : 0; \
            int corner = img.getRGB(4, 4) != 0 ? 2 : 0; \
            int inner = img.getRGB(2, 2) != 0 ? 4 : 0; \
            return edge + corner + inner; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 3);
}

#[test]
fn feat_field_access_through_erased_generic() {
    // `list.get(i)` erases to Object; reading a field off it must resolve against
    // the object's actual class and coerce to the field-ref's declared type
    // (here `(int) p.x` where x is a double) without corrupting adjacent locals.
    let src = "import java.util.ArrayList; import java.util.List; \
        public class T { \
        static class P { double x; double y; P(double x, double y) { this.x = x; this.y = y; } } \
        public static int run() { \
            List<P> ps = new ArrayList<>(); \
            ps.add(new P(3.9, 5.1)); \
            int x = (int) ps.get(0).x; \
            int y = (int) ps.get(0).y; \
            return x * 10 + y; } }";
    // (int)3.9=3, (int)5.1=5 → 35
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 35);
}

#[test]
fn feat_erased_generic_field_access_emits_checkcast_and_exact_fieldref() {
    let src = "import java.util.ArrayList; import java.util.List; \
        public class T { \
        static class P { int x; P(int x) { this.x = x; } } \
        public static int run() { \
            List<P> ps = new ArrayList<>(); ps.add(new P(42)); return ps.get(0).x; } }";
    let producer = JavaBytecodeProducer::new();
    producer.parse(src, "T.java").expect("parse");
    let bytes = producer
        .produce_all_classes()
        .expect("every class of the unit compiles")
        .into_iter()
        .find_map(|(name, bytes)| (name == "T").then_some(bytes))
        .expect("compile T with its nested-class registry");
    let class = cafebabe::parse_class(&bytes).expect("parse generated class");
    // The backend currently emits nested source classes as standalone classes
    // under their simple names (see `convert_compilation_unit`).  What matters
    // for verifier correctness here is that CHECKCAST and the subsequent
    // Fieldref name that exact emitted class, never java/lang/Object.
    let expected_owner = "P";
    let mut saw_exact_checkcast = false;
    let mut checkcast_targets = Vec::new();
    let mut field_owners = Vec::new();
    for method in &class.methods {
        for attribute in &method.attributes {
            let cafebabe::attributes::AttributeData::Code(code) = &attribute.data else {
                continue;
            };
            let bytecode = code.bytecode.as_ref().expect("decoded bytecode");
            for (_, opcode) in &bytecode.opcodes {
                match opcode {
                    cafebabe::bytecode::Opcode::Checkcast(
                        cafebabe::constant_pool::ObjectArrayType::BinaryName(name),
                    ) => {
                        saw_exact_checkcast |= name.as_ref() == expected_owner;
                        checkcast_targets.push(name.as_ref().to_string());
                    }
                    cafebabe::bytecode::Opcode::Getfield(field)
                        if field.name_and_type.name.as_ref() == "x" =>
                    {
                        field_owners.push(field.class_name.as_ref().to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    assert!(
        saw_exact_checkcast,
        "generic List.get result needs CHECKCAST {expected_owner}, got {checkcast_targets:?}"
    );
    assert_eq!(field_owners, [expected_owner]);
}

#[test]
fn feat_map_generic_value_field_access() {
    let src = "import java.util.HashMap; import java.util.Map; \
        public class T { \
        static class P { int x; P(int x) { this.x = x; } } \
        public static int run() { \
            Map<String, P> values = new HashMap<>(); \
            values.put(\"answer\", new P(42)); return values.get(\"answer\").x; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 42);
}

#[test]
fn feat_iterator_generic_element_field_access() {
    let src = "import java.util.ArrayList; import java.util.Iterator; import java.util.List; \
        public class T { \
        static class P { int x; P(int x) { this.x = x; } } \
        public static int run() { \
            List<P> values = new ArrayList<>(); values.add(new P(42)); \
            Iterator<P> iterator = values.iterator(); return iterator.next().x; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 42);
}

#[test]
fn feat_enhanced_for_generic_element_field_access() {
    let src = "import java.util.ArrayList; import java.util.List; \
        public class T { \
        static class P { int x; P(int x) { this.x = x; } } \
        public static int run() { \
            List<P> values = new ArrayList<>(); values.add(new P(40)); values.add(new P(2)); \
            int sum = 0; for (P value : values) { sum += value.x; } return sum; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 42);
}

#[test]
fn unsupported_user_generic_field_chain_fails_during_compilation() {
    let src = "public class T { \
        static class P { int x; } \
        static class Box<U> { U value; Box(U value) { this.value = value; } U get() { return value; } } \
        public static int run() { Box<P> box = new Box<>(new P()); return box.get().x; } }";
    let producer = JavaBytecodeProducer::new();
    producer.parse(src, "T.java").expect("parse");
    let error = producer
        .produce_bytes(&CompilationContext {
            source: Some(src.to_string()),
            source_path: Some("T.java".to_string()),
            ..Default::default()
        })
        .expect_err("unsupported generic chains must not emit Object.x Fieldrefs");
    let message = format!("{error:?}");
    assert!(
        message.contains("cannot resolve instance field") && message.contains(".x"),
        "unexpected compile error: {message}"
    );
}

#[test]
fn feat_array_param_followed_by_param() {
    // An array-typed parameter *followed by* another parameter must keep its
    // array dimension (descriptor `([II)I`, not `(II)I`). Before the fix the
    // first type node won and the array dim was dropped, mis-typing the method
    // and dropping the whole class on conversion.
    let src = "public class T { \
        static int at(int[] a, int i) { return a[i]; } \
        public static int run() { int[] xs = {10, 20, 30}; return at(xs, 2); } }";
    assert_eq!(run_int(src, "run"), 30);
}

#[test]
fn feat_contextual_keyword_param_name() {
    // `to` is a contextual keyword (lexed as TO_KW) but a legal identifier in a
    // parameter position. The converter must accept it as a name, not error out
    // (which previously dropped the entire enclosing class).
    let src = "public class T { \
        static int span(int from, int to) { return to - from; } \
        public static int run() { return span(3, 10); } }";
    assert_eq!(run_int(src, "run"), 7);
}

#[test]
fn feat_constructor_this_delegation() {
    // `this(...)` must chain to the matching constructor (and suppress the
    // implicit `super()`), coercing args to the target's declared types.
    let src = "public class T { \
        int v; \
        T() { this(42); } \
        T(int x) { this.v = x; } \
        public static int run() { return new T().v; } }";
    assert_eq!(run_int(src, "run"), 42);
}

#[test]
fn feat_stringbuilder_append_char() {
    // append(char) must append the character, not its int code: building "AB"
    // by char must yield length 2 (the int-code bug produced "6566", length 4).
    let src = "public class T { \
        public static int run() { \
            StringBuilder sb = new StringBuilder(); \
            sb.append('A').append('B'); \
            return sb.length() * 1000 + sb.charAt(0); } }";
    // length 2 → 2000, charAt(0)='A'=65 → 2065
    assert_eq!(run_int(src, "run"), 2065);
}

#[test]
fn feat_stringbuilder_chained_append() {
    // Chained `append(...).append(...)` requires inferring the first append's
    // return type (StringBuilder) so the second call's methodref names the right
    // owner. A blank owner previously dispatched wrong and recursed (StackOverflow).
    let src = "public class T { \
        public static int run() { \
            return new StringBuilder().append(\"ab\").append(\"cd\").length(); } }";
    assert_eq!(run_int(src, "run"), 4);
}

#[test]
fn feat_stringbuilder_reverse_and_seed() {
    // `new StringBuilder(String)` seeds content; reverse() mutates in place and
    // returns the builder so it chains.
    let src = "public class T { \
        public static int run() { \
            return new StringBuilder(\"abc\").reverse().charAt(0); } }";
    // reversed "abc" is "cba"; charAt(0) = 'c' = 99
    assert_eq!(run_int(src, "run"), 99);
}

#[test]
fn feat_foreach_over_arraylist() {
    // Enhanced-for over a List iterates via iterator()/hasNext()/next(); summing
    // the (primitive-typed) elements exercises the collection-iterator lowering.
    let src = "import java.util.ArrayList; import java.util.List; \
        public class T { \
        public static int run() { \
            List<Integer> xs = new ArrayList<>(); \
            xs.add(10); xs.add(20); xs.add(30); \
            int sum = 0; \
            for (int x : xs) { sum += x; } \
            return sum; } }";
    assert_eq!(run_int(src, "run"), 60);
}

#[test]
fn feat_collection_size_in_value_position() {
    // `list.size()` must infer int, not Void — otherwise the `()V` descriptor
    // corrupts the stack when the result is used (e.g. as a return/argument).
    let src = "import java.util.ArrayList; \
        public class T { \
        public static int run() { \
            ArrayList<String> xs = new ArrayList<>(); \
            xs.add(\"a\"); xs.add(\"b\"); \
            return xs.size() + (xs.isEmpty() ? 100 : 5); } }";
    assert_eq!(run_int(src, "run"), 7);
}

#[test]
fn feat_instance_method_call_on_field() {
    // `field.method(args)` via implicit `this` must compile to an instance call
    // on the field's value, not `invokestatic field.method` (which named the
    // variable as the owner class and failed/recursed at runtime). Here `buf` is
    // a StringBuilder field; building "ab" through it must yield length 2.
    let src = "public class T { \
        StringBuilder buf; \
        T() { this.buf = new StringBuilder(); } \
        int build() { buf.append('a'); buf.append('b'); return buf.length(); } \
        public static int run() { return new T().build(); } }";
    assert_eq!(run_int(src, "run"), 2);
}

#[test]
fn feat_entry_point_main_in_later_class() {
    // When a helper class is declared *before* the entry class, `entry_point`
    // must still find `main` (it scans all classes, not just the first/primary).
    let src = "class Helper { static int twice(int x) { return x * 2; } } \
        public class Main { public static void main(String[] args) {} \
        static int run() { return Helper.twice(21); } }";
    let producer = JavaBytecodeProducer::new();
    producer.parse(src, "__Eval__.java").expect("parse");
    let (class, method, _desc) = producer.entry_point().expect("entry point");
    assert_eq!((class.as_str(), method.as_str()), ("Main", "main"));
}

#[test]
fn try_with_resources_closes_the_resource() {
    let src = r#"
public class T {
    static class R implements AutoCloseable {
        static int closes = 0;
        public void close() { closes = closes + 1; }
    }
    public static int run() {
        try (R r = new R()) {
            int x = 1;
        }
        return R.closes;
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

#[test]
fn try_with_resources_closes_the_resource_when_the_body_returns() {
    let src = r#"
public class T {
    static class R implements AutoCloseable {
        static int closes = 0;
        public void close() { closes = closes + 1; }
    }
    static int body() {
        try (R r = new R()) {
            return 5;
        }
    }
    public static int run() {
        int x = body();
        return x * 10 + R.closes;
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 51);
}

#[test]
fn try_with_resources_does_not_close_a_null_resource() {
    let src = r#"
public class T {
    static class R implements AutoCloseable {
        public void close() { throw new IllegalStateException("closed"); }
    }
    public static int run() {
        try (R r = null) {
            return 7;
        }
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 7);
}

#[test]
fn a_failing_close_is_suppressed_on_the_body_exception() {
    let src = r#"
public class T {
    static class R implements AutoCloseable {
        public void close() { throw new IllegalStateException("close"); }
    }
    public static int run() {
        try (R r = new R()) {
            throw new RuntimeException("body");
        } catch (RuntimeException e) {
            return e.getSuppressed().length;
        }
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

#[test]
fn finally_runs_when_the_try_returns() {
    let src = r#"
public class T {
    static int closed = 0;
    static int body() {
        try {
            return 5;
        } finally {
            closed = closed + 1;
        }
    }
    public static int run() {
        int x = body();
        return x * 10 + closed;
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 51);
}

#[test]
fn finally_runs_when_a_loop_breaks_out_of_its_try() {
    let src = r#"
public class T {
    public static int run() {
        int n = 0;
        for (int i = 0; i < 3; i++) {
            try {
                if (i == 1) {
                    break;
                }
                n = n + 10;
            } finally {
                n = n + 1;
            }
        }
        return n;
    }
}
"#;
    // i = 0: 10, then the finally makes 11. i = 1: break, the finally makes 12.
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 12);
}

#[test]
fn finally_runs_when_a_loop_continues_out_of_its_try() {
    let src = r#"
public class T {
    public static int run() {
        int n = 0;
        for (int i = 0; i < 3; i++) {
            try {
                if (i == 1) {
                    continue;
                }
                n = n + 10;
            } finally {
                n = n + 1;
            }
        }
        return n;
    }
}
"#;
    // i = 0: 11. i = 1: continue, the finally makes 12. i = 2: 22, then the finally makes 23.
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 23);
}

#[test]
fn finally_runs_when_a_switch_continues_the_loop_out_of_its_try() {
    let src = r#"
public class T {
    public static int run() {
        int n = 0;
        for (int i = 0; i < 2; i++) {
            try {
                switch (i) {
                    case 0:
                        continue;
                    default:
                        n = n + 10;
                }
            } finally {
                n = n + 1;
            }
        }
        return n;
    }
}
"#;
    // i = 0: continue, the finally makes 1. i = 1: 11, then the finally makes 12.
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 12);
}

#[test]
fn a_finally_that_throws_after_a_return_is_not_caught_by_its_own_catch() {
    let src = r#"
public class T {
    static int runs = 0;
    public static int run() {
        try {
            try {
                return 1;
            } catch (RuntimeException e) {
                return 2;
            } finally {
                runs = runs + 1;
                Integer.parseInt("x");
            }
        } catch (NumberFormatException e) {
            return 30 + runs;
        }
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 31);
}

#[test]
fn a_finally_that_throws_after_the_try_completes_runs_once() {
    let src = r#"
public class T {
    static int runs = 0;
    public static int run() {
        try {
            try {
                runs = runs + 1;
            } catch (RuntimeException e) {
                return -1;
            } finally {
                runs = runs + 10;
                Integer.parseInt("x");
            }
        } catch (NumberFormatException e) {
            return runs;
        }
        return 0;
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 11);
}

#[test]
fn a_finally_that_throws_after_a_break_is_caught_by_the_enclosing_try_only() {
    let src = r#"
public class T {
    public static int run() {
        int n = 0;
        for (int i = 0; i < 3; i++) {
            try {
                try {
                    if (i == 1) {
                        break;
                    }
                } catch (RuntimeException x) {
                    n = n + 100;
                } finally {
                    n = n + 1;
                    if (i == 1) {
                        Integer.parseInt("x");
                    }
                }
            } catch (NumberFormatException x) {
                n = n + 1000;
            }
        }
        return n;
    }
}
"#;
    // The exception from the finally abandons the break, so the loop runs i = 2 too.
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1003);
}

#[test]
fn try_with_resources_closes_once_when_close_throws_after_the_body() {
    let src = r#"
public class T {
    static class R implements AutoCloseable {
        static int closes = 0;
        public void close() {
            closes = closes + 1;
            throw new IllegalStateException("close");
        }
    }
    public static int run() {
        try {
            try (R r = new R()) {
                int x = 1;
            }
        } catch (IllegalStateException e) {
            return R.closes;
        }
        return -1;
    }
}
"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A method the backend has no return type for cannot have its value used. Before this was
/// refused, the value was dropped and the program failed with an internal stack underflow,
/// or with a NoSuchMethodError for a method that exists.
#[test]
fn value_of_a_method_without_a_known_return_type_is_refused() {
    let src = r#"
public class T {
    public static int run() {
        String s = "abc";
        return s.codePointAt(0);
    }
}"#;
    let Err(err) = run_static(src, "T", "run", "()I") else {
        panic!("the value of a method with no known return type must not compile");
    };
    assert!(err.contains("not known to the backend"), "{err}");
    assert!(err.contains("java.lang.String.codePointAt"), "{err}");
}

/// A nested class names a static field of the class that encloses it, by its simple name.
#[test]
fn nested_class_reads_and_assigns_a_static_field_of_its_enclosing_class() {
    let src = r#"
public class T {
    static int closed = 0;
    static class R {
        void close() { closed = closed + 1; }
    }
    public static int run() {
        R r = new R();
        r.close();
        r.close();
        return closed;
    }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 2);
}

/// The nested class's own static field shadows the enclosing class's field of the same name.
#[test]
fn nested_class_field_shadows_the_enclosing_classs_static_field() {
    let src = r#"
public class T {
    static int n = 10;
    static class R {
        static int n = 1;
        static int get() { return n; }
    }
    public static int run() {
        return R.get();
    }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A static field's initializer runs when the class is initialized.
#[test]
fn static_field_initializer_runs() {
    let src = r#"
public class T {
    static int n = 1;
    public static int run() { return n; }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// Static initializers and static blocks run in source order.
#[test]
fn static_initializers_run_in_source_order() {
    let src = r#"
public class T {
    static int n = 3;
    static { n = n + 1; }
    static int m = n + 4;
    public static int run() { return m; }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 8);
}

/// An instance initializer runs for the synthesized default constructor.
#[test]
fn instance_field_initializer_runs_in_the_default_constructor() {
    let src = r#"
public class T {
    int x = 5;
    public static int run() {
        T t = new T();
        return t.x;
    }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 5);
}

/// The initializer runs before the constructor body, and a parameter of the same name does not
/// receive it.
#[test]
fn instance_field_initializer_runs_before_the_constructor_body() {
    let src = r#"
public class T {
    int x = 5;
    T(int x) { this.x = x + 1; }
    public static int run() {
        return new T(10).x;
    }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 11);
}

/// A constructor that delegates with `this(...)` does not run the initializers again.
#[test]
fn delegating_constructor_does_not_rerun_instance_initializers() {
    let src = r#"
public class T {
    static int count = 0;
    static int bump() {
        count = count + 1;
        return count;
    }
    int x = bump();
    T() { }
    T(int unused) { this(); }
    public static int run() {
        new T(2);
        return count;
    }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A static block assigns a static field, and a static method then reads the assigned value.
#[test]
fn static_block_assignment_is_visible_to_a_static_method() {
    let src = r#"
public class T {
    static int n;
    static { n = 7; }
    public static int run() { return n; }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 7);
}

/// The message from refusing a unit: the producer returns an error, and nothing is compiled.
fn refused_message(source: &str) -> String {
    let producer = JavaBytecodeProducer::new();
    producer
        .parse(source, "T.java")
        .map_err(|e| format!("{e:?}"))
        .and_then(|_| {
            producer
                .produce_all_classes()
                .map(|_| ())
                .map_err(|e| format!("{e:?}"))
        })
        .expect_err("the unit must be refused")
}

const COLOR: &str = "public enum Color { RED, GREEN(2); private final int v; Color() { v = 0; } Color(int x) { v = x; } int get() { return v; } }\n";

/// Constants have their ordinals, and `values()` lists them in declaration order.
#[test]
fn enum_constants_have_their_ordinal_and_values() {
    let src = format!("{COLOR}public class T {{ public static int run() {{ return Color.GREEN.ordinal() * 10 + Color.values().length; }} }}");
    assert_eq!(run_static_multi(&src, "T", "run", "()I"), 12);
}

/// Constructor arguments reach the constant that names them.
#[test]
fn enum_constructor_arguments_reach_their_constant() {
    let src = format!("{COLOR}public class T {{ public static int run() {{ return Color.GREEN.get() + Color.RED.get(); }} }}");
    assert_eq!(run_static_multi(&src, "T", "run", "()I"), 2);
}

/// `valueOf` returns the named constant, and throws IllegalArgumentException for any other name.
#[test]
fn enum_value_of_returns_the_named_constant_and_throws_for_others() {
    let src = format!("{COLOR}public class T {{ public static int run() {{ int found = Color.valueOf(\"GREEN\").ordinal(); try {{ Color.valueOf(\"X\"); return 0; }} catch (IllegalArgumentException e) {{ return found + 7; }} }} }}");
    assert_eq!(run_static_multi(&src, "T", "run", "()I"), 8);
}

/// `name()` and `toString()` give the constant's name.
#[test]
fn enum_name_and_to_string_give_the_constant_name() {
    let src = format!("{COLOR}public class T {{ public static int run() {{ return (Color.RED.name().equals(\"RED\") ? 1 : 0) + (Color.GREEN.toString().equals(\"GREEN\") ? 2 : 0); }} }}");
    assert_eq!(run_static_multi(&src, "T", "run", "()I"), 3);
}

/// An enhanced for loop visits every constant.
#[test]
fn enum_constants_are_visited_by_an_enhanced_for_loop() {
    let src = format!("{COLOR}public class T {{ public static int run() {{ int s = 0; for (Color c : Color.values()) {{ s = s + c.get(); }} return s; }} }}");
    assert_eq!(run_static_multi(&src, "T", "run", "()I"), 2);
}

/// An instance initializer of an enum runs in its constructor.
#[test]
fn enum_instance_initializer_runs_in_the_constructor() {
    let src = "public enum E { A; int x = 5; int get() { return x; } } public class T { public static int run() { return E.A.get() + E.A.ordinal(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 5);
}

/// An enum nested in a class is compiled with its enclosing class.
#[test]
fn nested_enum_is_compiled_with_its_enclosing_class() {
    let src = "public class T { enum Color { RED, GREEN } public static int run() { return Color.GREEN.ordinal(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A constant with a body is an instance of a subclass of the enum, so its body overrides the enum's
/// methods.
#[test]
fn enum_constant_body_overrides_the_enum_method() {
    let src = "enum Op { PLUS { int apply(int a, int b) { return a + b; } }, MINUS { int apply(int a, int b) { return a - b; } }; int apply(int a, int b) { return 0; } }\npublic class T { public static int run() { return Op.PLUS.apply(5, 3) * 10 + Op.MINUS.apply(5, 3); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 82);
}

/// The arguments of a constant with a body reach the enum's constructor, and the body reads the field
/// that the constructor set.
#[test]
fn enum_constant_body_receives_its_constructor_arguments() {
    let src = "enum Level { LOW(1) { int weight() { return x * 10; } }, HIGH(2) { int weight() { return x * 100; } }; final int x; Level(int x) { this.x = x; } int weight() { return x; } }\npublic class T { public static int run() { return Level.LOW.weight() + Level.HIGH.weight(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 210);
}

/// A constant whose arguments match no constructor of the enum is refused, not compiled without them.
#[test]
fn enum_constant_body_without_a_matching_constructor_is_refused() {
    let message = refused_message(
        "public enum E { A(1) { int f() { return 1; } }; E() { } int f() { return 0; } }",
    );
    assert!(
        message.contains("no constructor of E takes 1 argument"),
        "{message}"
    );
}

/// A switch whose labels name enum constants selects the arm of each constant, falls through where
/// there is no break, and takes the default arm for the rest.
#[test]
fn switch_on_enum_constants_selects_the_arm_of_each_constant() {
    let src = r#"
public enum Color { RED, GREEN, BLUE }
public class T {
    public static int run() {
        int total = 0;
        for (Color c : Color.values()) {
            switch (c) {
                case RED: total = total + 1;
                case GREEN: total = total + 10; break;
                default: total = total + 100;
            }
        }
        return total;
    }
}"#;
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 121);
}

/// A name label on an int scrutinee is refused: it names no enum.
#[test]
fn a_name_label_on_a_non_enum_scrutinee_is_refused() {
    let message = refused_message(
        "public class T { public static int run() { int x = 1; switch (x) { case FOO: return 1; default: return 0; } } }",
    );
    assert!(message.contains("not an enum"), "{message}");
}

/// The result of `instanceof` is stored in a boolean local. Before this, the initializer was
/// dropped without a record, and the local was read unset.
#[test]
fn instanceof_into_a_boolean_local_is_the_test_result() {
    let src = "class A { }\npublic class T { public static int run() { A a = new A(); boolean b = a instanceof A; return b ? 1 : 0; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// An `instanceof` in a parenthesised condition is parsed and tests the operand.
#[test]
fn instanceof_in_a_parenthesised_condition_tests_the_operand() {
    let src = "class A { }\npublic class T { public static int run() { A a = new A(); return (a instanceof A) ? 1 : 0; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// `instanceof` is false for an object of another class, and for null.
#[test]
fn instanceof_is_false_for_another_class_and_for_null() {
    let src = "class A { }\nclass B { }\npublic class T { public static int run() { B b = new B(); A n = null; return ((b instanceof A) ? 10 : 0) + ((n instanceof A) ? 1 : 0); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 0);
}

/// `instanceof` on a JDK class reads the runtime class of the object.
#[test]
fn instanceof_on_a_jdk_class() {
    let src = "public class T { public static int run() { Object o = \"x\"; return (o instanceof String) ? 1 : 0; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A pattern binding in `instanceof` is refused, not compiled without the binding.
#[test]
fn instanceof_with_a_pattern_binding_is_refused() {
    let message = refused_message(
        "public class T { public static int run() { Object o = \"x\"; if (o instanceof String s) { return 1; } return 0; } }",
    );
    assert!(message.contains("pattern binding"), "{message}");
}

/// A class literal is refused, not dropped from the expression it appears in.
#[test]
fn class_literal_is_refused_not_dropped() {
    let message = refused_message(
        "public class T { public static int run() { Object c = String.class; return 0; } }",
    );
    assert!(message.contains("class literals"), "{message}");
}

/// `Math.max` of an `int` and a `long` is the `long` overload, so its result is a `long`.
#[test]
fn math_max_of_int_and_long_takes_the_promoted_overload() {
    let src = "public class T { public static int run() { long r = Math.max(3, 4L); return (int) (r * 10); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 40);
}

/// A user class cannot extend a JDK collection the interpreter represents natively: its instances
/// would not be collections, so it is refused, with the class named.
#[test]
fn a_class_extending_a_native_jdk_collection_is_refused() {
    let message = refused_message(
        "import java.util.ArrayList; class L extends ArrayList<String> { } \
         public class T { public static int run() { return 0; } }",
    );
    assert!(message.contains("java.util.ArrayList"), "{message}");
}

/// The refusal reaches a class that extends a unit class which extends a native collection.
#[test]
fn a_class_extending_a_native_collection_through_a_unit_class_is_refused() {
    let message = refused_message(
        "import java.util.HashMap; class A extends HashMap<String, String> { } \
         class B extends A { } \
         public class T { public static int run() { return 0; } }",
    );
    assert!(message.contains("java.util.HashMap"), "{message}");
}

/// A static call whose return type no source knows is refused. The backend used to guess one from
/// the first argument, and a wrong guess names a descriptor the method does not have.
#[test]
fn a_static_call_with_no_known_return_type_is_refused() {
    let message = refused_message(
        "public class T { public static int run() { Object o = Lib.compute(2); return 0; } }",
    );
    assert!(message.contains("Lib.compute"), "{message}");
}

/// A call through an interface type runs the implementing class's method.
#[test]
fn interface_method_is_dispatched_to_the_implementing_class() {
    let src = "interface Shape { int area(); }\nclass Sq implements Shape { public int area() { return 4; } }\npublic class T { public static int run() { Shape s = new Sq(); return s.area(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 4);
}

/// `instanceof` an interface is true for an implementing class.
#[test]
fn instanceof_an_interface_is_true_for_an_implementing_class() {
    let src = "interface Shape { int area(); }\nclass Sq implements Shape { public int area() { return 4; } }\npublic class T { public static int run() { Sq s = new Sq(); return (s instanceof Shape) ? 1 : 0; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A class inherits a default method of an interface it does not override.
#[test]
fn default_method_is_inherited_from_an_interface() {
    let src = "interface Shape { default int twice() { return 2; } }\nclass Sq implements Shape { }\npublic class T { public static int run() { Sq s = new Sq(); return s.twice(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 2);
}

/// An interface field is a static constant.
#[test]
fn interface_field_is_a_static_constant() {
    let src =
        "interface K { int N = 9; }\npublic class T { public static int run() { return K.N; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 9);
}

/// A method of an interface that an interface extends is dispatched through the subinterface.
#[test]
fn method_of_a_super_interface_is_dispatched_through_the_subinterface() {
    let src = "interface A { int f(); }\ninterface B extends A { }\nclass C implements B { public int f() { return 3; } }\npublic class T { public static int run() { A a = new C(); return a.f(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 3);
}

/// An enum implements an interface, and its inherited `ordinal()` is called on itself.
#[test]
fn enum_implementing_an_interface_calls_its_inherited_ordinal() {
    let src = "interface Named { int code(); }\nenum Color implements Named { RED, GREEN; public int code() { return ordinal() + 5; } }\npublic class T { public static int run() { Named n = Color.GREEN; return n.code(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 6);
}

/// An annotation type compiles, and an annotation of it on a class does not change the class.
#[test]
fn annotation_declaration_compiles_and_its_use_is_ignored() {
    let src = "public @interface Marker { int value() default 0; }\n@Marker(3) public class T { public static int run() { return 5; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 5);
}

/// A class that implements a JDK interface links, and `instanceof` that interface is true for it.
#[test]
fn class_implementing_a_jdk_interface_links_and_is_an_instance_of_it() {
    let src = "class A implements Runnable { public void run() { } }\npublic class T { public static int run() { A a = new A(); return (a instanceof Runnable) ? 1 : 0; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A call through a JDK interface type reaches the implementing class, as `invokeinterface` does.
#[test]
fn a_call_through_a_jdk_interface_type_reaches_the_implementation() {
    let src = "class A implements Runnable { int n; public void run() { n = 7; } }\n\
        public class T { public static int run() { A a = new A(); Runnable r = a; r.run(); return a.n; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 7);
}

/// A class that implements a generic interface at a specialised type is also called through the
/// interface's erased method, as javac's bridge method makes possible.
#[test]
fn a_specialised_generic_interface_method_is_called_through_the_erased_one() {
    let src = "class A implements Comparable<A> { int v; A(int v) { this.v = v; } \
        public int compareTo(A o) { return v - o.v; } }\n\
        public class T { public static int run() { Comparable<A> c = new A(5); return c.compareTo(new A(2)) * 10; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 30);
}

/// A method that overrides a generic superclass's method at a specialised type is reached through the
/// superclass's erased method, as javac's bridge makes possible.
#[test]
fn a_specialised_generic_superclass_method_is_called_through_the_erased_one() {
    let src = "class Box<T> { public void put(T t) { } }\n\
        class IntBox extends Box<Integer> { int last; public void put(Integer i) { last = i; } }\n\
        public class T { public static int run() { Box<Integer> b = new IntBox(); b.put(7); return ((IntBox) b).last; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 7);
}

/// A superclass's `Object` parameter that the subclass does not name as a type argument is a real
/// `Object`, so the subclass's overload is not bridged to it. A bridge would throw here.
#[test]
fn a_superclass_object_parameter_is_not_bridged_to_an_unrelated_overload() {
    let src = "class Base { public void f(Object o) { } }\n\
        class Sub extends Base { public void f(String s) { } }\n\
        public class T { public static int run() { Base b = new Sub(); b.f(new Object()); return 1; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A covariant return is bridged too: the interface's `Object make()` reaches `String make()`.
#[test]
fn a_covariant_return_is_called_through_the_interface_return_type() {
    let src = "interface Maker { Object make(); }\n\
        class M implements Maker { public String make() { return \"hi\"; } }\n\
        public class T { public static int run() { Maker m = new M(); Object o = m.make(); return ((String) o).length() * 10; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 20);
}

/// `equals(E)` overloads `Object.equals(Object)` in name only, so it gets no bridge: the inherited
/// `equals(Object)` still answers identity, and a different object is not equal.
#[test]
fn an_overload_of_a_superclass_method_is_not_bridged() {
    let src = "class E { public boolean equals(E other) { return true; } }\n\
        public class T { public static int run() { Object x = new E(); return x.equals(new Object()) ? 1 : 0; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 0);
}

/// An inherited `toString` on a user class has the JDK ancestor's `String` return type, so its
/// value is usable.
#[test]
fn an_inherited_tostring_on_a_user_class_has_a_usable_string_value() {
    let src = "class E { } public class T { public static int run() { E e = new E(); return e.toString().length() > 0 ? 1 : 0; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A class that does not override `equals` inherits identity, which answers `x.equals(x)` as true.
#[test]
fn an_inherited_equals_is_identity() {
    let src = "class E { } public class T { public static int run() { E x = new E(); return (x.equals(x) ? 1 : 0) + (x.equals(new E()) ? 10 : 0); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 1);
}

/// A primitive result and a reference argument through a JDK interface type. The raw `Comparable`
/// is used, so the implementing method's descriptor is the erased one and needs no bridge method.
#[test]
fn a_primitive_result_through_a_jdk_interface_type_is_typed_and_returned() {
    let src = "class A implements Comparable { public int compareTo(Object o) { return 4; } }\n\
        public class T { public static int run() { Comparable c = new A(); return c.compareTo(new A()) * 10; } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 40);
}

/// A JDK instance method the stdlib table does not type takes its return type from the stub
/// registry, so a value used from it compiles and runs. `repeat` returns a String.
#[test]
fn jdk_instance_return_type_comes_from_the_stub_registry() {
    let src = "public class T { public static int run() { String s = \"ab\"; return s.repeat(3).length(); } }";
    assert_eq!(run_static_multi(src, "T", "run", "()I"), 6);
}
