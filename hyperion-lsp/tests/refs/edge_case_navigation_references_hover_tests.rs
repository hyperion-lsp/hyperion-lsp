// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Edge case coverage for navigation, references, and hover.

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{GotoDefinitionResponse, Hover, HoverContents, Location};

const CONSTANTS_JAVA: &str = r#"
package com.example.config;

public class Constants {
    public static final int TIMEOUT<def_timeout>_MS = 250;
    public static final String DEFAULT_<def_host>HOST = "localhost";
}
"#;

const SERVICE_JAVA: &str = r"
package com.example.service;

import static com.example.config.Constants.TIMEOUT_MS;
import static com.example.config.Constants.DEFAULT_HOST;

public class Service {
    public void connect() {
        int timeout = TIMEOUT<|1>_MS;
        String host = DEFAULT_<|2>HOST;
        System.out.println(timeout + host.length());
    }
}
";

const QUALIFIED_JAVA: &str = r"
package com.example.service;

import com.example.config.Constants;

public class QualifiedUsage {
    public int timeout() {
        return Constants.TIMEOUT<|3>_MS;
    }
}
";

const OUTER_JAVA: &str = r#"
package com.example.model;

public class Outer {
    public static class Inner {
        public String descri<def_describe>be() {
            return "inner";
        }
    }
}
"#;

const INNER_USAGE_JAVA: &str = r"
package com.example.service;

import com.example.model.Outer;
import com.example.model.Outer.Inner;

public class InnerUsage {
    public void run() {
        Inner inner = new Inner();
        inner.descri<|4>be();

        Outer.Inn<|5>er direct = new Outer.Inner();
        direct.describe();
    }
}
";

const STATUS_JAVA: &str = r"
package com.example.status;

public enum Status {
    O<def_ok>K,
    ERROR
}
";

const STATUS_SERVICE_JAVA: &str = r"
package com.example.service;

import static com.example.status.Status.OK;

public class StatusService {
    public String ok() {
        return OK<|6>.name();
    }
}
";

const STATUS_QUALIFIED_JAVA: &str = r"
package com.example.service;

import com.example.status.Status;

public class StatusQualified {
    public Status status() {
        return Status.OK;
    }
}
";

const CRLF_MAIN_JAVA: &str =
    "package com.example.crlf;\r\n\r\npublic class Main {\r\n    public void run() {\r\n        Helper h = new Helper();\r\n        h.hel<|7>per();\r\n    }\r\n}\r\n";

const CRLF_HELPER_JAVA: &str =
    "package com.example.crlf;\r\n\r\npublic class Helper {\r\n    public void helper() {}\r\n}\r\n";

const METHOD_REFERENCE_MAIN_JAVA: &str = r"
package com.example.refs;

import java.util.function.Function;

public class Main {
    public void run() {
        Function<String, String> f = String::toUpper<|>Case;
    }
}
";

const METHOD_REFERENCE_STRING_JAVA: &str = r#"
package java.lang;

public class String {
    public String toUpperCase() { return ""; }
}
"#;

const METHOD_REF_REFS_MAIN_JAVA: &str = r"
package com.example.refs;

import java.util.function.Function;

public class Main {
    public void run() {
        Function<String, String> f = String::toU<|>pperCase;
    }
}
";

const METHOD_REF_REFS_STRING_JAVA: &str = r#"
package java.lang;

public class String {
    public String toUpperC<def>ase() { return ""; }
}
"#;

const CTOR_REF_REFS_MAIN_JAVA: &str = r"
package com.example.refs;

import java.util.function.Supplier;

public class Main {
    public void run() {
        Supplier<Person> factory = Person::n<|>ew;
        Person person = new Person();
    }
}
";

const CTOR_REF_REFS_PERSON_JAVA: &str = r"
package com.example.refs;

public class Person {
    public Pers<def>on() {}
}
";

const ARRAY_HOVER_JAVA: &str = r"
package com.example.array;

public class Main {
    public void run(User[] users) {
        String name = users[0].<|>getName();
    }
}
";

const ARRAY_HOVER_USER_JAVA: &str = r#"
package com.example.array;

public class User {
    public String getName() { return ""; }
}
"#;

const CAST_HOVER_JAVA: &str = r"
package com.example.cast;

public class Main {
    public void run(Object obj) {
        ((Runner) obj).r<|>un();
    }
}
";

const CAST_HOVER_RUNNER_JAVA: &str = r"
package com.example.cast;

public interface Runner {
    void run();
}
";

const INNER_CLASS_TYPE_JAVA: &str = r"
package com.example.inner;

public class Outer {
    public static class Inn<def_inner>er {}
}
";

const INNER_CLASS_USAGE_JAVA: &str = r"
package com.example.inner;

public class UseInner {
    private Outer.Inner nested;
    private Inner imported;
}
";

fn assert_goto_file(result: Result<Option<GotoDefinitionResponse>, String>, expected: &str) {
    match result {
        Ok(Some(GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains(expected),
                "Expected navigation to '{}', got: {}",
                expected,
                loc.uri
            );
        }
        Ok(Some(GotoDefinitionResponse::Array(locs))) => {
            assert!(
                locs.iter().any(|loc| loc.uri.as_str().contains(expected)),
                "Expected navigation to '{}', got: {:?}",
                expected,
                locs
            );
        }
        Ok(Some(GotoDefinitionResponse::Link(_))) => {
            panic!("Unexpected link response");
        }
        Ok(None) => panic!("Expected definition in {}, got None", expected),
        Err(e) => panic!("goto_definition failed: {}", e),
    }
}

fn hover_text(hover: Hover) -> String {
    match hover.contents {
        HoverContents::Markup(markup) => markup.value,
        HoverContents::Scalar(scalar) => match scalar {
            tower_lsp::lsp_types::MarkedString::String(s) => s,
            tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
        },
        HoverContents::Array(arr) => arr
            .into_iter()
            .map(|s| match s {
                tower_lsp::lsp_types::MarkedString::String(text) => text,
                tower_lsp::lsp_types::MarkedString::LanguageString(ls) => ls.value,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn assert_refs(refs: Vec<Location>, min_count: usize, expected_files: &[&str]) {
    assert!(
        refs.len() >= min_count,
        "Expected at least {} references, found {}",
        min_count,
        refs.len()
    );

    for expected_file in expected_files {
        assert!(
            refs.iter()
                .any(|loc| loc.uri.as_str().contains(expected_file)),
            "Expected reference in '{}', got: {:?}",
            expected_file,
            refs
        );
    }
}

fn build_static_import_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/config/Constants.java", CONSTANTS_JAVA)
        .add_file("com/example/service/Service.java", SERVICE_JAVA)
        .add_file("com/example/service/QualifiedUsage.java", QUALIFIED_JAVA);
    test
}

fn build_inner_class_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/model/Outer.java", OUTER_JAVA)
        .add_file("com/example/service/InnerUsage.java", INNER_USAGE_JAVA);
    test
}

fn build_status_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/status/Status.java", STATUS_JAVA)
        .add_file(
            "com/example/service/StatusService.java",
            STATUS_SERVICE_JAVA,
        )
        .add_file(
            "com/example/service/StatusQualified.java",
            STATUS_QUALIFIED_JAVA,
        );
    test
}

fn build_crlf_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/crlf/Main.java", CRLF_MAIN_JAVA)
        .add_file("com/example/crlf/Helper.java", CRLF_HELPER_JAVA);
    test
}

fn build_method_reference_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/refs/Main.java", METHOD_REFERENCE_MAIN_JAVA)
        .add_file("java/lang/String.java", METHOD_REFERENCE_STRING_JAVA);
    test
}

fn build_method_ref_references_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/refs/Main.java", METHOD_REF_REFS_MAIN_JAVA)
        .add_file("java/lang/String.java", METHOD_REF_REFS_STRING_JAVA);
    test
}

fn build_ctor_ref_references_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/refs/Main.java", CTOR_REF_REFS_MAIN_JAVA)
        .add_file("com/example/refs/Person.java", CTOR_REF_REFS_PERSON_JAVA);
    test
}

fn build_array_hover_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/array/Main.java", ARRAY_HOVER_JAVA)
        .add_file("com/example/array/User.java", ARRAY_HOVER_USER_JAVA);
    test
}

fn build_cast_hover_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/cast/Main.java", CAST_HOVER_JAVA)
        .add_file("com/example/cast/Runner.java", CAST_HOVER_RUNNER_JAVA);
    test
}

fn build_inner_class_type_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/inner/Outer.java", INNER_CLASS_TYPE_JAVA)
        .add_file("com/example/inner/UseInner.java", INNER_CLASS_USAGE_JAVA);
    test
}

#[test]
fn goto_definition_static_import_field() {
    let mut test = build_static_import_workspace();
    let result = test.goto_definition("com/example/service/Service.java", "<|1>");
    assert_goto_file(result, "Constants.java");
}

#[test]
fn hover_static_import_field() {
    let mut test = build_static_import_workspace();
    let hover = test
        .hover("com/example/service/Service.java", "<|2>")
        .expect("hover failed")
        .expect("Expected hover result");
    let text = hover_text(hover);
    assert!(
        text.contains("DEFAULT_HOST"),
        "Expected hover text to contain DEFAULT_HOST, got: {}",
        text
    );
}

#[test]
fn find_references_static_import_field() {
    let mut test = build_static_import_workspace();
    let refs = test
        .find_references("com/example/config/Constants.java", "<def_timeout>", true)
        .expect("find_references failed");
    assert_refs(refs, 2, &["Constants.java", "QualifiedUsage.java"]);
}

#[test]
fn goto_definition_qualified_inner_class() {
    let mut test = build_inner_class_workspace();
    let result = test.goto_definition("com/example/service/InnerUsage.java", "<|5>");
    assert_goto_file(result, "Outer.java");
}

#[test]
fn hover_on_inner_method_call() {
    let mut test = build_inner_class_workspace();
    let hover = test
        .hover("com/example/service/InnerUsage.java", "<|4>")
        .expect("hover failed")
        .expect("Expected hover result");
    let text = hover_text(hover);
    assert!(
        text.contains("describe"),
        "Expected hover text to contain describe, got: {}",
        text
    );
}

#[test]
fn find_references_inner_method() {
    let mut test = build_inner_class_workspace();
    let refs = test
        .find_references("com/example/model/Outer.java", "<def_describe>", true)
        .expect("find_references failed");
    assert_refs(refs, 2, &["Outer.java", "InnerUsage.java"]);
}

#[test]
fn find_references_static_import_enum_constant() {
    let mut test = build_status_workspace();
    let refs = test
        .find_references("com/example/status/Status.java", "<def_ok>", true)
        .expect("find_references failed");
    assert_refs(refs, 2, &["Status.java", "StatusQualified.java"]);
}

#[test]
fn goto_definition_with_crlf_line_endings() {
    let mut test = build_crlf_workspace();
    let result = test.goto_definition("com/example/crlf/Main.java", "<|7>");
    assert_goto_file(result, "Helper.java");
}

#[test]
fn find_references_in_comment_and_string_returns_empty() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Notes.java",
        r#"
            package com.example;

            public class Notes {
                public void run() {
                    // TODO: call helper<|8>
                    String text = "helper<|9>";
                }

                public void helper() {}
            }
        "#,
    );

    let refs_comment = test
        .find_references("com/example/Notes.java", "<|8>", true)
        .expect("find_references failed");
    assert!(
        refs_comment.is_empty(),
        "Expected no references in comment, got: {:?}",
        refs_comment
    );

    let refs_string = test
        .find_references("com/example/Notes.java", "<|9>", true)
        .expect("find_references failed");
    assert!(
        refs_string.is_empty(),
        "Expected no references in string, got: {:?}",
        refs_string
    );
}

#[test]
fn find_references_includes_method_reference() {
    let mut test = build_method_ref_references_workspace();
    let refs = test
        .find_references("java/lang/String.java", "<def>", true)
        .expect("find_references failed");
    assert_refs(refs, 2, &["String.java", "Main.java"]);
}

#[test]
fn find_references_includes_constructor_reference() {
    let mut test = build_ctor_ref_references_workspace();
    let refs = test
        .find_references("com/example/refs/Person.java", "<def>", true)
        .expect("find_references failed");
    assert_refs(refs, 3, &["Person.java", "Main.java", "Main.java"]);
}

#[test]
fn hover_on_method_reference() {
    let mut test = build_method_reference_workspace();
    let hover = test
        .hover("com/example/refs/Main.java", "<|>")
        .expect("hover failed")
        .expect("Expected hover result");
    let text = hover_text(hover);
    assert!(
        text.contains("toUpperCase"),
        "Expected hover text to contain toUpperCase, got: {}",
        text
    );
}

#[test]
fn hover_on_array_element_method() {
    let mut test = build_array_hover_workspace();
    let hover = test
        .hover("com/example/array/Main.java", "<|>")
        .expect("hover failed")
        .expect("Expected hover result");
    let text = hover_text(hover);
    assert!(
        text.contains("getName"),
        "Expected hover text to contain getName, got: {}",
        text
    );
}

#[test]
fn hover_on_casted_receiver_method() {
    let mut test = build_cast_hover_workspace();
    let hover = test
        .hover("com/example/cast/Main.java", "<|>")
        .expect("hover failed")
        .expect("Expected hover result");
    let text = hover_text(hover);
    assert!(
        text.contains("run"),
        "Expected hover text to contain run, got: {}",
        text
    );
}

#[test]
fn find_references_inner_class_type_usage() {
    let mut test = build_inner_class_type_workspace();
    let refs = test
        .find_references("com/example/inner/Outer.java", "<def_inner>", true)
        .expect("find_references failed");
    assert_refs(refs, 2, &["Outer.java", "UseInner.java"]);
}
