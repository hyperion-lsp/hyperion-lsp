// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Static overload navigation must retain the types of ordinary value expressions.

use crate::lsp_test_framework::{goto_response_to_locations, LspTest, TestFile};

fn assert_integer_overload(argument: &str) {
    let declarations = r"package api;
public class Choices {
    public static int count = 1;
    public static int choose(boolean value) { return 0; }
    public static int «integer»choose(int value) { return value; }
}
";
    let client = format!(
        "package client;\nimport api.Choices;\nclass Use {{\n    int run(int[] values) {{ return Choices.«call»choose({argument}); }}\n}}\n"
    );
    let mut fixture = LspTest::new();
    fixture.add_file("api/Choices.java", declarations);
    fixture.add_file("client/Use.java", &client);
    let context = fixture.into_shared_context_with_options(false).unwrap();
    let locations = goto_response_to_locations(
        context
            .goto_definition("client/Use.java", "«call»")
            .unwrap(),
    );
    assert_eq!(locations.len(), 1, "argument {argument}: {locations:?}");
    let target = &locations[0];
    assert!(target.uri.path().ends_with("api/Choices.java"));
    let source = TestFile::new(declarations);
    let expected = source.find_marker("«integer»").unwrap();
    assert_eq!(target.range.start.line, expected.line);
    assert_eq!(target.range.start.character, expected.character);
}

#[test]
fn static_overload_with_field_argument() {
    assert_integer_overload("Choices.count");
}

#[test]
fn static_overload_with_array_element_argument() {
    assert_integer_overload("values[0]");
}
