// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java calling Kotlin: file facades, accessors, statics.
//!
//! Every fixture is compilable Java against compilable Kotlin, so any error
//! diagnostic on the Java file is a false positive.

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

use super::kotlin_clean_code_tests::project_diagnostics;

const SQUARE_KT: (&str, &str) = (
    "core/Square.kt",
    r#"
package demo.core

data class Square(val side: Double) {
    fun area(): Double = side * side
}

fun allShapes(): List<Square> = listOf(Square(1.0))

fun Square.doubledArea(): Double = area() * 2

val PI_ISH: Double = 3.14

var counter: Int = 0
"#,
);

fn error_messages(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .map(|d| {
            format!(
                "{}:{} {}",
                d.range.start.line + 1,
                d.range.start.character,
                d.message
            )
        })
        .collect()
}

async fn java_errors(kotlin: &[(&str, &str)], java: &str) -> Vec<String> {
    let mut files: Vec<(&str, &str)> = kotlin.to_vec();
    files.push(("app/App.java", java));
    let diagnostics = project_diagnostics(&files, "app/App.java").await;
    error_messages(&diagnostics)
}

#[tokio::test]
async fn java_calls_top_level_kotlin_functions_through_the_file_facade() {
    let errors = java_errors(
        &[SQUARE_KT],
        r#"
package app;

import demo.core.Square;
import demo.core.SquareKt;
import java.util.List;

public class App {
    public static void main(String[] args) {
        List<Square> all = SquareKt.allShapes();
        double twice = SquareKt.doubledArea(all.get(0));
        double pi = SquareKt.getPI_ISH();
        SquareKt.setCounter(SquareKt.getCounter() + 1);
    }
}
"#,
    )
    .await;
    assert!(errors.is_empty(), "false errors: {errors:#?}");
}

#[tokio::test]
async fn java_sees_kotlin_properties_as_accessors() {
    let errors = java_errors(
        &[SQUARE_KT],
        r#"
package app;

import demo.core.Square;

public class App {
    public static void main(String[] args) {
        Square sq = new Square(3.0);
        double side = sq.getSide();
        Square copy = sq.copy(4.0);
        double a = sq.component1() + copy.area();
    }
}
"#,
    )
    .await;
    assert!(errors.is_empty(), "false errors: {errors:#?}");
}

lsp_test_suite! {
    name: "Java to Kotlin navigation",

    test facade_class_import_goes_to_the_kotlin_file {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.SquareK<|>t;

                public class App {}
            ",
            "core/Square.kt" => r"
                package demo.core

                fun allShapes(): List<Int> = listOf(1)
            "
        },
        nav: "app/App.java", "<|>" => "Square.kt"
    }

    test facade_function_call_goes_to_the_kotlin_function {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.SquareKt;

                public class App {
                    void f() { SquareKt.allSh<|>apes(); }
                }
            ",
            "core/Square.kt" => r"
                package demo.core

                fun allShapes(): List<Int> = listOf(1)
            "
        },
        nav: "app/App.java", "<|>" => "Square.kt"
    }

    test getter_call_goes_to_the_kotlin_property {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.Square;

                public class App {
                    double f(Square sq) { return sq.getSi<|>de(); }
                }
            ",
            "core/Square.kt" => r"
                package demo.core

                data class Square(val side: Double)
            "
        },
        nav: "app/App.java", "<|>" => "Square.kt"
    }

    test companion_static_and_field_go_to_the_kotlin_members {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.Money;

                public class App {
                    Money f() { return Money.o<|>f(1); }
                }
            ",
            "core/Money.kt" => r"
                package demo.core

                class Money(val v: Int) {
                    companion object {
                        @JvmStatic
                        fun of(v: Int): Money = Money(v)
                    }
                }
            "
        },
        nav: "app/App.java", "<|>" => "Money.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin to Java navigation",

    test java_getter_is_a_kotlin_property {
        files: {
            "app/Use.kt" => r"
                package app

                import demo.core.Circle

                fun f(c: Circle): Double = c.rad<|>ius
            ",
            "core/Circle.java" => r"
                package demo.core;

                public class Circle {
                    private final double r;
                    public Circle(double r) { this.r = r; }
                    public double getRadius() { return r; }
                }
            "
        },
        nav: "app/Use.kt", "<|>" => "Circle.java"
    }

    test java_boolean_getter_is_a_kotlin_property {
        files: {
            "app/Use.kt" => r"
                package app

                import demo.core.Toggle

                fun f(t: Toggle): Boolean = t.isAct<|>ive
            ",
            "core/Toggle.java" => r"
                package demo.core;

                public class Toggle {
                    public boolean isActive() { return true; }
                }
            "
        },
        nav: "app/Use.kt", "<|>" => "Toggle.java"
    }
}

const MONEY_KT: (&str, &str) = (
    "core/Money.kt",
    r#"
package demo.core

@file:JvmName("MoneyKt")

class Money @JvmOverloads constructor(val amount: Int, val currency: String = "EUR") {
    companion object {
        @JvmField
        val ZERO = Money(0)

        const val LIMIT = 100

        @JvmStatic
        fun of(value: Int): Money = Money(value)

        fun eur(value: Int): Money = Money(value)
    }

    @JvmOverloads
    fun plus(other: Int, times: Int = 1): Money = Money(amount + other * times)
}

object Registry {
    @JvmStatic
    fun size(): Int = 0

    val name: String = "r"
}

enum class Kind { PRODUCT, SERVICE }

fun Money.doubled(): Money = Money(amount * 2)
"#,
);

#[tokio::test]
async fn java_uses_companion_object_static_field_and_enum_members_without_false_errors() {
    let errors = java_errors(
        &[MONEY_KT],
        r#"
package app;

import demo.core.Kind;
import demo.core.Money;
import demo.core.MoneyKt;
import demo.core.Registry;

public class App {
    void f() {
        Money zero = Money.ZERO;
        int limit = Money.LIMIT;
        Money a = Money.of(1);
        Money b = Money.Companion.eur(2);
        Money c = Money.Companion.of(3);
        Money d = a.plus(1);
        Money e = a.plus(1, 2);
        Money g = new Money(1);
        Money h = new Money(1, "USD");
        int size = Registry.size();
        String name = Registry.INSTANCE.getName();
        Kind[] kinds = Kind.values();
        Kind first = Kind.valueOf("PRODUCT");
        Money twice = MoneyKt.doubled(a);
    }
}
"#,
    )
    .await;
    assert!(errors.is_empty(), "false errors: {errors:#?}");
}

lsp_test_suite! {
    name: "Java to Kotlin JVM-view navigation",

    test jvm_field_goes_to_the_kotlin_property {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.Money;

                public class App {
                    Money f() { return Money.ZE<|>RO; }
                }
            ",
            "core/Money.kt" => r"
                package demo.core

                class Money(val amount: Int) {
                    companion object {
                        @JvmField
                        val ZERO = Money(0)
                    }
                }
            "
        },
        nav: "app/App.java", "<|>" => "Money.kt"
    }

    test companion_instance_method_goes_to_the_kotlin_function {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.Money;

                public class App {
                    Money f() { return Money.Companion.e<|>ur(2); }
                }
            ",
            "core/Money.kt" => r"
                package demo.core

                class Money(val amount: Int) {
                    companion object {
                        fun eur(value: Int): Money = Money(value)
                    }
                }
            "
        },
        nav: "app/App.java", "<|>" => "Money.kt"
    }
}

// ---------------------------------------------------------------------------
// References and rename across the language boundary
// ---------------------------------------------------------------------------

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{Location, WorkspaceEdit};

const INVOICE_KT: &str = r"
package demo.billing

class Invoice(val id: String) {
    var no<|>te: String? = null
    val paid: Boolean = false
}

fun label(inv: Invoice): String = inv.note ?: inv.id
";

const APP_JAVA: &str = r"
package demo.billing;

public class App {
    String f(Invoice inv) {
        inv.setNote(inv.getNote() + inv.getId());
        return inv.getNote();
    }
}
";

fn uri_ends_with(location: &Location, suffix: &str) -> bool {
    location.uri.as_str().ends_with(suffix)
}

/// Apply every edit of `edit` to the text of `path` (relative to the test root).
fn apply(test: &LspTest, edit: &WorkspaceEdit, path: &str, original: &str) -> String {
    let uri = test.file_uri(path).expect("file uri");
    let mut edits = edit
        .changes
        .as_ref()
        .and_then(|changes| changes.get(&uri))
        .cloned()
        .unwrap_or_default();
    edits
        .sort_by_key(|edit| std::cmp::Reverse((edit.range.start.line, edit.range.start.character)));
    let mut lines: Vec<String> = original.split('\n').map(str::to_string).collect();
    for edit in edits {
        assert_eq!(
            edit.range.start.line, edit.range.end.line,
            "single-line edits only"
        );
        let line = &mut lines[edit.range.start.line as usize];
        let chars: Vec<char> = line.chars().collect();
        let before: String = chars[..edit.range.start.character as usize]
            .iter()
            .collect();
        let after: String = chars[edit.range.end.character as usize..].iter().collect();
        *line = format!("{before}{}{after}", edit.new_text);
    }
    lines.join("\n")
}

#[test]
fn references_of_a_kotlin_property_include_java_accessor_calls() {
    let mut test = LspTest::new();
    test.add_file("demo/billing/Invoice.kt", INVOICE_KT);
    test.add_file("demo/billing/App.java", APP_JAVA);
    let refs = test
        .find_references("demo/billing/Invoice.kt", "<|>", true)
        .expect("references");
    let java = refs.iter().filter(|l| uri_ends_with(l, "App.java")).count();
    assert_eq!(java, 3, "getNote x2 + setNote x1 in Java: {refs:#?}");
    assert!(
        refs.iter().any(|l| uri_ends_with(l, "Invoice.kt")),
        "Kotlin usages must remain: {refs:#?}"
    );
}

#[test]
fn references_of_a_java_getter_call_include_the_kotlin_property() {
    let mut test = LspTest::new();
    test.add_file("demo/billing/Invoice.kt", &INVOICE_KT.replace("<|>", ""));
    test.add_file(
        "demo/billing/App.java",
        &APP_JAVA.replace(
            "inv.getNote() + inv.getId()",
            "inv.get<|>Note() + inv.getId()",
        ),
    );
    let refs = test
        .find_references("demo/billing/App.java", "<|>", true)
        .expect("references");
    assert!(
        refs.iter().any(|l| uri_ends_with(l, "Invoice.kt")),
        "the Kotlin property and its Kotlin uses belong to the Java getter's references: {refs:#?}"
    );
}

#[test]
fn renaming_a_kotlin_property_renames_java_accessor_calls() {
    let mut test = LspTest::new();
    test.add_file("demo/billing/Invoice.kt", INVOICE_KT);
    test.add_file("demo/billing/App.java", APP_JAVA);
    let edit = test
        .rename("demo/billing/Invoice.kt", "<|>", "remark")
        .expect("rename")
        .expect("edit");
    let kotlin = apply(
        &test,
        &edit,
        "demo/billing/Invoice.kt",
        &INVOICE_KT.replace("<|>", ""),
    );
    let java = apply(&test, &edit, "demo/billing/App.java", APP_JAVA);
    assert!(kotlin.contains("var remark: String?"), "{kotlin}");
    assert!(kotlin.contains("inv.remark ?: inv.id"), "{kotlin}");
    assert!(
        java.contains("inv.setRemark(inv.getRemark() + inv.getId())"),
        "{java}"
    );
    assert!(java.contains("return inv.getRemark();"), "{java}");
    assert!(
        !java.contains("getNote") && !java.contains("setNote"),
        "{java}"
    );
}

#[test]
fn renaming_a_top_level_kotlin_function_renames_facade_calls() {
    let mut test = LspTest::new();
    test.add_file(
        "demo/Shapes.kt",
        "package demo\n\nfun all<|>Shapes(): List<Int> = listOf(1)\n",
    );
    test.add_file(
        "demo/App.java",
        "package demo;\n\npublic class App {\n    void f() { ShapesKt.allShapes(); }\n}\n",
    );
    let edit = test
        .rename("demo/Shapes.kt", "<|>", "everyShape")
        .expect("rename")
        .expect("edit");
    let java = apply(
        &test,
        &edit,
        "demo/App.java",
        "package demo;\n\npublic class App {\n    void f() { ShapesKt.allShapes(); }\n}\n",
    );
    assert!(java.contains("ShapesKt.everyShape()"), "{java}");
}

#[test]
fn renaming_from_a_java_accessor_call_is_refused_with_an_explanation() {
    let mut test = LspTest::new();
    test.add_file("demo/billing/Invoice.kt", &INVOICE_KT.replace("<|>", ""));
    test.add_file(
        "demo/billing/App.java",
        &APP_JAVA.replace(
            "inv.getNote() + inv.getId()",
            "inv.get<|>Note() + inv.getId()",
        ),
    );
    let result = test.rename("demo/billing/App.java", "<|>", "getRemark");
    let message = result.expect_err("must not partially rename");
    assert!(
        message.contains("rename the Kotlin declaration"),
        "{message}"
    );
}

lsp_test_suite! {
    name: "Java calling Kotlin instance methods",

    test instance_method_call_goes_to_the_kotlin_function {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.Money;

                public class App {
                    Money f(Money m) { return m.pl<|>us(2); }
                }
            ",
            "core/Money.kt" => r"
                package demo.core

                class Money(val amount: Int) {
                    fun plus(other: Int): Money = Money(amount + other)
                }
            "
        },
        nav: "app/App.java", "<|>" => "Money.kt"
    }
}

lsp_test_suite! {
    name: "Java reaching the companion object and generated members",

    test companion_field_goes_to_the_kotlin_companion {
        files: {
            "app/App.java" => r"
                package app;

                import demo.core.Money;

                public class App {
                    Object f() { return Money.Comp<|>anion; }
                }
            ",
            "core/Money.kt" => r"
                package demo.core

                class Money(val amount: Int) {
                    companion object {
                        fun eur(value: Int): Money = Money(value)
                    }
                }
            "
        },
        nav: "app/App.java", "<|>" => "Money.kt"
    }

    test data_class_copy_goes_to_the_kotlin_class {
        files: {
            "app/App.java" => r#"
                package app;

                import demo.core.Item;

                public class App {
                    Item f(Item i) { return i.co<|>py("x", 2); }
                }
            "#,
            "core/Item.kt" => r"
                package demo.core

                data class Item(val name: String, val qty: Int)
            "
        },
        nav: "app/App.java", "<|>" => "Item.kt"
    }
}
