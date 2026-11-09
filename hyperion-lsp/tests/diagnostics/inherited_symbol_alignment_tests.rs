// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Paired diagnostics and navigation checks for inherited Java members.

use std::{path::Path, sync::Arc};

use hyperion_lsp::diagnostics::semantic_error_to_diagnostic;
use hyperion_parser::{lexer::Lexer, parser::Parser, SyntaxNode};
use hyperion_semantics::{semantic_walker::compute_file_semantics, ResolverBuilder, TypeChecker};
use tower_lsp::lsp_types::{Diagnostic, GotoDefinitionResponse};

use crate::lsp_test_framework::{LspTest, SharedLspContext, TestFile};

fn fixture(
    child_path: &str,
    child_source: &str,
    dependencies: &[(&str, &str)],
) -> (SharedLspContext, Vec<Diagnostic>) {
    let mut test = LspTest::new();
    for (path, source) in dependencies {
        test.add_file(path, source);
    }
    test.add_file(child_path, child_source);

    let path = test.file_path(child_path);
    let source = TestFile::new(child_source).clean_content;
    let context = test
        .into_shared_context_with_options(false)
        .expect("index fixture once for diagnostics and navigation");
    let diagnostics = diagnostics_for(&context, &path, &source);
    (context, diagnostics)
}

fn diagnostics_for(context: &SharedLspContext, path: &Path, source: &str) -> Vec<Diagnostic> {
    let file_id = context.index.get_file_id(path).expect("indexed child file");
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let syntax = SyntaxNode::new_root(green);
    let resolver = ResolverBuilder::with_workspace(file_id, Arc::clone(&context.index))
        .build_from_syntax(&syntax);
    let semantics = Arc::new(compute_file_semantics(
        &syntax,
        file_id,
        resolver.symbol_table(),
        &context.index,
    ));
    let mut errors = semantics.diagnostics().to_vec();
    let mut checker = TypeChecker::new();
    checker.set_file_semantics(semantics);
    errors.extend(checker.check_with_context(&syntax, &[], file_id, Arc::clone(&context.index)));
    errors
        .iter()
        .map(|error| semantic_error_to_diagnostic(error, source))
        .collect()
}

fn assert_no_diagnostics(diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:#?}"
    );
}

fn assert_target(
    context: &SharedLspContext,
    use_file: &str,
    use_marker: &str,
    declaration_file: &str,
    declaration_source: &str,
    declaration_marker: &str,
) {
    let response = context
        .goto_definition(use_file, use_marker)
        .expect("goto definition")
        .expect("inherited member should navigate");
    let GotoDefinitionResponse::Scalar(location) = response else {
        panic!("expected one declaration, got {response:?}");
    };
    let declaration = TestFile::new(declaration_source);
    let expected = declaration
        .find_marker(declaration_marker)
        .expect("declaration marker");
    assert!(
        location.uri.path().ends_with(declaration_file),
        "expected {declaration_file}, got {}",
        location.uri
    );
    assert_eq!(
        location.range.start.line, expected.line,
        "wrong declaration line"
    );
    assert_eq!(
        location.range.start.character, expected.character,
        "wrong declaration column"
    );
}

#[test]
fn inherited_unqualified_field_and_method_agree() {
    const BASE: &str = r"
        package base;
        public class Parent {
            protected int «field»count;
            protected int «method»readCount() { return count; }
        }
    ";
    const CHILD: &str = r"
        package app;
        public class Child extends base.Parent {
            int read() { return «field_use»count + «method_use»readCount(); }
        }
    ";
    let (context, diagnostics) = fixture("app/Child.java", CHILD, &[("base/Parent.java", BASE)]);
    assert_no_diagnostics(&diagnostics);
    assert_target(
        &context,
        "app/Child.java",
        "«field_use»",
        "base/Parent.java",
        BASE,
        "«field»",
    );
    assert_target(
        &context,
        "app/Child.java",
        "«method_use»",
        "base/Parent.java",
        BASE,
        "«method»",
    );
}

#[test]
fn inherited_generic_return_uses_declaring_imports_for_both_paths() {
    const CHILD: &str = r"
        package app;
        import other.CatalogApi;
        import util.ServiceHandle;
        public class Child extends base.Parent {
            void run() {
                try (ServiceHandle<api.CatalogApi.Contract> handle = «factory_use»inheritedFactory()) {}
            }
        }
    ";
    const PARENT: &str = r"
        package base;
        import api.CatalogApi;
        import util.ServiceHandle;
        public class Parent {
            protected ServiceHandle<CatalogApi.Contract> «factory»inheritedFactory() { return null; }
        }
    ";
    let dependencies = [
        ("api/CatalogApi.java", "package api; public class CatalogApi { public interface Contract {} }"),
        ("other/CatalogApi.java", "package other; public class CatalogApi { public interface Contract {} }"),
        ("util/ServiceHandle.java", "package util; public class ServiceHandle<T> implements AutoCloseable { public void close() {} }"),
        ("base/Parent.java", PARENT),
    ];
    let (context, diagnostics) = fixture("app/Child.java", CHILD, &dependencies);
    assert_no_diagnostics(&diagnostics);
    assert_target(
        &context,
        "app/Child.java",
        "«factory_use»",
        "base/Parent.java",
        PARENT,
        "«factory»",
    );
}

#[test]
fn inherited_overload_selects_matching_parameter() {
    const PARENT: &str = r"
        package base;
        public class Parent {
            protected int «string_overload»choose(String value) { return 0; }
            protected int «int_overload»choose(int value) { return value; }
        }
    ";
    const CHILD: &str = r#"
        package app;
        public class Child extends base.Parent {
            int run() { return «int_call»choose(1) + «string_call»choose("one"); }
        }
    "#;
    let (context, diagnostics) = fixture("app/Child.java", CHILD, &[("base/Parent.java", PARENT)]);
    assert_no_diagnostics(&diagnostics);
    assert_target(
        &context,
        "app/Child.java",
        "«int_call»",
        "base/Parent.java",
        PARENT,
        "«int_overload»",
    );
    assert_target(
        &context,
        "app/Child.java",
        "«string_call»",
        "base/Parent.java",
        PARENT,
        "«string_overload»",
    );
}

#[test]
fn inaccessible_parent_members_remain_unresolved() {
    const PARENT: &str = r"
        package base;
        public class Parent {
            private int hidden;
            private int hiddenMethod() { return hidden; }
        }
    ";
    const CHILD: &str = r"
        package app;
        public class Child extends base.Parent {
            int run() { return «field_use»hidden + «method_use»hiddenMethod(); }
        }
    ";
    let (context, diagnostics) = fixture("app/Child.java", CHILD, &[("base/Parent.java", PARENT)]);
    let child = TestFile::new(CHILD);
    for marker in ["«field_use»", "«method_use»"] {
        let use_site = child.find_marker(marker).expect("use marker");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.range.start.line == use_site.line
                    && diagnostic.range.start.character == use_site.character
                    && matches!(
                        &diagnostic.code,
                        Some(tower_lsp::lsp_types::NumberOrString::String(code))
                            if code == "undefined-symbol"
                    )
            }),
            "expected undefined-symbol at {marker}: {diagnostics:#?}"
        );
    }
    for marker in ["«field_use»", "«method_use»"] {
        assert!(
            context
                .goto_definition("app/Child.java", marker)
                .expect("goto definition")
                .is_none(),
            "private parent member should not navigate from subclass: {marker}"
        );
    }
}
