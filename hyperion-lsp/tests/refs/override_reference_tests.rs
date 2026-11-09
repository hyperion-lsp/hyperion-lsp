// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Find-references across an override family.
//!
//! A Java method is not a single declaration for the purposes of "find all
//! references". `Service.run()` and `Impl.run()` are one dispatch slot, and
//! `impl.run()` invokes both: the compile-time declaration it resolves against
//! and the implementation the JVM will actually run (JLS 15.12.4.4). Rename
//! has always modelled this -- it renames the whole override-connected family
//! atomically -- so find-references must agree with it, or the editor offers a
//! rename that touches locations it never showed the user.

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::Location;

fn keys(test: &LspTest, locations: &[Location]) -> Vec<String> {
    let mut keys = locations
        .iter()
        .map(|location| {
            test.location_to_key(location).map_or_else(
                |error| format!("<unresolvable: {error}>"),
                |key| format!("{key:?}"),
            )
        })
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

fn contains_line(locations: &[Location], file_suffix: &str, line: u32) -> bool {
    locations.iter().any(|location| {
        location.uri.path().ends_with(file_suffix) && location.range.start.line == line
    })
}

fn interface_and_two_implementations() -> LspTest {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Service.java",
        "package com.example;\n\
         public interface Service {\n\
         \x20   void ru<|>n();\n\
         }\n",
    );
    test.add_file(
        "com/example/FastService.java",
        "package com.example;\n\
         public class FastService implements Service {\n\
         \x20   @Override\n\
         \x20   public void ru<|>n() {}\n\
         }\n",
    );
    test.add_file(
        "com/example/SlowService.java",
        "package com.example;\n\
         public class SlowService implements Service {\n\
         \x20   @Override\n\
         \x20   public void run() {}\n\
         }\n",
    );
    test.add_file(
        "com/example/Caller.java",
        "package com.example;\n\
         public class Caller {\n\
         \x20   void viaInterface(Service service) {\n\
         \x20       service.run();\n\
         \x20   }\n\
         \x20   void viaImplementation(FastService service) {\n\
         \x20       service.run();\n\
         \x20   }\n\
         }\n",
    );
    test
}

/// Every overriding declaration is a reference to the base declaration, and so
/// is every call through a subtype receiver.
#[test]
fn references_to_an_interface_method_include_its_implementations() {
    let mut test = interface_and_two_implementations();
    let locations = test
        .find_references("com/example/Service.java", "<|>", true)
        .expect("find-references must not fail");

    assert!(
        contains_line(&locations, "Caller.java", 3),
        "call through the interface missing: {:?}",
        keys(&test, &locations)
    );
    assert!(
        contains_line(&locations, "FastService.java", 3),
        "overriding declaration missing: {:?}",
        keys(&test, &locations)
    );
    assert!(
        contains_line(&locations, "SlowService.java", 3),
        "second overriding declaration missing: {:?}",
        keys(&test, &locations)
    );
    assert!(
        contains_line(&locations, "Caller.java", 6),
        "call through the implementation missing: {:?}",
        keys(&test, &locations)
    );
}

#[test]
fn references_without_declarations_exclude_the_whole_override_family() {
    let mut test = interface_and_two_implementations();

    for selected_file in ["FastService.java", "Service.java"] {
        let file = format!("com/example/{selected_file}");
        let locations = test
            .find_references(&file, "<|>", false)
            .expect("find-references must not fail");

        for declaration_file in ["Service.java", "FastService.java", "SlowService.java"] {
            let declaration_line = if declaration_file == "Service.java" {
                2
            } else {
                3
            };
            assert!(
                !contains_line(
                    &locations,
                    &format!("/{declaration_file}"),
                    declaration_line
                ),
                "{selected_file} references included {declaration_file} declaration: {:?}",
                keys(&test, &locations)
            );
        }
        assert!(
            contains_line(&locations, "Caller.java", 3),
            "call through the interface missing: {:?}",
            keys(&test, &locations)
        );
        assert!(
            contains_line(&locations, "Caller.java", 6),
            "call through the implementation missing: {:?}",
            keys(&test, &locations)
        );
    }
}

/// The augmentation must not depend on the base declaration having any direct
/// references of its own: a base method nobody calls through the supertype
/// still has its overrides.
#[test]
fn a_base_method_with_no_direct_calls_still_reports_its_overrides() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Base.java",
        "package com.example;\n\
         public abstract class Base {\n\
         \x20   public abstract void unc<|>alled();\n\
         }\n",
    );
    test.add_file(
        "com/example/Concrete.java",
        "package com.example;\n\
         public class Concrete extends Base {\n\
         \x20   @Override\n\
         \x20   public void uncalled() {}\n\
         }\n",
    );

    let locations = test
        .find_references("com/example/Base.java", "<|>", true)
        .expect("find-references must not fail");

    assert!(
        contains_line(&locations, "Concrete.java", 3),
        "overriding declaration missing for an otherwise-unreferenced base: {:?}",
        keys(&test, &locations)
    );

    let usages = test
        .find_references("com/example/Base.java", "<|>", false)
        .expect("find-references without declarations must not fail");
    assert!(
        usages.is_empty(),
        "an uncalled abstract method has no usages: {:?}",
        keys(&test, &usages)
    );
}

/// The family is transitive: `Leaf` overrides `Middle`, which overrides
/// `Root`, so a search from `Root` reaches `Leaf`.
#[test]
fn the_override_family_is_transitive() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Root.java",
        "package com.example;\n\
         public class Root {\n\
         \x20   public void ti<|>ck() {}\n\
         }\n",
    );
    test.add_file(
        "com/example/Middle.java",
        "package com.example;\n\
         public class Middle extends Root {\n\
         \x20   @Override\n\
         \x20   public void tick() {}\n\
         }\n",
    );
    test.add_file(
        "com/example/Leaf.java",
        "package com.example;\n\
         public class Leaf extends Middle {\n\
         \x20   @Override\n\
         \x20   public void tick() {}\n\
         }\n",
    );

    let locations = test
        .find_references("com/example/Root.java", "<|>", true)
        .expect("find-references must not fail");

    assert!(
        contains_line(&locations, "Middle.java", 3),
        "direct override missing: {:?}",
        keys(&test, &locations)
    );
    assert!(
        contains_line(&locations, "Leaf.java", 3),
        "transitive override missing: {:?}",
        keys(&test, &locations)
    );
}

/// An unrelated same-named method with a different signature is not in the
/// family and must not be reported.
#[test]
fn an_unrelated_same_named_method_is_not_a_reference() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Contract.java",
        "package com.example;\n\
         public interface Contract {\n\
         \x20   void ha<|>ndle(String value);\n\
         }\n",
    );
    test.add_file(
        "com/example/Handler.java",
        "package com.example;\n\
         public class Handler implements Contract {\n\
         \x20   @Override\n\
         \x20   public void handle(String value) {}\n\
         \x20   public void handle(int value) {}\n\
         }\n",
    );

    let locations = test
        .find_references("com/example/Contract.java", "<|>", true)
        .expect("find-references must not fail");

    assert!(
        contains_line(&locations, "Handler.java", 3),
        "overriding declaration missing: {:?}",
        keys(&test, &locations)
    );
    assert!(
        !contains_line(&locations, "Handler.java", 4),
        "an unrelated overload was reported as a reference: {:?}",
        keys(&test, &locations)
    );
}
