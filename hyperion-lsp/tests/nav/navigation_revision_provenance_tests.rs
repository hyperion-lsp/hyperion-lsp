// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end guards for query-stamped workspace navigation locations.
//!
//! These deliberately exercise the production provider dispatch. A location
//! produced by any of these paths must retain its file-publication identity all
//! the way to LSP conversion; otherwise the central converter fails closed.

lsp_test_suite! {
    name: "Revisioned Workspace Navigation",

    test java_type_location_keeps_query_revision {
        files: {
            "p/Use.java" => "package p; class Use { Tar<|>get value; }",
            "p/Target.java" => "package p; class Target {}"
        },
        nav: "p/Use.java", "<|>" => "Target.java"
    }

    test kotlin_type_location_keeps_query_revision {
        files: {
            "p/Use.kt" => "package p\nclass Use(val value: Tar<|>get)",
            "p/Target.kt" => "package p\nclass Target"
        },
        nav: "p/Use.kt", "<|>" => "Target.kt"
    }

    test groovy_type_location_keeps_query_revision {
        files: {
            "p/Use.groovy" => "package p\nclass Use { Tar<|>get value }",
            "p/Target.groovy" => "package p\nclass Target {}"
        },
        nav: "p/Use.groovy", "<|>" => "Target.groovy"
    }

    test scala_type_location_keeps_query_revision {
        files: {
            "p/Use.scala" => "package p\nclass Use { val value: Tar<|>get = null }",
            "p/Target.scala" => "package p\nclass Target"
        },
        nav: "p/Use.scala", "<|>" => "Target.scala"
    }

    test annotation_location_keeps_query_revision {
        files: {
            "p/Use.java" => "package p; @Mar<|>ker class Use {}",
            "p/Marker.java" => "package p; public @interface Marker {}"
        },
        nav: "p/Use.java", "<|>" => "Marker.java"
    }

    test chained_member_location_keeps_query_revision {
        files: {
            "p/Use.java" => "package p; class Use { void f(Target t) { t.na<|>me(); } }",
            "p/Target.java" => "package p; class Target { void name() {} }"
        },
        nav: "p/Use.java", "<|>" => "Target.java"
    }
}
