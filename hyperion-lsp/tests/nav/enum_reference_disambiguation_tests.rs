// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::lsp_test_framework::{hover_to_text, LspTest};
use tower_lsp::lsp_types::GotoDefinitionResponse;

#[test]
fn same_named_enum_members_are_disambiguated_across_packages() {
    let mut test = LspTest::new();
    test.add_file(
        "src/main/java/com/alpha/Status.java",
        r"
            package com.alpha;

            public enum Status {
                ACTIVE,
                INACTIVE
            }
        ",
    );
    test.add_file(
        "src/main/java/com/beta/Status.java",
        r"
            package com.beta;

            public enum Status {
                ACTIVE,
                PENDING
            }
        ",
    );
    test.add_file(
        "src/main/java/com/consumer/AlphaConsumer.java",
        r"
            package com.consumer;

            import com.alpha.Status;

            public class AlphaConsumer {
                Status current() {
                    return Status.ACT<|>IVE;
                }
            }
        ",
    );
    test.add_file(
        "src/main/java/com/consumer/BetaConsumer.java",
        r"
            package com.consumer;

            import com.beta.Status;

            public class BetaConsumer {
                Status current() {
                    return Status.ACTIVE;
                }
            }
        ",
    );

    let references = test
        .find_references("src/main/java/com/consumer/AlphaConsumer.java", "<|>", true)
        .expect("find-references should succeed");
    let uris: Vec<&str> = references
        .iter()
        .map(|location| location.uri.as_str())
        .collect();

    assert!(
        uris.iter()
            .any(|uri| uri.ends_with("/com/alpha/Status.java")),
        "expected the alpha enum declaration; got {uris:?}"
    );
    assert!(
        uris.iter()
            .any(|uri| uri.ends_with("/com/consumer/AlphaConsumer.java")),
        "expected the alpha usage; got {uris:?}"
    );
    assert!(
        uris.iter()
            .all(|uri| !uri.ends_with("/com/beta/Status.java")),
        "must not include the beta enum declaration; got {uris:?}"
    );
    assert!(
        uris.iter()
            .all(|uri| !uri.ends_with("/com/consumer/BetaConsumer.java")),
        "must not include the beta usage; got {uris:?}"
    );
}

#[test]
fn unqualified_switch_member_uses_the_selector_enum_identity() {
    let mut test = LspTest::new();
    test.add_file(
        "src/main/java/com/alpha/EventType.java",
        "package com.alpha; public enum EventType { <alphaDecl>CREATED, DELETED }",
    );
    test.add_file(
        "src/main/java/com/beta/EventType.java",
        "package com.beta; public enum EventType { <betaDecl>CREATED, UPDATED }",
    );
    test.add_file(
        "src/main/java/com/consumer/Processor.java",
        r"
            package com.consumer;
            import com.beta.EventType;
            public class Processor {
                void process(EventType event) {
                    switch (event) {
                        case <caseUse>CRE<|>ATED: break;
                        default: break;
                    }
                }
            }
        ",
    );

    let definition = test
        .goto_definition("src/main/java/com/consumer/Processor.java", "<|>")
        .expect("goto-definition should succeed");
    let Some(GotoDefinitionResponse::Scalar(location)) = definition else {
        panic!("expected one exact enum member definition, got {definition:?}");
    };
    assert!(
        location.uri.path().ends_with("/com/beta/EventType.java"),
        "selector type must choose beta.EventType, got {}",
        location.uri
    );
    assert_eq!(
        test.location_to_key(&location).expect("definition key"),
        test.marker_location_key("src/main/java/com/beta/EventType.java", "<betaDecl>")
            .expect("beta declaration key")
    );

    let hover = test
        .hover("src/main/java/com/consumer/Processor.java", "<|>")
        .expect("hover should succeed")
        .expect("the enum label should have hover information");
    let hover_text = hover_to_text(&hover);
    assert!(hover_text.contains("EventType"), "{hover_text}");
    assert!(hover_text.contains("CREATED"), "{hover_text}");
    assert!(hover_text.contains("enum"), "{hover_text}");

    let references = test
        .find_references("src/main/java/com/consumer/Processor.java", "<|>", true)
        .expect("find-references should succeed");
    let actual = test
        .locations_to_keys(&references)
        .expect("reference location keys");
    let expected = test
        .marker_map_to_keys(&[
            (
                "src/main/java/com/beta/EventType.java".to_string(),
                vec!["<betaDecl>".to_string()],
            ),
            (
                "src/main/java/com/consumer/Processor.java".to_string(),
                vec!["<caseUse>".to_string()],
            ),
        ])
        .expect("expected reference keys");
    assert_eq!(
        actual, expected,
        "references must use the same enum identity"
    );

    assert!(test
        .prepare_rename("src/main/java/com/consumer/Processor.java", "<|>")
        .expect("prepare-rename should succeed")
        .is_some());
    let edit = test
        .rename(
            "src/main/java/com/consumer/Processor.java",
            "<|>",
            "RECEIVED",
        )
        .expect("rename should succeed")
        .expect("rename should return edits");
    let changes = edit.changes.expect("plain text edits");
    let edited_locations = changes
        .iter()
        .flat_map(|(uri, edits)| {
            edits.iter().map(|edit| tower_lsp::lsp_types::Location {
                uri: uri.clone(),
                range: edit.range,
            })
        })
        .collect::<Vec<_>>();
    assert!(changes
        .values()
        .flatten()
        .all(|edit| edit.new_text == "RECEIVED"));
    assert_eq!(
        test.locations_to_keys(&edited_locations)
            .expect("rename location keys"),
        expected,
        "rename must edit exactly the definition and references selected above"
    );
}
