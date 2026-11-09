// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0
//! Java lambda EXECUTION tests — compile real Java with lambdas, run on the VM,
//! assert on results. These drive lambda execution.
//!
//! Strategy: a Java lambda lowers to a synthetic
//! closure class (extends `groovy/lang/Closure`) with a java-compiled `doCall`;
//! the functional-interface SAM call (`apply`/`test`/`get`/…) is routed to
//! `doCall` by the interpreter.

use hyperion_bytecode::interpreter::{MemoryConfig, Value, VirtualMachine};
use hyperion_bytecode::java_producer::JavaBytecodeProducer;
use hyperion_bytecode::producer::BytecodeProducer;

/// Compile a unit, load all of its classes (incl. synthetic lambda classes),
/// and invoke a static method on the primary class.
fn run(source: &str, class: &str, method: &str, descriptor: &str) -> Result<Value, String> {
    let producer = JavaBytecodeProducer::new();
    producer
        .parse(source, &format!("{class}.java"))
        .map_err(|e| format!("Parse failed: {e:?}"))?;
    let mut vm = VirtualMachine::new(MemoryConfig::default());
    let classes = producer
        .produce_all_classes()
        .map_err(|e| format!("generate classes: {e:?}"))?;
    let report = vm.load_classes_from_bytes(&classes);
    if !report.failures.is_empty() {
        return Err(format!("load generated classes: {:#?}", report.failures));
    }
    vm.invoke_static(class, method, descriptor, &[])
        .map_err(|e| format!("Execution failed: {e:?}"))
}

fn as_int(v: Value) -> i32 {
    match v {
        Value::Int(i) => i,
        other => panic!("expected int, got {other:?}"),
    }
}

fn as_long(v: Value) -> i64 {
    match v {
        Value::Long(value) => value,
        other => panic!("expected long, got {other:?}"),
    }
}

#[test]
fn lambda_function_apply() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.Function<Integer,Integer> f = x -> x * 2;
                return f.apply(5);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 10);
}

#[test]
fn lambda_supplier_get() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.Supplier<Integer> s = () -> 42;
                return s.get();
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 42);
}

#[test]
fn lambda_bifunction_apply() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.BiFunction<Integer,Integer,Integer> f = (a, b) -> a + b;
                return f.apply(3, 4);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 7);
}

#[test]
fn lambda_captures_local() {
    let src = r#"
        class L {
            static int run() {
                int n = 10;
                java.util.function.Function<Integer,Integer> f = x -> x + n;
                return f.apply(5);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 15);
}

#[test]
fn lambda_predicate_test() {
    let src = r#"
        class L {
            static boolean run() {
                java.util.function.Predicate<Integer> p = x -> x > 5;
                return p.test(10);
            }
        }
    "#;
    // boolean return surfaces as Int 0/1 at the VM boundary.
    assert_eq!(as_int(run(src, "L", "run", "()Z").expect("run")), 1);
}

#[test]
fn lambda_multi_capture() {
    let src = r#"
        class L {
            static int run() {
                int a = 3;
                int b = 4;
                java.util.function.Function<Integer,Integer> f = x -> x > a ? b : x;
                return f.apply(10);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 4);
}

#[test]
fn lambda_chained_operator_body() {
    // A chained binary operator in a lambda body must fold correctly.
    let src = r#"
        class L {
            static int run() {
                int a = 3;
                int b = 4;
                java.util.function.Function<Integer,Integer> f = x -> x + a + b;
                return f.apply(10);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 17);
}

#[test]
fn lambda_boolean_body() {
    let src = r#"
        class L {
            static boolean run() {
                java.util.function.Predicate<Integer> even = x -> x % 2 == 0;
                return even.test(8);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()Z").expect("run")), 1);
}

#[test]
fn lambda_runnable_void() {
    // A void-SAM lambda (Runnable.run) must dispatch without leaving a stray
    // value on the stack.
    let src = r#"
        class L {
            static int run() {
                Runnable r = () -> Math.max(1, 2);
                r.run();
                return 99;
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 99);
}

#[test]
fn lambda_bifunction_max() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.BiFunction<Integer,Integer,Integer> mx = (a, b) -> a > b ? a : b;
                return mx.apply(3, 7);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 7);
}

#[test]
fn lambda_multi_statement_block_executes() {
    let src = r#"
        class L {
            static int run() {
                int offset = 3;
                java.util.function.Function<Integer,Integer> f = x -> {
                    int doubled = x * 2;
                    int adjusted = doubled + offset;
                    return adjusted;
                };
                return f.apply(5);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 13);
}

#[test]
fn lambda_block_preserves_branching_returns() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.Function<Integer,Integer> abs = x -> {
                    if (x >= 0) return x;
                    return 0 - x;
                };
                return abs.apply(-7);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 7);
}

#[test]
fn exception_from_native_callback_unwinds_to_the_java_caller_once() {
    let src = r#"
        import java.util.ArrayList;
        import java.util.Comparator;
        import java.util.List;

        class L {
            static int run() {
                List<String> values = new ArrayList<>();
                values.add("b");
                values.add("a");
                IllegalArgumentException expected = new IllegalArgumentException();
                Comparator<String> exploding = (left, right) -> {
                    throw expected;
                };
                try {
                    values.sort(exploding);
                    return -1;
                } catch (IllegalArgumentException caught) {
                    return caught == expected ? 42 : -2;
                }
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 42);
}

#[test]
fn void_lambda_block_falls_through_cleanly() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.Consumer<Integer> consume = x -> {
                    Math.max(1, 2);
                    Math.min(3, 4);
                };
                consume.accept(5);
                return 99;
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 99);
}

#[test]
fn void_lambda_block_can_return_explicitly() {
    let src = r#"
        class L {
            static int run() {
                Runnable task = () -> { return; };
                task.run();
                return 42;
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 42);
}

#[test]
fn unary_operators_unbox_lambda_parameters() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.Function<Integer,Integer> negate = x -> -x;
                java.util.function.Function<Integer,Integer> complement = x -> ~x;
                return negate.apply(7) + complement.apply(3);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), -11);
}

#[test]
fn boolean_sam_arguments_keep_their_wrapper_type() {
    let src = r#"
        class L {
            static boolean run() {
                java.util.function.Predicate<Boolean> invert = value -> !value;
                return invert.test(false);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()Z").expect("run")), 1);
}

#[test]
fn functional_target_preserves_long_numeric_width() {
    let src = r#"
        class L {
            static long negate() {
                java.util.function.Function<Long,Long> f = value -> -value;
                return f.apply(3000000000L);
            }

            static long doubleIt() {
                java.util.function.Function<Long,Long> f = value -> value + value;
                return f.apply(3000000000L);
            }
        }
    "#;
    assert_eq!(
        as_long(run(src, "L", "negate", "()J").expect("negate")),
        -3_000_000_000
    );
    assert_eq!(
        as_long(run(src, "L", "doubleIt", "()J").expect("doubleIt")),
        6_000_000_000
    );
}

#[test]
fn unbound_instance_method_reference_executes() {
    let src = r#"
        import java.util.function.Function;

        class L {
            static int run() {
                Function<String,Integer> length = String::length;
                return length.apply("hyperion");
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 8);
}

#[test]
fn static_method_reference_executes() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.Function<String,Integer> parse = Integer::parseInt;
                return parse.apply("42");
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 42);
}

#[test]
fn primitive_static_method_reference_unboxes_parameters() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.BiFunction<Integer,Integer,Integer> max = Math::max;
                return max.apply(3, 7);
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 7);
}

#[test]
fn bound_method_reference_captures_receiver() {
    let src = r#"
        class L {
            static int run() {
                String value = "hyperion";
                java.util.function.Supplier<Integer> length = value::length;
                return length.get();
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 8);
}

#[test]
fn constructor_reference_executes() {
    let src = r#"
        class L {
            static int run() {
                java.util.function.Supplier<StringBuilder> factory = StringBuilder::new;
                StringBuilder builder = factory.get();
                builder.append("ok");
                return builder.length();
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 2);
}

/// A lambda implementing a user-defined functional interface is called through that interface: the
/// call reaches the lambda body, and not the interface's own method, which has no body.
#[test]
fn lambda_implementing_a_user_interface_is_called_through_it() {
    let src = r#"
        class L {
            interface G { Object g(); }
            static int run() {
                G h = () -> "x";
                Object o = h.g();
                return o == null ? 0 : 1;
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 1);
}

/// A lambda implementing a user-defined functional interface whose method returns a primitive: the
/// call takes the int, and not a box.
#[test]
fn lambda_implementing_a_user_interface_with_a_primitive_method() {
    let src = r#"
        class L {
            interface F { int f(); }
            static int run() {
                F g = () -> 7;
                return g.f() + 1;
            }
        }
    "#;
    assert_eq!(as_int(run(src, "L", "run", "()I").expect("run")), 8);
}
