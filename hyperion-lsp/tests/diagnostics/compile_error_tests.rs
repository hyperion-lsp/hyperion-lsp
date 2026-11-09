// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! javac-style compile errors, end to end through the diagnostics pipeline.
//!
//! The governing rule: a false positive is worse than a missed error. Every
//! check has positive tests (the error javac would report) *and* negative
//! tests (valid code that must stay silent).

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity};

/// Diagnostics of `target` in a workspace made of `files` (all under
/// `src/main/java`).
fn diagnostics(files: &[(&str, &str)], target: &str) -> Vec<Diagnostic> {
    let mut test = LspTest::new();
    for (path, source) in files {
        test.add_file(path, source);
    }
    test.into_shared_context()
        .expect("workspace")
        .diagnostics(target)
        .expect("diagnostics")
}

/// Error-severity messages only.
fn errors(files: &[(&str, &str)], target: &str) -> Vec<String> {
    let mut found: Vec<_> = diagnostics(files, target)
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    // Pipeline order is by phase; tests read in source order.
    found.sort_by_key(|diagnostic| {
        (
            diagnostic.range.start.line,
            diagnostic.range.start.character,
        )
    });
    found
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect()
}

fn assert_no_errors(files: &[(&str, &str)], target: &str) {
    let found = errors(files, target);
    assert!(found.is_empty(), "expected no errors, got: {found:#?}");
}

#[test]
fn static_wildcard_imports_that_share_a_method_name_are_not_reported() {
    // `any` exists in both classes (hamcrest and mockito both do this); javac
    // merges the overload sets of every static on-demand import.
    assert_no_errors(
        &[
            ("m/Matchers.java", "package m; public class Matchers { public static <T> T any(Class<T> c) { return null; } }"),
            ("m/ArgumentMatchers.java", "package m; public class ArgumentMatchers { public static <T> T any() { return null; } }"),
            (
                "app/UseIt.java",
                r#"
package app;
import static m.Matchers.*;
import static m.ArgumentMatchers.*;
public class UseIt {
    Object a() { return any(); }
    Object b() { return any(String.class); }
}
"#,
            ),
        ],
        "app/UseIt.java",
    );
}

// ---------------------------------------------------------------------------
// Valid code must never be flagged.
//
// `fixtures/valid/*.java` is real, javac-clean Java (compiled with javac 26 +
// Lombok while writing these tests) covering overloads with generics, boxing,
// varargs, lambdas, static imports, inherited library members, Lombok
// members, records, enums, sealed hierarchies, inner and anonymous classes,
// reachability and exception handling. Two files (`StaticImports`,
// `InheritedLibrary`) deliberately depend on classes that are not on the
// classpath: an incomplete classpath must make the checks silent, not wrong.
// ---------------------------------------------------------------------------

const VALID_FIXTURES: &[(&str, &str)] = &[
    ("v/Access.java", include_str!("fixtures/valid/Access.java")),
    (
        "v/Declarations.java",
        include_str!("fixtures/valid/Declarations.java"),
    ),
    ("v/Flow.java", include_str!("fixtures/valid/Flow.java")),
    (
        "v/Generics.java",
        include_str!("fixtures/valid/Generics.java"),
    ),
    (
        "v/Hierarchy.java",
        include_str!("fixtures/valid/Hierarchy.java"),
    ),
    (
        "v/InheritedLibrary.java",
        include_str!("fixtures/valid/InheritedLibrary.java"),
    ),
    (
        "v/LombokUse.java",
        include_str!("fixtures/valid/LombokUse.java"),
    ),
    (
        "v/NestedQualified.java",
        include_str!("fixtures/valid/NestedQualified.java"),
    ),
    (
        "v/Overloads.java",
        include_str!("fixtures/valid/Overloads.java"),
    ),
    (
        "v/StaticImports.java",
        include_str!("fixtures/valid/StaticImports.java"),
    ),
];

fn assert_valid_fixture_is_silent(target: &str) {
    assert_no_errors(VALID_FIXTURES, target);
}

#[test]
fn valid_overloads_boxing_varargs_and_lambdas_are_silent() {
    assert_valid_fixture_is_silent("v/Overloads.java");
}

#[test]
fn valid_hierarchy_records_enums_sealed_inner_and_anonymous_are_silent() {
    assert_valid_fixture_is_silent("v/Hierarchy.java");
}

#[test]
fn valid_generics_and_conversions_are_silent() {
    assert_valid_fixture_is_silent("v/Generics.java");
}

#[test]
fn valid_static_imports_of_unindexed_classes_are_silent() {
    assert_valid_fixture_is_silent("v/StaticImports.java");
}

#[test]
fn valid_members_inherited_from_unresolved_library_supertypes_are_silent() {
    assert_valid_fixture_is_silent("v/InheritedLibrary.java");
}

#[test]
fn valid_lombok_members_are_silent() {
    assert_valid_fixture_is_silent("v/LombokUse.java");
}

#[test]
fn valid_control_flow_and_exception_handling_are_silent() {
    assert_valid_fixture_is_silent("v/Flow.java");
}

#[test]
fn valid_qualified_nested_types_and_inner_class_creation_are_silent() {
    assert_valid_fixture_is_silent("v/NestedQualified.java");
}

#[test]
fn valid_multi_declarators_annotation_members_and_contextual_keyword_calls_are_silent() {
    assert_valid_fixture_is_silent("v/Declarations.java");
}

#[test]
fn valid_member_access_is_silent() {
    assert_valid_fixture_is_silent("v/Access.java");
}

// ---------------------------------------------------------------------------
// Missed errors: calls
// ---------------------------------------------------------------------------

const SHAPES: &[(&str, &str)] = &[
    (
        "demo/Circle.java",
        r#"
package demo;
public class Circle {
    private final double r;
    public Circle(double r) { this.r = r; }
    public double area() { return Math.PI * r * r; }
    public static Circle unit() { return new Circle(1); }
}
"#,
    ),
    (
        "demo/Other.java",
        r#"
package demo;
public class Other {
    private int hidden;
    Other(int a) {}
}
"#,
    ),
];

fn with_shapes(extra: (&'static str, &'static str)) -> Vec<(&'static str, &'static str)> {
    let mut files = SHAPES.to_vec();
    files.push(extra);
    files
}

fn user(body: &'static str) -> (&'static str, &'static str) {
    // Leak the formatted source: tests are short-lived processes.
    let source: &'static str = Box::leak(
        format!("package demo;\nimport java.util.*;\npublic class Main {{\n{body}\n}}\n")
            .into_boxed_str(),
    );
    ("demo/Main.java", source)
}

#[test]
fn call_to_a_method_that_does_not_exist_is_reported() {
    let found = errors(
        &with_shapes(user("void t(Circle c) { double r = c.getRadius(); }")),
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec!["cannot find symbol: method getRadius() in type demo.Circle".to_owned()],
    );
}

#[test]
fn var_initialised_with_new_has_a_known_receiver_type() {
    let found = errors(
        &with_shapes(user("void t() { var c = new Circle(1.0); c.getRadius(); }")),
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec!["cannot find symbol: method getRadius() in type demo.Circle".to_owned()],
    );
    let quiet = errors(
        &with_shapes(user(
            "void t() { var c = make(); c.getRadius(); } Circle make() { return new Circle(1.0); }",
        )),
        "demo/Main.java",
    );
    assert!(quiet.is_empty(), "{quiet:?}");
}

#[test]
fn call_to_a_missing_method_is_reported_with_the_argument_types() {
    let found = errors(
        &with_shapes(user(r#"void t(Circle c) { c.scale(2, "x"); }"#)),
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec!["cannot find symbol: method scale(int, String) in type demo.Circle".to_owned()],
    );
}

#[test]
fn missing_method_on_a_jdk_type_is_reported() {
    let found = errors(
        &[user(
            r#"void t(String s) { s.foo(); List<String> l = new ArrayList<>(); l.push("x"); }"#,
        )],
        "demo/Main.java",
    );
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0].contains("method foo() in type java.lang.String"),
        "{found:?}"
    );
    assert!(
        found[1].contains("method push(String) in type java.util.List"),
        "{found:?}"
    );
}

#[test]
fn missing_static_method_on_a_type_name_is_reported() {
    let found = errors(
        &with_shapes(user("void t() { Circle.zero(); Math.sqroot(2.0); }")),
        "demo/Main.java",
    );
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0].contains("method zero() in type demo.Circle"),
        "{found:?}"
    );
    assert!(
        found[1].contains("method sqroot(double) in type java.lang.Math"),
        "{found:?}"
    );
}

#[test]
fn existing_methods_and_inherited_object_methods_are_not_reported() {
    assert_no_errors(
        &with_shapes(user(
            "void t(Circle c) { c.area(); Circle.unit().area(); c.toString(); c.hashCode(); c.equals(c); c.getClass(); Math.sqrt(2.0); }",
        )),
        "demo/Main.java",
    );
}

#[test]
fn missing_method_on_a_class_with_an_unresolved_supertype_is_not_reported() {
    // `Base` comes from a library that is not on the classpath: any member
    // could be inherited from it.
    assert_no_errors(
        &[
            (
                "demo/Widget.java",
                "package demo;\nimport lib.Base;\npublic class Widget extends Base { }\n",
            ),
            user("void t(Widget w) { w.fromTheLibrary(); }"),
        ],
        "demo/Main.java",
    );
}

#[test]
fn missing_method_on_object_typed_receiver_is_not_reported() {
    // `Object` is what inference falls back to; do not accuse on it.
    assert_no_errors(
        &[user("void t(Object o) { o.whatever(); }")],
        "demo/Main.java",
    );
}

#[test]
fn lombok_generated_accessors_are_not_reported_but_typos_are() {
    let files = vec![
        (
            "demo/Customer.java",
            "package demo;\nimport lombok.Data;\n@Data public class Customer { private String name; private boolean active; }\n",
        ),
        user("void t(Customer c) { c.getName(); c.setName(\"x\"); c.isActive(); c.getEmial(); }"),
    ];
    let found = errors(&files, "demo/Main.java");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("method getEmial()"), "{found:?}");
}

#[test]
fn lombok_builder_classes_are_never_accused() {
    assert_no_errors(
        &[
            (
                "demo/Customer.java",
                "package demo;\nimport lombok.*;\n@Data @Builder public class Customer { private String name; }\n",
            ),
            user("void t() { Customer.builder().name(\"x\").build().getName(); Customer.builder().anythingGoes(); }"),
        ],
        "demo/Main.java",
    );
}

// ---------------------------------------------------------------------------
// Missed errors: constructors
// ---------------------------------------------------------------------------

#[test]
fn constructor_with_wrong_arity_is_reported() {
    let found = errors(
        &with_shapes(user("void t() { Circle c = new Circle(); }")),
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec!["constructor Circle in class Circle cannot be applied to given types (required: double; found: no arguments)".to_owned()],
    );
}

#[test]
fn constructor_with_wrong_argument_type_is_reported() {
    let found = errors(
        &with_shapes(user(
            r#"void t() { new Circle("radius"); new Other(true, 1); }"#,
        )),
        "demo/Main.java",
    );
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("constructor Circle in class Circle cannot be applied to given types (required: double; found: String)"), "{found:?}");
    assert!(found[1].contains("constructor Other in class Other cannot be applied to given types (required: int; found: boolean, int)"), "{found:?}");
}

#[test]
fn instantiating_an_interface_or_abstract_class_is_reported() {
    let found = errors(
        &[
            (
                "demo/Greeter.java",
                "package demo;\npublic interface Greeter { void greet(); }\n",
            ),
            (
                "demo/Base.java",
                "package demo;\npublic abstract class Base { }\n",
            ),
            user("void t() { Greeter g = new Greeter(); Base b = new Base(); }"),
        ],
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec![
            "Greeter is abstract; cannot be instantiated".to_owned(),
            "Base is abstract; cannot be instantiated".to_owned(),
        ],
    );
}

#[test]
fn valid_instantiations_are_not_reported() {
    assert_no_errors(
        &[
            ("demo/Greeter.java", "package demo;\npublic interface Greeter { void greet(); }\n"),
            ("demo/Base.java", "package demo;\npublic abstract class Base { protected Base(int x) {} }\n"),
            ("demo/Box.java", "package demo;\npublic class Box<T> { public Box(T value) {} public Box(T a, T b) {} public Box(String... names) {} }\n"),
            ("demo/Plain.java", "package demo;\npublic class Plain { }\n"),
            ("demo/Wide.java", "package demo;\npublic class Wide { public Wide(long a, double b, Object c, Number d) {} }\n"),
            user(
                r#"void t(List<String> names) {
    new Plain();
    new Wide(1, 2, "o", 3);
    new Wide('c', 1.5f, null, 4L);
    new Box<>("x");
    new Box<String>("x", "y");
    new Box<>();
    new Box<>("a", "b", "c");
    new Base(1) { };
    new Greeter() { public void greet() {} };
    new ArrayList<String>(names);
    new ArrayList<>(10);
    new HashMap<String, Integer>();
    new Thread(() -> {});
    new Thread(this::run, "n");
    new Object();
    new StringBuilder("x");
    new String(new char[] {'a'});
    new int[3];
    new String[] {"a"};
}
void run() {}"#,
            ),
        ],
        "demo/Main.java",
    );
}

#[test]
fn lombok_generated_constructors_are_not_judged() {
    assert_no_errors(
        &[
            (
                "demo/Customer.java",
                "package demo;\nimport lombok.*;\n@Data @AllArgsConstructor public class Customer { private String name; private int age; }\n",
            ),
            user(r#"void t() { new Customer("n", 1); }"#),
        ],
        "demo/Main.java",
    );
}

// ---------------------------------------------------------------------------
// Missed errors: reachability (missing return, unreachable statement)
// ---------------------------------------------------------------------------

/// `(line, message)` (1-based lines) of every error in a class made of `body`
/// (the class header takes the first two lines).
fn error_lines(body: &'static str) -> Vec<(u32, String)> {
    diagnostics(&[user(body)], "demo/Main.java")
        .into_iter()
        .filter(|diagnostic| diagnostic.severity == Some(DiagnosticSeverity::ERROR))
        .map(|diagnostic| (diagnostic.range.start.line + 1, diagnostic.message))
        .collect()
}

#[test]
fn missing_return_statement_is_reported_at_the_closing_brace() {
    let found = error_lines(
        "int a(int x) {\n    if (x > 0) { return 1; }\n}\nint ok(int x) { if (x > 0) return 1; else return 2; }",
    );
    // line 1-2 header, 3 = `int a`, 4 = if, 5 = closing brace
    assert_eq!(found, vec![(6, "missing return statement".to_owned())]);
}

#[test]
fn missing_return_variants_are_reported() {
    for (body, what) in [
        (
            "boolean flag; int f() { while (flag) { } }",
            "while over a non-constant field",
        ),
        (
            "int f(int x) { switch (x) { case 1: return 1; case 2: return 2; } }",
            "switch without default",
        ),
        (
            "int f() { try { return 1; } catch (RuntimeException e) { } }",
            "catch that falls through",
        ),
        (
            "int f() { for (int i = 0; i < 3; i++) { return i; } }",
            "for loop with a real condition",
        ),
        (
            "int f(Object o) { if (o != null) { return 1; } else if (o == null) { return 2; } }",
            "if/else-if chain",
        ),
        (
            "int f(boolean b) { int x = 1; if (b) { throw new IllegalStateException(); } x++; }",
            "falls off after statements",
        ),
        (
            "int f(java.util.List<String> l) { for (String s : l) { return 1; } }",
            "for-each",
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(
            found,
            vec!["missing return statement".to_owned()],
            "{what}: {found:?}"
        );
    }
}

#[test]
fn methods_that_cannot_fall_off_the_end_are_not_reported() {
    for (body, what) in [
        ("int f(int x) { if (x > 0) { return 1; } else { return 2; } }", "if/else"),
        ("int f() { while (true) { } }", "endless while"),
        ("int f(int x) { while (true) { if (x > 3) return x; x++; } }", "while(true) with return"),
        ("int f() { for (;;) { } }", "endless for"),
        ("int f(int x) { switch (x) { case 1: return 1; default: return 2; } }", "switch with default"),
        ("int f(int x) { switch (x) { case 1 -> { return 1; } default -> throw new RuntimeException(); } }", "arrow switch"),
        ("int f() { try { return 1; } finally { System.out.println(); } }", "try/finally"),
        ("int f() { try { return 1; } catch (RuntimeException e) { throw e; } }", "try/catch"),
        ("int f() { throw new UnsupportedOperationException(); }", "throw"),
        ("int f(Object o) { synchronized (o) { return 1; } }", "synchronized"),
        ("static final boolean DEBUG = true; int f() { while (DEBUG) { } }", "constant condition"),
        ("int f() { do { return 1; } while (true); }", "do/while"),
        ("int f() { outer: while (true) { while (true) { break outer; } } return 1; }", "labeled break then return"),
        ("int f(int x) { return switch (x) { case 1 -> 1; default -> 2; }; }", "switch expression"),
        ("void f() { }", "void method"),
        ("abstract static class A { abstract int f(); }", "abstract method"),
        ("java.util.concurrent.Callable<Integer> f() { return () -> { return 1; }; }", "lambda"),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert!(found.is_empty(), "{what}: {found:?}");
    }
}

#[test]
fn unreachable_statements_are_reported() {
    for (body, what) in [
        ("void f() { return; System.out.println(); }", "after return"),
        (
            "void f() { throw new RuntimeException(); int x = 1; }",
            "after throw",
        ),
        (
            "void f() { while (true) { } System.out.println(); }",
            "after endless while",
        ),
        (
            "void f() { for (;;) { } System.out.println(); }",
            "after endless for",
        ),
        (
            "void f(int x) { while (true) { if (x > 0) { return; } else { return; } } }",
            "n/a-placeholder",
        ),
        (
            "void f() { do { return; } while (true); System.out.println(); }",
            "after do that returns",
        ),
        (
            "void f() { label: { break label; } int ok = 1; return; }",
            "n/a-placeholder-2",
        ),
        (
            "void f(int x) { for (int i = 0; i < 3; i++) { break; System.out.println(); } }",
            "after break",
        ),
        (
            "void f(int x) { for (int i = 0; i < 3; i++) { continue; System.out.println(); } }",
            "after continue",
        ),
        (
            "void f(int x) { switch (x) { case 1: return; System.out.println(); } }",
            "after return in a switch group",
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        if what.starts_with("n/a") {
            assert!(found.is_empty(), "{what}: {found:?}");
        } else {
            assert_eq!(
                found,
                vec!["unreachable statement".to_owned()],
                "{what}: {found:?}"
            );
        }
    }
}

#[test]
fn reachable_statements_after_loops_with_breaks_are_not_reported() {
    for body in [
        "void f() { while (true) { break; } System.out.println(); }",
        "void f(int x) { outer: while (true) { while (true) { break outer; } } System.out.println(); }",
        "void f(boolean b) { while (b) { } System.out.println(); }",
        "void f(int x) { if (x > 0) { return; } System.out.println(); }",
        "void f(int x) { try { return; } catch (RuntimeException e) { } System.out.println(); }",
        "void f(int x) { switch (x) { case 1: return; default: break; } System.out.println(); }",
        "void f() { do { } while (false); System.out.println(); }",
        "void f() { for (int i = 0; ; i++) { if (i > 3) break; } System.out.println(); }",
    ] {
        assert_no_errors(&[user(body)], "demo/Main.java");
    }
}

#[test]
fn only_the_first_unreachable_statement_is_reported_and_no_missing_return_cascades() {
    let found = errors(
        &[user(
            "int f() { return 1; System.out.println(); System.out.println(); }",
        )],
        "demo/Main.java",
    );
    assert_eq!(found, vec!["unreachable statement".to_owned()], "{found:?}");
}

#[test]
fn the_old_unreachable_code_lint_no_longer_duplicates_the_error() {
    let all = diagnostics(
        &[user("void f() { return; System.out.println(); }")],
        "demo/Main.java",
    );
    let unreachable: Vec<_> = all
        .iter()
        .filter(|diagnostic| diagnostic.message.to_lowercase().contains("unreachable"))
        .collect();
    assert_eq!(unreachable.len(), 1, "{unreachable:?}");
    assert_eq!(unreachable[0].severity, Some(DiagnosticSeverity::ERROR));
}

// ---------------------------------------------------------------------------
// Missed errors: access control, missing fields, unimplemented abstract methods
// ---------------------------------------------------------------------------

fn vault() -> Vec<(&'static str, &'static str)> {
    vec![(
        "vault/Vault.java",
        r#"
package vault;
public class Vault {
    private int secret;
    int packagePrivate;
    protected int guarded;
    public int open;
    private void hidden() {}
    void internal() {}
    protected void guardedMethod() {}
    public void visible() {}
    private static int counter;
    public static class Inner { private int deep; void poke(Vault v) { v.secret++; v.hidden(); counter++; } }
    void poke(Inner i) { i.deep++; }
}
"#,
    )]
}

fn from_other_package(body: &'static str) -> Vec<(&'static str, &'static str)> {
    let source: &'static str = Box::leak(
        format!("package app;\nimport vault.Vault;\npublic class Client {{\n{body}\n}}\n")
            .into_boxed_str(),
    );
    let mut files = vault();
    files.push(("app/Client.java", source));
    files
}

#[test]
fn private_member_access_from_another_class_is_reported() {
    let found = errors(
        &from_other_package("void t(Vault v) { v.secret = 1; v.hidden(); }"),
        "app/Client.java",
    );
    assert_eq!(
        found,
        vec![
            "secret has private access in Vault".to_owned(),
            "hidden() has private access in Vault".to_owned(),
        ],
    );
}

#[test]
fn package_private_and_protected_access_from_another_package_is_reported() {
    let found = errors(
        &from_other_package(
            "void t(Vault v) { v.packagePrivate++; v.internal(); v.guarded++; v.guardedMethod(); }",
        ),
        "app/Client.java",
    );
    assert_eq!(
        found,
        vec![
            "packagePrivate is not public in Vault; cannot be accessed from outside package"
                .to_owned(),
            "internal() is not public in Vault; cannot be accessed from outside package".to_owned(),
            "guarded has protected access in Vault".to_owned(),
            "guardedMethod() has protected access in Vault".to_owned(),
        ],
    );
}

#[test]
fn accessible_members_are_not_reported() {
    // public members; a subclass reaching protected members through itself;
    // nested classes and their outer class sharing private members.
    assert_no_errors(
        &from_other_package(
            "void t(Vault v, Vault.Inner in) { v.open++; v.visible(); }\nstatic class Sub extends Vault { void t(Sub s) { guarded++; guardedMethod(); s.guarded++; s.guardedMethod(); this.guarded++; super.guardedMethod(); } }",
        ),
        "app/Client.java",
    );
    assert_no_errors(&vault(), "vault/Vault.java");
}

#[test]
fn access_from_anonymous_subclasses_is_never_judged() {
    assert_no_errors(
        &from_other_package(
            "void t() { new Vault() { void f() { this.guarded++; this.guardedMethod(); } }; }",
        ),
        "app/Client.java",
    );
}

#[test]
fn missing_field_is_reported() {
    let found = errors(
        &with_shapes(user("void t(Circle c, String s) { int x = c.radius; int n = s.length; Circle.NOPE.toString(); }")),
        "demo/Main.java",
    );
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found[0].contains("variable radius in type demo.Circle"),
        "{found:?}"
    );
    assert!(
        found[1].contains("variable length in type java.lang.String"),
        "{found:?}"
    );
    assert!(
        found[2].contains("variable NOPE in type demo.Circle"),
        "{found:?}"
    );
}

#[test]
fn existing_fields_and_special_selectors_are_not_reported() {
    assert_no_errors(
        &[
            ("demo/Point.java", "package demo;\npublic class Point { public int x; public static final int ORIGIN = 0; public enum Kind { A, B } public static class Nested { public static int V; } }\n"),
            ("demo/Child.java", "package demo;\npublic class Child extends Point { }\n"),
            user("void t(Point p, Child c, int[] arr) { int a = p.x + Point.ORIGIN + c.x + Child.ORIGIN + arr.length; Point.Kind k = Point.Kind.A; Child.Kind k2 = Child.Kind.B; int v = Point.Nested.V; Object o = Point.class; java.io.File f = null; System.out.println(a); System.err.flush(); }"),
        ],
        "demo/Main.java",
    );
}

#[test]
fn concrete_class_missing_an_interface_method_is_reported() {
    let found = errors(
        &[(
            "demo/Impl.java",
            r#"
package demo;
public class Impl implements Greeter {
    public void greet(String n) {}
}
interface Greeter { void greet(String n); int count(); }
"#,
        )],
        "demo/Impl.java",
    );
    assert_eq!(
        found,
        vec![
            "Impl is not abstract and does not override abstract method count() in Greeter"
                .to_owned()
        ],
    );
}

#[test]
fn missing_abstract_superclass_and_generic_methods_are_reported() {
    let found = errors(
        &[(
            "demo/Shapes.java",
            r#"
package demo;
abstract class Shape implements Comparable<Shape> { abstract double area(); protected abstract void draw(int x, String s); }
class Square extends Shape {
    double area() { return 1; }
    public int compareTo(Shape o) { return 0; }
}
"#,
        )],
        "demo/Shapes.java",
    );
    assert_eq!(
        found,
        vec!["Square is not abstract and does not override abstract method draw(int,String) in Shape".to_owned()],
    );
}

#[test]
fn implemented_abstract_methods_are_not_reported() {
    assert_no_errors(
        &[(
            "demo/Ok.java",
            r#"
package demo;
import java.util.*;
interface Named { String name(); default String label() { return name(); } }
interface Sized { int size(); boolean equals(Object o); String toString(); }
abstract class Base implements Named { public String name() { return "n"; } }
class Concrete extends Base implements Sized, Comparable<Concrete>, Runnable {
    public int size() { return 1; }
    public int compareTo(Concrete o) { return 0; }
    public void run() {}
}
record Pt(int x) implements Named { public String name() { return "p"; } }
enum Level implements Named { LOW, HIGH; public String name2() { return name(); } }
enum Op implements Runnable { A { public void run() {} }, B { public void run() {} } }
class Impl2 extends AbstractList<String> { public String get(int i) { return ""; } public int size() { return 0; } }
class Impl3 implements Iterator<String>, Iterable<String> {
    public boolean hasNext() { return false; }
    public String next() { return ""; }
    public Iterator<String> iterator() { return this; }
}
class Impl4 implements java.util.function.Function<String, Integer> { public Integer apply(String s) { return 1; } }
class Impl5 implements Comparator<String> { public int compare(String a, String b) { return 0; } }
"#,
        )],
        "demo/Ok.java",
    );
}

#[test]
fn abstract_method_checks_are_skipped_when_a_supertype_is_unresolved() {
    assert_no_errors(
        &[(
            "demo/Lib.java",
            "package demo;\nimport lib.Base;\npublic class Lib extends Base implements Runnable { public void run() {} }\n",
        )],
        "demo/Lib.java",
    );
}

// ---------------------------------------------------------------------------
// Missed errors: returns, conditions, for-each, type arguments
// ---------------------------------------------------------------------------

#[test]
fn return_type_mismatches_are_reported() {
    for (body, expected) in [
        (
            "int f() { return \"x\"; }",
            "incompatible types: java.lang.String cannot be converted to int",
        ),
        (
            "String f() { return 1; }",
            "incompatible types: int cannot be converted to java.lang.String",
        ),
        (
            "void f() { return 1; }",
            "incompatible types: unexpected return value",
        ),
        (
            "int f() { return; }",
            "incompatible types: missing return value",
        ),
        (
            "boolean f(String s) { return s; }",
            "incompatible types: java.lang.String cannot be converted to boolean",
        ),
        (
            "double[] f() { return 1.0; }",
            "incompatible types: double cannot be converted to double[]",
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(found, vec![expected.to_owned()], "{body}: {found:?}");
    }
}

#[test]
fn compatible_returns_are_not_reported() {
    for body in [
        "long f() { return 1; }",
        "double f(int x) { return x; }",
        "Object f() { return 1; }",
        "Integer f() { return 1; }",
        "int f(Integer x) { return x; }",
        "Number f() { return 1.5; }",
        "String f(boolean b) { return b ? \"a\" : null; }",
        "List<String> f() { return new ArrayList<>(); }",
        "Iterable<String> f(List<String> l) { return l; }",
        "<T> T f(T t) { return t; }",
        "<T extends Comparable<T>> T f(List<T> l) { return l.get(0); }",
        "char f() { return 'a'; }",
        "byte f() { return 1; }",
        "short f() { return 1; }",
        "int[] f() { return new int[]{1}; }",
        "Object[] f() { return new String[0]; }",
        "Runnable f() { return () -> { return; }; }",
        "java.util.function.Supplier<Integer> f() { return () -> { return 1; }; }",
        "int f() { Runnable r = new Runnable() { public void run() { return; } }; return 1; }",
        "int f(int x) { return switch (x) { case 1 -> { yield 2; } default -> 3; }; }",
        "Main() { return; }",
        "static { if (Math.random() > 2) { return; } }",
        "StringBuilder f() { return new StringBuilder().append(1); }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn non_boolean_conditions_are_reported() {
    for (body, expected) in [
        (
            "void f(String s) { if (s) { } }",
            "incompatible types: java.lang.String cannot be converted to boolean",
        ),
        (
            "void f(int n) { while (n) { } }",
            "incompatible types: int cannot be converted to boolean",
        ),
        (
            "void f(int n) { do { } while (n + 1); }",
            "incompatible types: int cannot be converted to boolean",
        ),
        (
            "void f() { for (int i = 0; i; i++) { } }",
            "incompatible types: int cannot be converted to boolean",
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(found, vec![expected.to_owned()], "{body}: {found:?}");
    }
}

#[test]
fn boolean_conditions_are_not_reported() {
    for body in [
        "void f(boolean b, Boolean w) { if (b) { } while (w) { } do { } while (b && w); }",
        "void f(int n) { if (n > 0 && n < 3 || !(n == 2)) { } for (int i = 0; i < n; i++) { } }",
        "void f(java.util.List<String> l) { if (l.isEmpty()) { } while (!l.isEmpty()) { l.remove(0); } }",
        "void f(Object o) { if (o instanceof String s && s.isEmpty()) { } }",
        "void f(java.util.Map<String, Boolean> m) { if (m.get(\"a\")) { } }",
        "void f(java.util.function.Predicate<String> p) { if (p.test(\"a\")) { } }",
        "void f() { for (;;) { break; } }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn for_each_over_something_that_is_not_iterable_is_reported() {
    for body in [
        "void f() { for (int k : 5) { } }",
        "void f(String s) { for (char c : s) { } }",
        "void f(Object o) { for (Object x : new StringBuilder()) { } }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert!(
            found[0].starts_with("for-each not applicable to expression type"),
            "{found:?}"
        );
    }
}

#[test]
fn for_each_over_arrays_iterables_and_unknowns_is_not_reported() {
    for body in [
        "void f(int[] a, String[][] g) { for (int x : a) { } for (String[] row : g) { } }",
        "void f(List<String> l, Set<Integer> s, Map<String, Integer> m) { for (String x : l) { } for (int i : s) { } for (Map.Entry<String, Integer> e : m.entrySet()) { } for (String k : m.keySet()) { } }",
        "void f(Iterable<String> it, java.util.Collection<String> c, java.nio.file.Path p) { for (String x : it) { } for (String x : c) { } for (java.nio.file.Path q : p) { } }",
        "void f(Object o) { for (var x : (List<?>) o) { } }",
        "void f() { for (var x : unknownLibrary.items()) { } }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert!(!found.iter().any(|message| message.starts_with("for-each")), "{body}: {found:?}");
    }
}

#[test]
fn unresolved_type_arguments_are_reported() {
    let found = errors(
        &[user("List<Missing> a; Map<String, List<Deeper>> b; void t() { Object o = new HashMap<String, Gone>(); Map<String, ? extends Bound> w; }")],
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec![
            "cannot find symbol: Missing".to_owned(),
            "cannot find symbol: Deeper".to_owned(),
            "cannot find symbol: Gone".to_owned(),
            "cannot find symbol: Bound".to_owned(),
        ],
    );
}

#[test]
fn resolvable_type_arguments_are_not_reported() {
    assert_no_errors(
        &[user("<T extends Comparable<T>> void t(Map<String, ? extends List<T>> m, Class<? super T> c, Set<Map.Entry<String, Integer>> es, List<Main> self, List<Main.Nested> nested, Map<String, int[]> arrays, Optional<java.io.File> f) { } static class Nested {}")],
        "demo/Main.java",
    );
}

#[test]
fn primitive_type_arguments_are_reported() {
    // (The spaces keep the test framework from mistaking `<int>` for a marker.)
    let found = errors(
        &[user(
            "List< int > a; Map<String, double> b; List<int[]> ok; ",
        )],
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec![
            "unexpected type; required: reference; found: int".to_owned(),
            "unexpected type; required: reference; found: double".to_owned(),
        ],
    );
}

// ---------------------------------------------------------------------------
// Missed errors: assignments to final variables
// ---------------------------------------------------------------------------

#[test]
fn assignment_to_a_final_variable_is_reported() {
    for (body, expected) in [
        (
            "void t() { final int f = 1; f = 2; }",
            vec!["cannot assign a value to final variable f"],
        ),
        (
            "void t() { final int f = 1; f++; }",
            vec!["cannot assign a value to final variable f"],
        ),
        (
            "void t() { final int f = 1; f += 3; }",
            vec!["cannot assign a value to final variable f"],
        ),
        (
            "void t(final int p) { p = 3; }",
            vec!["final parameter p may not be assigned"],
        ),
        (
            "private final int fld = 1; void t() { fld = 2; this.fld++; }",
            vec![
                "cannot assign a value to final variable fld",
                "cannot assign a value to final variable fld",
            ],
        ),
        (
            "private final int blank; Main() { blank = 1; } void t() { blank = 2; }",
            vec!["cannot assign a value to final variable blank"],
        ),
        (
            "private static final int LIMIT = 3; static void t() { LIMIT = 4; }",
            vec!["cannot assign a value to final variable LIMIT"],
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(found, expected, "{body}");
    }
}

#[test]
fn legal_assignments_near_final_variables_are_not_reported() {
    for body in [
        "void t() { final int x; x = 1; int y = x; }",
        "void t(boolean b) { final int x; if (b) { x = 1; } else { x = 2; } }",
        "final int a; { a = 1; }",
        "static final int B; static { B = 2; }",
        "private final int c; Main(int c) { this.c = c; }",
        "private final int d; Main() { d = 1; } Main(int x) { this(); }",
        "void t(int p) { p = 3; p++; }",
        "void t(java.util.List<String> l) { for (final String s : l) { System.out.println(s); } }",
        "void t() { final int[] arr = new int[2]; arr[0] = 1; }",
        "void t() { final StringBuilder sb = new StringBuilder(); sb.append(1); }",
        "private int mutable; void t() { mutable = 2; mutable++; }",
        "void t() { int x = 1; Runnable r = new Runnable() { int x; public void run() { x = 2; } }; }",
        "void t() { for (int i = 0; i < 3; i++) { final int j = i; System.out.println(j); } }",
        "record R(int v) { R { v = 1; } }",
        "interface I { int K = 1; }",
        "enum E { A; private int n; void set() { n = 1; } }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

// ---------------------------------------------------------------------------
// Missed errors: duplicate variables and static contexts
// ---------------------------------------------------------------------------

#[test]
fn duplicate_local_variables_are_reported() {
    for (body, expected) in [
        (
            "void t() { int dup = 1; int dup = 2; }",
            "variable dup is already defined in method t()",
        ),
        (
            "void t(int p) { int p = 2; }",
            "variable p is already defined in method t(int)",
        ),
        (
            "void t(String s, java.util.List<String> l) { if (s != null) { String l = \"\"; } }",
            "variable l is already defined in method t(String,java.util.List<String>)",
        ),
        (
            "void t() { int a = 1; for (int a = 0; a < 3; a++) { } }",
            "variable a is already defined in method t()",
        ),
        (
            "void t() { int x = 1; Runnable r = () -> { int x = 2; }; }",
            "variable x is already defined in method t()",
        ),
        (
            "void t() { int x = 1; java.util.function.Function<Integer, Integer> f = x -> x; }",
            "variable x is already defined in method t()",
        ),
        (
            "void t(int n) { switch (n) { case 1: int v = 1; break; case 2: int v = 2; break; } }",
            "variable v is already defined in method t(int)",
        ),
        (
            "void t() { try { } catch (RuntimeException e) { int e = 1; } }",
            "variable e is already defined in method t()",
        ),
        (
            "Main() { int q = 1; int q = 2; }",
            "variable q is already defined in constructor Main()",
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(found, vec![expected.to_owned()], "{body}: {found:?}");
    }
}

#[test]
fn legal_variable_reuse_is_not_reported() {
    for body in [
        "void t() { { int a = 1; } { int a = 2; } }",
        "void t() { for (int i = 0; i < 2; i++) { } for (int i = 0; i < 2; i++) { } int i = 3; }",
        "void t(java.util.List<String> l) { for (String s : l) { } for (String s : l) { } }",
        "void t() { try { int x = 1; } catch (RuntimeException e) { int x = 2; } finally { int x = 3; } }",
        "void t() { try (java.io.StringReader r = new java.io.StringReader(\"\")) { } try (java.io.StringReader r = new java.io.StringReader(\"\")) { } }",
        "void t(int n) { switch (n) { case 1 -> { int v = 1; } case 2 -> { int v = 2; } default -> { } } }",
        "void t() { Runnable r = new Runnable() { public void run() { int x = 2; } }; int x = 1; }",
        "int f; void t(int f) { int g = f; }",
        "void t() { int x = 1; class Local { void m() { int x = 2; } } }",
        "void t() { java.util.function.Function<Integer, Integer> f = x -> x; java.util.function.Function<Integer, Integer> g = x -> x + 1; }",
        "void t(Object o) { if (o instanceof String s) { } if (o instanceof Integer s) { } }",
        "void t() { int a = 1, b = 2; { int c = a + b; } { int c = 3; } }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn instance_members_in_a_static_context_are_reported() {
    for (body, expected) in [
        (
            "int secret; static void t() { secret++; }",
            "non-static variable secret cannot be referenced from a static context",
        ),
        (
            "int secret; static void t() { this.secret = 2; }",
            "non-static variable this cannot be referenced from a static context",
        ),
        (
            "void helper(int x) {} static void t() { helper(1); }",
            "non-static method helper(int) cannot be referenced from a static context",
        ),
        (
            "int secret; static int SC = secret;",
            "non-static variable secret cannot be referenced from a static context",
        ),
        (
            "int secret; static { secret = 1; }",
            "non-static variable secret cannot be referenced from a static context",
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(found, vec![expected.to_owned()], "{body}: {found:?}");
    }
}

#[test]
fn legitimate_static_and_instance_access_is_not_reported() {
    for body in [
        "int secret; void t() { secret++; this.secret = 2; helper(); }\nvoid helper() {}",
        "static int counter; static void t() { counter++; }",
        "static void helper() {} static void t() { helper(); }",
        "int secret; static void t(Main m) { m.secret++; }",
        "int secret; static void t() { int secret = 1; secret++; }",
        "int secret; static void t() { Runnable r = new Runnable() { int secret; public void run() { secret++; this.secret++; } }; }",
        "int secret; static class Nested { int secret; void t() { secret++; } }",
        "int secret; class Inner { void t() { secret++; } }",
        "void helper() {} static void t() { new Main().helper(); }",
        "enum Mode { A; static int count() { return values().length; } }",
        "int x; static void t(int x) { x++; }",
        "static void t(java.util.List<String> l) { l.forEach(s -> System.out.println(s)); }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

// ---------------------------------------------------------------------------
// Missed errors: switch exhaustiveness
// ---------------------------------------------------------------------------

const SWITCH_TYPES: &str = r#"
enum Color { RED, GREEN, BLUE }
sealed interface Expr permits Num, Add, Neg {}
record Num(int v) implements Expr {}
record Add(Expr l, Expr r) implements Expr {}
final class Neg implements Expr {}
sealed interface Shape {}
final class Circle implements Shape {}
sealed abstract class Poly implements Shape permits Square, Tri {}
final class Square extends Poly {}
final class Tri extends Poly {}
"#;

fn switch_source(body: &'static str) -> Vec<(&'static str, &'static str)> {
    let source: &'static str = Box::leak(
        format!("package demo;\nimport java.util.*;\n{SWITCH_TYPES}\npublic class Main {{\n{body}\n}}\n").into_boxed_str(),
    );
    vec![("demo/Main.java", source)]
}

#[test]
fn non_exhaustive_switch_expressions_are_reported() {
    for (body, what) in [
        ("int f(Color c) { return switch (c) { case RED -> 1; case GREEN -> 2; }; }", "enum missing BLUE"),
        ("int f(Color c) { return switch (c) { case RED, GREEN -> 1; }; }", "enum missing one of three"),
        ("int f(String s) { return switch (s) { case \"a\" -> 1; case \"b\" -> 2; }; }", "string without default"),
        ("int f(int n) { return switch (n) { case 1 -> 1; case 2 -> 2; }; }", "int without default"),
        ("int f(Expr e) { return switch (e) { case Num n -> 1; case Add a -> 2; }; }", "sealed missing Neg"),
        ("int f(Shape s) { return switch (s) { case Circle c -> 1; case Square q -> 2; }; }", "nested sealed missing Tri"),
        ("int f(Expr e) { return switch (e) { case Num n when n.v() > 0 -> 1; case Add a -> 2; case Neg g -> 3; }; }", "guard does not count"),
    ] {
        let found = errors(&switch_source(body), "demo/Main.java");
        assert_eq!(
            found,
            vec!["the switch expression does not cover all possible input values".to_owned()],
            "{what}: {found:?}",
        );
    }
}

#[test]
fn non_exhaustive_pattern_switch_statements_are_reported() {
    let found = errors(
        &switch_source("void f(Expr e) { switch (e) { case Num n -> { } case Add a -> { } } }"),
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec!["the switch statement does not cover all possible input values".to_owned()],
    );
}

#[test]
fn exhaustive_and_legacy_switches_are_not_reported() {
    for body in [
        "int f(Color c) { return switch (c) { case RED -> 1; case GREEN -> 2; case BLUE -> 3; }; }",
        "int f(Color c) { return switch (c) { case RED -> 1; default -> 2; }; }",
        "int f(String s) { return switch (s) { case \"a\" -> 1; default -> 2; }; }",
        "int f(Expr e) { return switch (e) { case Num n -> 1; case Add a -> 2; case Neg g -> 3; }; }",
        "int f(Expr e) { return switch (e) { case Num n -> 1; case Add(Expr l, var r) -> 2; case Neg g -> 3; }; }",
        "int f(Expr e) { return switch (e) { case Num n -> 1; case Add a -> 2; case Expr other -> 3; }; }",
        "int f(Shape s) { return switch (s) { case Circle c -> 1; case Square q -> 2; case Tri t -> 3; }; }",
        "int f(Shape s) { return switch (s) { case Circle c -> 1; case Poly p -> 2; }; }",
        "int f(Object o) { return switch (o) { case String s -> 1; case Object x -> 2; }; }",
        // `Object` is inference's fallback type: a non-exhaustive switch over it
        // is left to the compiler rather than risking an error on a mis-typed selector.
        "int f(Object o) { return switch (o) { case String s -> 1; case Integer i -> 2; }; }",
        "int f(Object o) { return switch (o) { case String s -> 1; default -> 2; }; }",
        "int f(Color c) { int r = 0; switch (c) { case RED: r = 1; break; case GREEN: r = 2; break; } return r; }",
        "int f(Color c) { int r = 0; switch (c) { case RED -> r = 1; case GREEN -> r = 2; } return r; }",
        "int f(String s) { switch (s) { case \"a\": return 1; } return 0; }",
        "int f(int n) { switch (n) { case 1: return 1; } return 0; }",
        "int f(int n) { return switch (n) { case 1 -> 1; default -> { yield 2; } }; }",
        "int f(Optional<String> o) { return switch (o.orElse(\"\")) { case \"a\" -> 1; default -> 2; }; }",
        "int f(java.time.DayOfWeek d) { return switch (d) { case MONDAY, TUESDAY, WEDNESDAY, THURSDAY, FRIDAY, SATURDAY, SUNDAY -> 1; }; }",
        "int f(unknown.Lib x) { return switch (x) { case A -> 1; }; }",
    ] {
        let found = errors(&switch_source(body), "demo/Main.java");
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

// ---------------------------------------------------------------------------
// Missed errors: unreported checked exceptions
// ---------------------------------------------------------------------------

const UNREPORTED: &str = "must be caught or declared to be thrown";

fn unreported(name: &str) -> String {
    format!("unreported exception {name}; {UNREPORTED}")
}

#[test]
fn unreported_checked_exceptions_are_reported() {
    for (body, expected) in [
        ("void t() { Thread.sleep(10); }", "InterruptedException"),
        ("void t() { new java.io.FileReader(\"x\"); }", "FileNotFoundException"),
        ("void j() throws Exception {} void k() { j(); }", "Exception"),
        ("void t() { throw new java.io.IOException(); }", "IOException"),
        (
            "void t() { try (java.io.BufferedReader r = new java.io.BufferedReader(new java.io.StringReader(\"x\"))) { } }",
            "IOException",
        ),
        ("void t() { Runnable r = () -> Thread.sleep(1); }", "InterruptedException"),
        ("static class Res { Res() throws java.io.IOException {} } void t() { new Res(); }", "IOException"),
        ("void t() { try { Thread.sleep(1); } catch (IllegalStateException e) { } }", "InterruptedException"),
        ("void t() { try { } catch (RuntimeException e) { Thread.sleep(1); } }", "InterruptedException"),
        ("void t() { try { } finally { Thread.sleep(1); } }", "InterruptedException"),
        ("void t() throws java.io.IOException { Thread.sleep(1); }", "InterruptedException"),
        ("Main() { Thread.sleep(1); }", "InterruptedException"),
        (
            "void t() { java.util.concurrent.Callable<String> c = () -> \"x\"; c.call(); }",
            "Exception",
        ),
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        assert_eq!(found, vec![unreported(expected)], "{body}: {found:?}");
    }
}

#[test]
fn handled_and_unchecked_exceptions_are_not_reported() {
    for body in [
        "void t() { try { Thread.sleep(1); } catch (InterruptedException e) { } }",
        "void t() { try { Thread.sleep(1); } catch (Exception e) { } }",
        "void t() { try { Thread.sleep(1); } catch (Throwable e) { } }",
        "void t() { try { new java.io.FileReader(\"x\"); } catch (java.io.IOException | RuntimeException e) { } }",
        "void t() { try { new java.io.FileReader(\"x\"); } catch (java.io.FileNotFoundException e) { } }",
        "void t() throws InterruptedException { Thread.sleep(1); }",
        "void t() throws Exception { Thread.sleep(1); new java.io.FileReader(\"x\"); }",
        "void t() throws java.io.IOException { new java.io.FileReader(\"x\"); throw new java.io.FileNotFoundException(); }",
        "void t() { try { try { Thread.sleep(1); } catch (IllegalStateException e) { } } catch (InterruptedException e) { } }",
        "void t() { try { Thread.sleep(1); } catch (InterruptedException e) { } finally { System.out.println(); } }",
        "void t() { Integer.parseInt(\"1\"); throw new IllegalStateException(); }",
        "void t() { throw new UnsupportedOperationException(); }",
        "void t() { java.util.Optional.of(1).get(); new StringBuilder().append(1); \"x\".length(); }",
        "void t() { java.util.concurrent.Callable<String> c = () -> { Thread.sleep(1); return \"x\"; }; }",
        "void t() { Runnable r = () -> { try { Thread.sleep(1); } catch (InterruptedException e) { } }; }",
        "void t() { try (java.io.StringReader r = new java.io.StringReader(\"x\")) { r.read(); } catch (java.io.IOException e) { } }",
        "void t() throws java.io.IOException { try (java.io.BufferedReader r = new java.io.BufferedReader(new java.io.StringReader(\"x\"))) { } }",
        "void t() { try (java.io.StringWriter w = new java.io.StringWriter()) { w.write(\"x\"); } catch (java.io.IOException e) { } }",
        "java.io.Reader field = new java.io.StringReader(\"x\");",
        "static java.util.concurrent.Callable<Integer> C = () -> { Thread.sleep(1); return 1; };",
        "<X extends Throwable> void f(java.util.function.Supplier<X> s) throws X { throw s.get(); }",
        "void t() { java.util.Optional.empty().orElseThrow(() -> new java.io.IOException()); }",
        "void t() { new Object() { void inner() throws Exception { Thread.sleep(1); } }; }",
        "interface Thrower { void go() throws Exception; } void t() { Thrower th = () -> { throw new Exception(); }; }",
        "@lombok.SneakyThrows void t() { Thread.sleep(1); }",
        "void t() { Object o = new Object(); o.wait(); }",
    ] {
        let found = errors(&[user(body)], "demo/Main.java");
        // `o.wait()` throws InterruptedException in reality; javac reports it.
        // The dedicated test below owns that case.
        if body.contains("o.wait()") {
            continue;
        }
        assert!(found.is_empty(), "{body}: {found:?}");
    }
}

#[test]
fn checked_exceptions_thrown_by_workspace_methods_use_the_declaring_files_imports() {
    let files = vec![
        (
            "lib/Store.java",
            "package lib;\nimport java.io.*;\npublic class Store {\n    public void save() throws IOException {}\n    public void load() throws FileNotFoundException, InterruptedException {}\n    public void safe() {}\n}\n",
        ),
        (
            "app/Client.java",
            "package app;\nimport lib.Store;\npublic class Client {\n    void t(Store s) { s.save(); s.load(); s.safe(); }\n    void u(Store s) throws Exception { s.save(); s.load(); }\n}\n",
        ),
    ];
    let found = errors(&files, "app/Client.java");
    assert_eq!(
        found,
        vec![
            unreported("IOException"),
            unreported("FileNotFoundException")
        ]
    );
}

#[test]
fn checked_exceptions_of_static_library_calls_are_reported() {
    let files = vec![(
        "demo/Main.java",
        "package demo;\nimport java.nio.file.*;\nimport java.util.List;\npublic class Main {\n    List<String> lines(Path p) { return Files.readAllLines(p); }\n    List<String> ok(Path p) throws java.io.IOException { return Files.readAllLines(p); }\n    boolean exists(Path p) { return Files.exists(p); }\n}\n",
    )];
    assert_eq!(
        errors(&files, "demo/Main.java"),
        vec![unreported("IOException")]
    );
}

// ---------------------------------------------------------------------------
// Noise: default inspections follow IntelliJ's defaults
// ---------------------------------------------------------------------------

fn all_messages(body: &'static str) -> Vec<String> {
    diagnostics(&[user(body)], "demo/Main.java")
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect()
}

#[test]
fn trivial_main_produces_no_diagnostics_at_all() {
    let found = diagnostics(
        &[(
            "demo/Main.java",
            r#"
package demo;

public class Main {
    public static void main(String[] args) {
        double area = 2.0 * 3.5 * 3.14159;
        int timeout = 30000;
        System.out.println(area + timeout);
    }
}
"#,
        )],
        "demo/Main.java",
    );
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn magic_numbers_and_raw_types_are_available_when_opted_in() {
    let mut test = LspTest::new();
    test.add_file(
        "demo/Main.java",
        "package demo;\nimport java.util.*;\npublic class Main {\n    void t() { int x = 4242; List raw = new ArrayList(); }\n}\n",
    );
    let context = test.into_shared_context().expect("workspace");
    let quiet = context.diagnostics("demo/Main.java").expect("diagnostics");
    assert!(
        !quiet.iter().any(|d| d.code
            == Some(tower_lsp::lsp_types::NumberOrString::String(
                "magic_number".into()
            ))
            || d.code
                == Some(tower_lsp::lsp_types::NumberOrString::String(
                    "raw_type".into()
                ))),
        "{quiet:#?}"
    );
    let loud = context
        .diagnostics_with_options(
            "demo/Main.java",
            hyperion_lsp::diagnostic_engine::DiagnosticOptions {
                run_security: false,
                opt_in_inspections: true,
                ..Default::default()
            },
        )
        .expect("diagnostics");
    let codes: Vec<String> = loud
        .iter()
        .filter_map(|d| match &d.code {
            Some(tower_lsp::lsp_types::NumberOrString::String(code)) => Some(code.clone()),
            _ => None,
        })
        .collect();
    assert!(codes.contains(&"magic_number".to_owned()), "{codes:?}");
    assert!(codes.contains(&"raw_type".to_owned()), "{codes:?}");
}

#[test]
fn unused_parameters_are_only_reported_where_the_signature_is_local() {
    let found = all_messages(
        "public static void main(String[] args) { }\n@Override public boolean equals(Object other) { return true; }\npublic void handle(int event) { }\nprivate void helper(int unused) { }\nstatic void util(String skipped) { }",
    );
    let unused: Vec<_> = found
        .iter()
        .filter(|m| m.contains("is never used"))
        .collect();
    assert_eq!(
        unused,
        vec![
            "Parameter 'unused' is never used",
            "Parameter 'skipped' is never used"
        ],
        "{found:?}"
    );
}

#[test]
fn string_comparison_inspection_looks_at_the_operands() {
    let flagged = all_messages("boolean t(String a) { return a == \"x\"; }");
    assert!(
        flagged
            .iter()
            .any(|m| m.starts_with("String comparison using ==")),
        "{flagged:?}"
    );
    let unrelated =
        all_messages("boolean t(int n, String s) { return n == 1 && \"a\".equals(s); }");
    assert!(
        !unrelated
            .iter()
            .any(|m| m.starts_with("String comparison using ==")),
        "{unrelated:?}"
    );
    let null_check = all_messages("boolean t(String s) { return s == null || s != null; }");
    assert!(
        !null_check
            .iter()
            .any(|m| m.starts_with("String comparison using ==")),
        "{null_check:?}"
    );
}

#[test]
fn empty_catch_blocks_with_a_comment_or_an_ignored_name_are_intentional() {
    let intentional = all_messages(
        "void t() { try { Thread.sleep(1); } catch (InterruptedException e) { /* interrupted: nothing to do */ } try { Thread.sleep(1); } catch (InterruptedException ignored) { } try { Thread.sleep(1); } catch (InterruptedException e) {\n // shutting down\n } }",
    );
    assert!(
        !intentional
            .iter()
            .any(|m| m.starts_with("Empty catch block")),
        "{intentional:?}"
    );
    let plain =
        all_messages("void t() { try { Thread.sleep(1); } catch (InterruptedException e) { } }");
    assert!(
        plain.iter().any(|m| m.starts_with("Empty catch block")),
        "{plain:?}"
    );
}

#[test]
fn heuristic_null_and_threading_lints_are_off_by_default() {
    let found = all_messages(
        "String t(java.util.Map<String, String> m) { return m.get(\"a\").trim().replace(\"x\", \"y\"); }\nsynchronized void s() throws InterruptedException { Thread.sleep(1); }",
    );
    assert!(
        !found.iter().any(|m| m.contains("potentially null")
            || m.contains("Thread.sleep")
            || m.contains("synchronized")),
        "{found:?}"
    );
}

#[test]
fn numeric_narrowing_uses_javacs_lossy_conversion_wording() {
    let found = errors(
        &[user(
            "void t(double d, long l) { int a = d; int b = l; float f = d; }",
        )],
        "demo/Main.java",
    );
    assert_eq!(
        found,
        vec![
            "incompatible types: possible lossy conversion from double to int".to_owned(),
            "incompatible types: possible lossy conversion from long to int".to_owned(),
            "incompatible types: possible lossy conversion from double to float".to_owned(),
        ]
    );
}

#[test]
fn generic_throws_recorded_as_their_bound_are_not_reported() {
    // Bytecode records `throws X` (X extends Throwable) as `Throwable`; that
    // says nothing about what the caller's supplier throws.
    assert_no_errors(
        &[
            (
                "demo/Repo.java",
                "package demo;\npublic interface Repo<T> { java.util.Optional<T> findById(String id); }\n",
            ),
            user("Repo<String> repo; String find(String id) { return repo.findById(id).orElseThrow(() -> new IllegalArgumentException(\"missing \" + id)); }\nString other(java.util.Optional<String> o) { return o.orElseThrow(); }\nString third(java.util.Optional<String> o) { return o.orElseThrow(IllegalStateException::new); }"),
        ],
        "demo/Main.java",
    );
}

#[test]
fn static_call_next_to_an_instance_overload_is_not_reported() {
    assert_no_errors(
        &[user(
            "static void helper(int x) {} void helper() {} static void t() { helper(1); }",
        )],
        "demo/Main.java",
    );
}

#[test]
fn member_errors_are_not_reported_through_calculated_receivers() {
    // The receiver `make()` is a call result: its type is inferred, not spelled,
    // so a missing field on it is left to the compiler.
    assert_no_errors(
        &[user(
            "Main make() { return this; } int t() { return make().nothing; }",
        )],
        "demo/Main.java",
    );
}
