// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! A dereference's own `NullPointerException` lane must not turn a receiver of
//! unknown nullness into evidence of nullness.
//!
//! Without that rule `log.info(..)` in a try body creates an NPE lane with
//! `log = Null`; `catch (Exception e)` joins it with the lanes where `log` was
//! already dereferenced successfully, gets `Nullable`, and warns on every
//! logging call in the handler.
//!
//! `Unknown` entries are the opt-in channel (off by default in the server);
//! any other nullability is published as a WARNING, which is the bug.

mod common_nullability;

use common_nullability::analyze_single_method;
use hyperion_semantics::nullability::{DiagnosticKind, Nullability, NullabilityAnalysis};

/// Every potential-null dereference as (variable, nullability), in source order.
/// `Unknown` entries are the opt-in channel (off by default in the server);
/// anything else is published as a WARNING.
fn dereferences(analysis: &NullabilityAnalysis) -> Vec<(&str, Nullability)> {
    analysis
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == DiagnosticKind::PotentialNullDereference)
        .map(|diagnostic| (diagnostic.variable_name.as_str(), diagnostic.nullability))
        .collect()
}

/// The dereferences the server publishes as warnings by default.
fn warnings(analysis: &NullabilityAnalysis) -> Vec<&str> {
    dereferences(analysis)
        .into_iter()
        .filter(|(_, nullability)| *nullability != Nullability::Unknown)
        .map(|(name, _)| name)
        .collect()
}

#[test]
fn logging_in_catch_exception_does_not_warn_for_unknown_field_receiver() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            private static final Logger log = Logger.getLogger("a");
            private final Runnable task = () -> {};
            String test(String input) {
                try {
                    log.info("start");
                    task.run();
                    return input.trim();
                } catch (IllegalArgumentException e) {
                    log.warning("bad argument");
                    return "";
                } catch (Exception e) {
                    log.severe("failed");
                    task.run();
                    return "";
                }
            }
        }
        "#,
    );

    assert_eq!(
        warnings(&analysis),
        Vec::<&str>::new(),
        "an unknown-nullness receiver must not be published as a warning in a catch handler"
    );
    assert_eq!(
        dereferences(&analysis),
        [("log", Nullability::Unknown), ("log", Nullability::Unknown)],
        "log.info in the try body and log.severe in the handler stay at the opt-in Unknown level"
    );
}

/// (dereference form, receiver type, statement that dereferences `v`,
/// `@Nullable` result). The last column is what the analysis reported for a
/// `@Nullable` receiver before the NPE-lane rule existed; it must not change.
type Site = (
    &'static str,
    &'static str,
    &'static str,
    &'static [(&'static str, Nullability)],
);

const NULLABLE: Nullability = Nullability::Nullable;
const NULL: Nullability = Nullability::Null;

const SITES: &[Site] = &[
    (
        "method call",
        "Object",
        "v.toString();",
        &[("v", NULLABLE), ("v", NULLABLE)],
    ),
    (
        "field read",
        "Holder",
        "Object x = v.field;",
        &[("v", NULLABLE), ("v", NULL)],
    ),
    (
        "field write",
        "Holder",
        "v.field = null;",
        &[("v", NULLABLE), ("v", NULL)],
    ),
    (
        "array read",
        "int[]",
        "int x = v[0];",
        &[("v", NULLABLE), ("v", NULLABLE)],
    ),
    (
        "array length",
        "int[]",
        "int x = v.length;",
        &[("v", NULLABLE), ("v", NULL)],
    ),
    (
        "unboxing",
        "Integer",
        "int x = v;",
        &[("v", NULLABLE), ("v", NULL)],
    ),
    (
        "primitive cast",
        "Object",
        "int x = (int) v;",
        &[("v", NULLABLE), ("v", NULLABLE)],
    ),
    ("throw", "RuntimeException", "throw v;", &[("v", NULLABLE)]),
    (
        "method reference",
        "Object",
        "Supplier<String> s = v::toString;",
        &[("v", NULLABLE), ("v", NULL)],
    ),
    (
        "qualified new",
        "Outer",
        "Object x = v.new Inner();",
        &[("v", NULLABLE), ("v", NULLABLE)],
    ),
    (
        "synchronized",
        "Object",
        "synchronized (v) { }",
        &[("v", NULLABLE), ("v", NULL)],
    ),
    (
        "enhanced for",
        "List<String>",
        "for (String s : v) { }",
        &[("v", NULLABLE), ("v", NULL)],
    ),
];

/// The receiver is a local whose value comes from an unannotated call, so its
/// nullability is `Unknown`. The handler dereferences it again.
fn unknown_receiver_source(receiver_type: &str, dereference: &str) -> String {
    format!(
        "class Test {{ void test() {{ {receiver_type} v = Registry.lookup(); \
         try {{ {dereference} }} catch (Exception e) {{ v.toString(); }} }} }}"
    )
}

/// The receiver is an explicitly `@Nullable` parameter.
fn nullable_receiver_source(receiver_type: &str, dereference: &str) -> String {
    format!(
        "class Test {{ void test(@Nullable {receiver_type} v) {{ \
         try {{ {dereference} }} catch (Exception e) {{ v.toString(); }} }} }}"
    )
}

#[test]
fn unknown_receiver_is_not_null_in_catch_exception_for_every_dereference_form() {
    for (form, receiver_type, dereference, _) in SITES {
        let analysis = analyze_single_method(&unknown_receiver_source(receiver_type, dereference));
        assert_eq!(
            warnings(&analysis),
            Vec::<&str>::new(),
            "{form}: an Unknown receiver must not be published as a warning in the handler: {:?}",
            dereferences(&analysis)
        );
    }
}

#[test]
fn nullable_receiver_reports_are_unchanged_for_every_dereference_form() {
    for (form, receiver_type, dereference, expected) in SITES {
        let analysis = analyze_single_method(&nullable_receiver_source(receiver_type, dereference));
        assert_eq!(
            dereferences(&analysis),
            *expected,
            "{form}: a @Nullable receiver must keep its Null NPE lane"
        );
    }
}

#[test]
fn unknown_receiver_is_not_null_in_npe_handler() {
    // Accepted loss: with no evidence of nullness, `catch (NullPointerException)`
    // does not by itself make the receiver null.
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object v = Registry.lookup();
                try {
                    v.toString();
                } catch (NullPointerException e) {
                    v.hashCode();
                }
            }
        }
        "#,
    );

    assert_eq!(warnings(&analysis), Vec::<&str>::new());
}

#[test]
fn null_assigned_receiver_is_still_null_in_npe_handler() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test(boolean flag) {
                String value = flag ? "x" : null;
                try {
                    value.length();
                } catch (NullPointerException e) {
                    value.length();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereferences(&analysis),
        [
            ("value", Nullability::Nullable),
            ("value", Nullability::Nullable)
        ],
        "a possibly-null-assigned receiver is reported in the try body and again in the handler"
    );
}

#[test]
fn definitely_null_receiver_is_still_null_in_handler() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                String value = null;
                try {
                    value.length();
                } catch (NullPointerException e) {
                    value.length();
                }
            }
        }
        "#,
    );

    assert_eq!(
        dereferences(&analysis),
        [("value", Nullability::Null), ("value", Nullability::Null)]
    );
}

#[test]
fn unknown_receiver_try_body_dereference_is_still_reported_at_unknown_level() {
    let analysis = analyze_single_method(
        r#"
        class Test {
            void test() {
                Object v = Registry.lookup();
                try {
                    v.toString();
                } catch (RuntimeException e) {
                }
            }
        }
        "#,
    );

    assert_eq!(dereferences(&analysis), [("v", Nullability::Unknown)]);
}
