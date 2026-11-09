// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Complex, multi-file LSP scenarios covering navigation, references, and hover.

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{GotoDefinitionResponse, HoverContents, Location};

const ORDER_JAVA: &str = r"
package com.example.domain;

import java.util.ArrayList;
import java.util.List;

public class Order {
    private final List<LineItem> items = new ArrayList<>();
    private final int taxRate;

    public Order(int taxRate) {
        this.taxRate = taxRate;
    }

    public int getTo<def>tal() {
        int subtotal = items.stream().mapToInt(LineItem::price).sum();
        return subtotal + (subtotal * taxRate / 100);
    }

    public List<LineItem> items() {
        return items;
    }

    public static Builder builder() {
        return new Builder();
    }

    public static class Builder {
        private final List<LineItem> items = new ArrayList<>();
        private int taxRate;

        public Builder lineI<lineitem>tem(String name, int priceCents) {
            items.add(new LineItem(name, priceCents));
            return this;
        }

        public Builder taxRate(int taxRate) {
            this.taxRate = taxRate;
            return this;
        }

        public Order build() {
            Order order = new Order(taxRate);
            order.items.addAll(items);
            return order;
        }
    }

    public record LineItem(String name, int price) {}
}
";

const AMOUNTS_JAVA: &str = r"
package com.example.util;

public class Amounts {
    public static int cents(int amount) {
        return amount;
    }
}
";

const REPOSITORY_JAVA: &str = r"
package com.example.repo;

import java.util.List;

public interface Repository<T> {
    List<T> findAll();

    default int count() {
        return findAll().size();
    }
}
";

const ORDER_REPOSITORY_JAVA: &str = r"
package com.example.repo;

import com.example.domain.Order;
import java.util.ArrayList;
import java.util.List;

public class OrderRepository implements Repository<Order> {
    @Override
    public List<Order> findAll() {
        return new ArrayList<>();
    }
}
";

const ORDER_SERVICE_JAVA: &str = r#"
package com.example.service;

import com.example.domain.Order;
import com.example.repo.OrderRepository;

import static com.example.util.Amounts.cents;

import java.util.List;

public class OrderService {
    private final OrderRepository repository = new OrderRepository();

    public Order createOrder() {
        return Order.builder()
            .lineI<|1>tem("Coffee", cen<|2>ts(450))
            .taxRate(10)
            .build();
    }

    public int sumTotals() {
        List<Order> orders = repository.findAll();
        return orders.stream().mapToInt(Order::getTo<|3>tal).sum();
    }

    public int sumTotalsInline() {
        return repository.findAll().stream()
            .mapToInt(o -> o.getTotal())
            .sum();
    }

    public int sampleTotal(Order order) {
        return order.getTotal();
    }
}
"#;

const PRICING_JAVA: &str = r"
package com.example.service;

import com.example.domain.Order;
import java.util.List;

public class Pricing {
    public int sumTotals(List<Order> orders) {
        int total = 0;
        for (Order order : orders) {
            total += order.getTotal();
        }
        return total;
    }
}
";

fn build_complex_workspace() -> LspTest {
    let mut test = LspTest::new();
    test.add_file("com/example/domain/Order.java", ORDER_JAVA)
        .add_file("com/example/util/Amounts.java", AMOUNTS_JAVA)
        .add_file("com/example/repo/Repository.java", REPOSITORY_JAVA)
        .add_file(
            "com/example/repo/OrderRepository.java",
            ORDER_REPOSITORY_JAVA,
        )
        .add_file("com/example/service/OrderService.java", ORDER_SERVICE_JAVA)
        .add_file("com/example/service/Pricing.java", PRICING_JAVA);
    test
}

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

fn assert_hover_contains(hover: Option<tower_lsp::lsp_types::Hover>, expected: &[&str]) {
    let hover = hover.expect("Expected hover result");
    let hover_text = match hover.contents {
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
    };

    for expected_text in expected {
        assert!(
            hover_text.contains(expected_text),
            "Expected hover text to contain '{}', got: {}",
            expected_text,
            hover_text
        );
    }
}

fn assert_refs(refs: Vec<Location>, min_count: usize, expected_files: &[&str]) {
    assert!(
        refs.len() >= min_count,
        "Expected at least {} references, found {}: {:?}",
        min_count,
        refs.len(),
        refs
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

#[test]
fn goto_definition_builder_chain_method() {
    let mut test = build_complex_workspace();
    let result = test.goto_definition("com/example/service/OrderService.java", "<|1>");
    assert_goto_file(result, "Order.java");
}

#[test]
fn goto_definition_static_import_method() {
    let mut test = build_complex_workspace();
    let result = test.goto_definition("com/example/service/OrderService.java", "<|2>");
    assert_goto_file(result, "Amounts.java");
}

#[test]
fn find_references_for_get_total_across_files() {
    let mut test = build_complex_workspace();
    let refs = test
        .find_references("com/example/domain/Order.java", "<def>", true)
        .expect("find_references failed");
    assert_refs(
        refs,
        3,
        &["Order.java", "OrderService.java", "Pricing.java"],
    );
}

#[test]
fn find_references_for_builder_method() {
    let mut test = build_complex_workspace();
    let refs = test
        .find_references("com/example/domain/Order.java", "<lineitem>", true)
        .expect("find_references failed");
    assert_refs(refs, 2, &["Order.java", "OrderService.java"]);
}

#[test]
fn hover_on_builder_chain_method() {
    let mut test = build_complex_workspace();
    let hover = test
        .hover("com/example/service/OrderService.java", "<|1>")
        .expect("hover failed");
    assert_hover_contains(hover, &["lineItem"]);
}

#[test]
fn hover_on_method_reference_in_stream_chain() {
    let mut test = build_complex_workspace();
    let hover = test
        .hover("com/example/service/OrderService.java", "<|3>")
        .expect("hover failed");
    assert_hover_contains(hover, &["getTotal"]);
}

#[test]
fn canonical_method_identity_is_consistent_across_navigation_features() {
    let mut test = LspTest::new();
    test.add_file(
        "left/Worker.java",
        concat!(
            "package left; public class Worker { ",
            "public int pick(String value) { return 1; } }",
        ),
    );
    test.add_file(
        "right/Worker.java",
        concat!(
            "package right; public class Worker { ",
            "public String pi<decl>ck(String value) { return value; } }",
        ),
    );
    test.add_file(
        "app/Service.java",
        concat!(
            "package app; import right.Worker; class Service { ",
            "String execute(Worker worker) { return worker.pi<use>ck(\"chosen\"); } }",
        ),
    );

    assert_goto_file(
        test.goto_definition("app/Service.java", "<use>"),
        "right/Worker.java",
    );

    let hover = test
        .hover("app/Service.java", "<use>")
        .expect("hover request should succeed");
    assert_hover_contains(hover, &["String", "pick"]);

    let references = test
        .find_references("right/Worker.java", "<decl>", true)
        .expect("references should succeed");
    assert_eq!(references.len(), 2);
    assert!(references
        .iter()
        .any(|location| location.uri.path().ends_with("/right/Worker.java")));
    assert!(references
        .iter()
        .any(|location| location.uri.path().ends_with("/app/Service.java")));
    assert!(references
        .iter()
        .all(|location| !location.uri.path().ends_with("/left/Worker.java")));

    let edit = test
        .rename("app/Service.java", "<use>", "choose")
        .expect("rename request should succeed")
        .expect("rename should produce edits");
    let changes = edit
        .changes
        .expect("rename should use simple workspace changes");
    assert_eq!(changes.values().map(Vec::len).sum::<usize>(), 2);
    assert!(changes
        .keys()
        .any(|uri| uri.path().ends_with("/right/Worker.java")));
    assert!(changes
        .keys()
        .any(|uri| uri.path().ends_with("/app/Service.java")));
    assert!(changes
        .keys()
        .all(|uri| !uri.path().ends_with("/left/Worker.java")));
}

#[test]
fn same_named_nested_types_keep_one_lexical_identity_across_features() {
    let mut test = LspTest::new();
    test.add_file(
        "p/Containers.java",
        r"
            package p;

            class Left {
                static class <leftDecl>Node {}
                <leftUse>Node value;
            }

            class Right {
                static class <rightDecl>Node {}
                <rightUse>Node value;

                <rightReturn>Node echo(<rightParam>Node input) {
                    return input;
                }
            }
        ",
    );

    let definition = test
        .goto_definition("p/Containers.java", "<rightUse>")
        .expect("goto-definition should succeed");
    let Some(GotoDefinitionResponse::Scalar(definition)) = definition else {
        panic!("expected one nested type definition, got {definition:?}");
    };
    assert_eq!(
        test.location_to_key(&definition).expect("definition key"),
        test.marker_location_key("p/Containers.java", "<rightDecl>")
            .expect("right nested declaration key")
    );

    let hover = test
        .hover("p/Containers.java", "<rightUse>")
        .expect("hover should succeed");
    assert_hover_contains(hover, &["Node", "p.Right.Node"]);

    let expected = test
        .marker_map_to_keys(&[(
            "p/Containers.java".to_string(),
            vec![
                "<rightDecl>".to_string(),
                "<rightUse>".to_string(),
                "<rightReturn>".to_string(),
                "<rightParam>".to_string(),
            ],
        )])
        .expect("expected right nested type locations");
    let references = test
        .find_references("p/Containers.java", "<rightUse>", true)
        .expect("find-references should succeed");
    assert_eq!(
        test.locations_to_keys(&references)
            .expect("reference location keys"),
        expected,
        "references must not cross into Left.Node"
    );

    assert!(test
        .prepare_rename("p/Containers.java", "<rightUse>")
        .expect("prepare-rename should succeed")
        .is_some());
    let edit = test
        .rename("p/Containers.java", "<rightUse>", "Element")
        .expect("rename should succeed")
        .expect("rename should produce edits");
    let edited_locations = edit
        .changes
        .expect("nested rename should use text edits")
        .into_iter()
        .flat_map(|(uri, edits)| {
            edits.into_iter().map(move |edit| Location {
                uri: uri.clone(),
                range: edit.range,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        test.locations_to_keys(&edited_locations)
            .expect("rename location keys"),
        expected,
        "rename must use the same nested type identity"
    );
}
