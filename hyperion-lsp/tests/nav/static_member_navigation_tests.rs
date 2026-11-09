// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Static member navigation through inherited owners, imports, and lexical scope.

use crate::lsp_test_framework::{goto_response_to_locations, LspTest, SharedLspContext, TestFile};
use tower_lsp::lsp_types::GotoDefinitionResponse;

const BASE: &str = r"package p;
public class Base {
    public static final int «value»VALUE = 7;
    public static int «pick»pick(int value) { return value; }
}
";

fn fixture(clients: &[(&str, &str)]) -> SharedLspContext {
    let mut test = LspTest::new();
    test.add_file("p/Base.java", BASE);
    test.add_file(
        "p/Child.java",
        "package p; public class Child extends Base {}\n",
    );

    for &(path, source) in clients {
        test.add_file(path, source);
    }

    test.into_shared_context_with_options(false).unwrap()
}

fn assert_target(
    context: &SharedLspContext,
    source_file: &str,
    cursor: &str,
    target_file: &str,
    target_source: &str,
    target_marker: &str,
) {
    let response = context
        .goto_definition(source_file, cursor)
        .unwrap_or_else(|error| panic!("{source_file} {cursor}: {error}"));
    let locations = goto_response_to_locations(response);
    let expected = TestFile::new(target_source);
    let marker = expected.find_marker(target_marker).unwrap();

    assert_eq!(locations.len(), 1, "{source_file} {cursor}: {locations:?}");
    let location = &locations[0];
    assert!(
        location.uri.path().ends_with(target_file),
        "{source_file} {cursor}: expected {target_file}, got {location:?}"
    );
    assert_eq!(location.range.start.line, marker.line);
    assert_eq!(location.range.start.character, marker.character);
}

#[test]
fn inherited_owner_qualified_field_and_method() {
    let source = r"package q;
import p.Child;
class Use {
    int field = Child.«field_use»VALUE;
    int call = Child.«call_use»pick(1);
}
";
    let context = fixture(&[("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«field_use»",
        "p/Base.java",
        BASE,
        "«value»",
    );
    assert_target(
        &context,
        "q/Use.java",
        "«call_use»",
        "p/Base.java",
        BASE,
        "«pick»",
    );
}

#[test]
fn qualified_static_call_with_no_arguments() {
    let methods = r"package p;
public class Factory {
    public static int «make»make() { return 1; }
}
";
    let source = r"package q;
import p.Factory;
class Use { int value = Factory.«call_use»make(); }
";
    let context = fixture(&[("p/Factory.java", methods), ("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«call_use»",
        "p/Factory.java",
        methods,
        "«make»",
    );
}

#[test]
fn qualified_static_call_selects_zero_argument_overload() {
    let methods = r"package p;
public class Factory {
    public static int «make_zero»make() { return 1; }
    public static int «make_int»make(int value) { return value; }
}
";
    let source = r"package q;
import p.Factory;
class Use { int value = Factory.«call_use»make(); }
";
    let context = fixture(&[("p/Factory.java", methods), ("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«call_use»",
        "p/Factory.java",
        methods,
        "«make_zero»",
    );
}

#[test]
fn fully_qualified_static_call_across_packages() {
    let methods = r"package api;
public class Factory {
    public static int «make»make() { return 1; }
}
";
    let source = r"package client;
class Use { int value = api.Factory.«call_use»make(); }
";
    let context = fixture(&[("api/Factory.java", methods), ("client/Use.java", source)]);

    assert_target(
        &context,
        "client/Use.java",
        "«call_use»",
        "api/Factory.java",
        methods,
        "«make»",
    );
}

#[test]
fn inherited_zero_argument_static_call_across_packages() {
    let methods = r"package api;
public class BaseFactory {
    public static int «make»make() { return 1; }
}
";
    let child = "package bridge; public class Factory extends api.BaseFactory {}\n";
    let source = r"package client;
import bridge.Factory;
class Use { int value = Factory.«call_use»make(); }
";
    let context = fixture(&[
        ("api/BaseFactory.java", methods),
        ("bridge/Factory.java", child),
        ("client/Use.java", source),
    ]);

    assert_target(
        &context,
        "client/Use.java",
        "«call_use»",
        "api/BaseFactory.java",
        methods,
        "«make»",
    );
}

#[test]
fn zero_argument_static_call_selects_fixed_arity_over_varargs() {
    let methods = r"package api;
public class Factory {
    public static int «make_zero»make() { return 1; }
    public static int «make_varargs»make(Object... values) { return values.length; }
    public static int «make_one»make(int value) { return value; }
}
";
    let source = r"package client;
import api.Factory;
class Use { int value = Factory.«call_use»make(); }
";
    let context = fixture(&[("api/Factory.java", methods), ("client/Use.java", source)]);

    assert_target(
        &context,
        "client/Use.java",
        "«call_use»",
        "api/Factory.java",
        methods,
        "«make_zero»",
    );
}

#[test]
fn generic_zero_argument_static_factory_with_sibling_bound() {
    let bound = "package model; public class Product {}\n";
    let methods = r"package api;
import model.Product;
public class Factory {
    public static <T extends Product> T «make»make() { return null; }
}
";
    let source = r"package client;
import api.Factory;
import model.Product;
class Use { Product value = Factory.«call_use»make(); }
";
    let context = fixture(&[
        ("model/Product.java", bound),
        ("api/Factory.java", methods),
        ("client/Use.java", source),
    ]);

    assert_target(
        &context,
        "client/Use.java",
        "«call_use»",
        "api/Factory.java",
        methods,
        "«make»",
    );
}

#[test]
fn zero_argument_static_call_with_unindexed_overload_bound() {
    let methods = r"package api;
public class Factory {
    public static int «make_zero»make() { return 1; }
    public static <T extends dependency.External> T «make_generic»make(T value) {
        return value;
    }
}
";
    let source = r"package client;
import api.Factory;
class Use { int value = Factory.«call_use»make(); }
";
    let context = fixture(&[("api/Factory.java", methods), ("client/Use.java", source)]);
    let candidates =
        context
            .index
            .find_projected_effective_methods_by_name("api.Factory", &[], "make");
    assert_eq!(candidates.len(), 2, "both overloads must remain visible");
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.projection.is_some()),
        "a missing indexed bound alone does not invalidate projection: {candidates:#?}"
    );

    assert_target(
        &context,
        "client/Use.java",
        "«call_use»",
        "api/Factory.java",
        methods,
        "«make_zero»",
    );
}

#[test]
fn zero_argument_static_call_on_owner_with_recursive_bound() {
    let comparable = "package model; public interface Comparable<T> { int compareTo(T value); }\n";
    let methods = r"package api;
import model.Comparable;
public class Factory<T extends Comparable<T>> {
    public static int «make»make() { return 1; }
}
";
    let source = r"package client;
import api.Factory;
class Use { int value = Factory.«call_use»make(); }
";
    let context = fixture(&[
        ("model/Comparable.java", comparable),
        ("api/Factory.java", methods),
        ("client/Use.java", source),
    ]);

    assert_target(
        &context,
        "client/Use.java",
        "«call_use»",
        "api/Factory.java",
        methods,
        "«make»",
    );
}

#[test]
fn zero_argument_generic_static_call_with_explicit_type_argument() {
    let product = "package model; public class Product {}\n";
    let methods = r"package api;
import model.Product;
public class Factory {
    public static <T extends Product> T «make»make() { return null; }
}
";
    let source = r"package client;
import api.Factory;
import model.Product;
class Use { Product value = Factory.<Product>«call_use»make(); }
";
    let context = fixture(&[
        ("model/Product.java", product),
        ("api/Factory.java", methods),
        ("client/Use.java", source),
    ]);

    assert_target(
        &context,
        "client/Use.java",
        "«call_use»",
        "api/Factory.java",
        methods,
        "«make»",
    );
}

#[test]
fn qualified_static_call_on_imported_nested_type() {
    let outer = r"package p;
public class Outer {
    public static class Nested {
        public static int «build»build() { return 1; }
    }
}
";
    let source = r"package q;
import p.Outer.Nested;
class Use { int value = Nested.«call_use»build(); }
";
    let context = fixture(&[("p/Outer.java", outer), ("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«call_use»",
        "p/Outer.java",
        outer,
        "«build»",
    );
}

#[test]
fn qualified_static_factory_with_nested_static_call_argument() {
    let methods = r"package p;
public class Factory {
    public static int «seed»seed() { return 1; }
    public static int «from_int»from(int value) { return value; }
    public static int «from_boolean»from(boolean value) { return value ? 1 : 0; }
}
";
    let source = r"package q;
import p.Factory;
class Use { int value = Factory.«outer_use»from(Factory.«inner_use»seed()); }
";
    let context = fixture(&[("p/Factory.java", methods), ("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«inner_use»",
        "p/Factory.java",
        methods,
        "«seed»",
    );
    assert_target(
        &context,
        "q/Use.java",
        "«outer_use»",
        "p/Factory.java",
        methods,
        "«from_int»",
    );
}

#[test]
fn nested_call_receiver_respects_local_value_shadowing() {
    let imported = "package p; public class Factory { public static int seed() { return 1; } }\n";
    let local = r"package q;
import p.Factory;
class LocalFactory {
    boolean «seed»seed() { return true; }
    int «from_boolean»from(boolean value) { return value ? 1 : 0; }
    int «from_int»from(int value) { return value; }
}
class Use {
    int read(LocalFactory Factory) {
        return Factory.«outer_use»from(Factory.«inner_use»seed());
    }
}
";
    let context = fixture(&[("p/Factory.java", imported), ("q/Use.java", local)]);
    assert_target(
        &context,
        "q/Use.java",
        "«inner_use»",
        "q/Use.java",
        local,
        "«seed»",
    );
    assert_target(
        &context,
        "q/Use.java",
        "«outer_use»",
        "q/Use.java",
        local,
        "«from_boolean»",
    );
}

#[test]
fn nested_call_with_ambiguous_imported_receiver_has_no_definition() {
    let first = "package p; public class Factory { public static int seed() { return 1; } public static int from(int value) { return value; } }\n";
    let second = "package r; public class Factory { public static int seed() { return 1; } public static int from(int value) { return value; } }\n";
    let source = r"package q;
import p.*;
import r.*;
class Use { int value = Factory.«outer_use»from(Factory.seed()); }
";
    let context = fixture(&[
        ("p/Factory.java", first),
        ("r/Factory.java", second),
        ("q/Use.java", source),
    ]);
    let response = context
        .goto_definition("q/Use.java", "«outer_use»")
        .expect("navigation request failed");
    assert!(
        goto_response_to_locations(response).is_empty(),
        "ambiguous receiver must not select a declaration"
    );
}

#[test]
fn nested_call_receiver_respects_inherited_field_shadowing() {
    let imported = "package p; public class Factory { public static int seed() { return 1; } }\n";
    let source = r"package q;
import p.Factory;
class LocalFactory {
    boolean «seed»seed() { return true; }
    int «from_boolean»from(boolean value) { return value ? 1 : 0; }
    int from(int value) { return value; }
}
class Base { LocalFactory Factory = new LocalFactory(); }
class Use extends Base {
    int value = Factory.«outer_use»from(Factory.«inner_use»seed());
}
";
    let context = fixture(&[("p/Factory.java", imported), ("q/Use.java", source)]);
    assert_target(
        &context,
        "q/Use.java",
        "«inner_use»",
        "q/Use.java",
        source,
        "«seed»",
    );
    assert_target(
        &context,
        "q/Use.java",
        "«outer_use»",
        "q/Use.java",
        source,
        "«from_boolean»",
    );
}

#[test]
fn nested_call_static_imported_value_blocks_type_receiver() {
    let imported = "package p; public class Factory { public static int seed() { return 1; } public static int from(int value) { return value; } }\n";
    let holder = "package q; public class Holder { public static LocalFactory Factory = new LocalFactory(); } class LocalFactory { public boolean seed() { return true; } public int «from_boolean»from(boolean value) { return 1; } }\n";
    let source = r"package q;
import p.Factory;
import static q.Holder.Factory;
class Use { int value = Factory.«outer_use»from(Factory.seed()); }
";
    let context = fixture(&[
        ("p/Factory.java", imported),
        ("q/Holder.java", holder),
        ("q/Use.java", source),
    ]);
    let mut imports = hyperion_workspace::ImportMap::new();
    imports.current_package = Some("q".to_string());
    imports.add_static_import("Factory".to_string(), "q.Holder.Factory".to_string());
    assert_eq!(
        hyperion_workspace::import_resolver::ImportResolver::new(&imports, &context.index)
            .resolve_static_values("Factory")
            .len(),
        1,
        "fixture must bind the static value"
    );
    assert_target(
        &context,
        "q/Use.java",
        "«outer_use»",
        "q/Holder.java",
        holder,
        "«from_boolean»",
    );
}

#[test]
fn nested_call_type_parameter_bound_does_not_shadow_imported_class() {
    let methods = r"package p;
public class Factory {
    public static int seed() { return 1; }
    public static int «from_int»from(int value) { return value; }
    public static int from(boolean value) { return value ? 1 : 0; }
}
";
    let source = r"package q;
import p.Factory;
class Use {
    <T extends Factory> int read() {
        return Factory.«outer_use»from(Factory.seed());
    }
}
";
    let context = fixture(&[("p/Factory.java", methods), ("q/Use.java", source)]);
    assert_target(
        &context,
        "q/Use.java",
        "«outer_use»",
        "p/Factory.java",
        methods,
        "«from_int»",
    );
}

#[test]
fn nested_call_static_imported_value_uses_its_declaration_type_context() {
    let imported_factory = "package p; public class Factory { public int seed() { return 1; } }\n";
    let local_factory =
        "package q; public class Factory { public boolean seed() { return true; } }\n";
    let holder = "package q; public class Holder { public static Factory item = new Factory(); }\n";
    let choices = r"package p;
public class Choices {
    public static int «boolean_choice»choose(boolean value) { return value ? 1 : 0; }
    public static int «int_choice»choose(int value) { return value; }
}
";
    let source = r"package q;
import p.Choices;
import p.Factory;
import static q.Holder.item;
class Use { int value = Choices.«outer_use»choose(item.seed()); }
";
    let context = fixture(&[
        ("p/Factory.java", imported_factory),
        ("q/Factory.java", local_factory),
        ("q/Holder.java", holder),
        ("p/Choices.java", choices),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«outer_use»",
        "p/Choices.java",
        choices,
        "«boolean_choice»",
    );
}

#[test]
fn explicit_static_import_points_to_inherited_declaration_at_import_and_use() {
    let source = r"package q;
import static p.Child.«import_use»VALUE;
class Use {
    int value = «field_use»VALUE;
}
";
    let context = fixture(&[("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«import_use»",
        "p/Base.java",
        BASE,
        "«value»",
    );
    assert_target(
        &context,
        "q/Use.java",
        "«field_use»",
        "p/Base.java",
        BASE,
        "«value»",
    );
}

#[test]
fn wildcard_static_import_of_inherited_method() {
    let source = r"package q;
import static p.Child.*;
class Use {
    int value = «call_use»pick(1);
}
";
    let context = fixture(&[("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«call_use»",
        "p/Base.java",
        BASE,
        "«pick»",
    );
}

#[test]
fn nested_class_sees_enclosing_static_members() {
    let source = r"package p;
class Outer {
    static final int «outer_value»VALUE = 3;
    static int «outer_pick»pick(int value) { return value; }

    static class Inner {
        int value = «field_use»VALUE;
        int call = «call_use»pick(1);
    }
}
";
    let context = fixture(&[("p/Outer.java", source)]);

    assert_target(
        &context,
        "p/Outer.java",
        "«field_use»",
        "p/Outer.java",
        source,
        "«outer_value»",
    );
    assert_target(
        &context,
        "p/Outer.java",
        "«call_use»",
        "p/Outer.java",
        source,
        "«outer_pick»",
    );
}

#[test]
fn qualified_access_with_whitespace_and_comments() {
    let source = r"package q;
import p.Base;
class Use {
    int spaced = Base . «spaced_use»VALUE;
    int commented = Base /* receiver */ . /* member */ «commented_use»VALUE;
    int called = Base /* receiver */ . /* member */ «call_use»pick(1);
}
";
    let context = fixture(&[("q/Use.java", source)]);

    for cursor in ["«spaced_use»", "«commented_use»"] {
        assert_target(
            &context,
            "q/Use.java",
            cursor,
            "p/Base.java",
            BASE,
            "«value»",
        );
    }
    assert_target(
        &context,
        "q/Use.java",
        "«call_use»",
        "p/Base.java",
        BASE,
        "«pick»",
    );
}

#[test]
fn static_constant_in_annotation_argument() {
    let marker = "package q; public @interface Marker { int value(); }\n";
    let source = r"package q;
import static p.Child.VALUE;
@Marker(«field_use»VALUE)
class Use {}
";
    let context = fixture(&[("q/Marker.java", marker), ("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«field_use»",
        "p/Base.java",
        BASE,
        "«value»",
    );
}

#[test]
fn static_constant_in_switch_label() {
    let source = r"package q;
import static p.Child.VALUE;
class Use {
    int read(int input) {
        switch (input) {
            case «field_use»VALUE: return 1;
            default: return 0;
        }
    }
}
";
    let context = fixture(&[("q/Use.java", source)]);

    assert_target(
        &context,
        "q/Use.java",
        "«field_use»",
        "p/Base.java",
        BASE,
        "«value»",
    );
}

fn overloaded_lambda_fixture() -> (SharedLspContext, &'static str) {
    let function = "package p; public interface IntFn { int apply(int value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «functional»choose(IntFn fn) { return fn.apply(1); }
    public static int «integer»choose(int value) { return value; }
}
";
    let imported = r"package q;
import static p.Helpers.choose;
class ImportedUse {
    int value = «imported_use»choose(x -> x);
}
";
    let qualified = r"package q;
import p.Helpers;
class QualifiedUse {
    int value = Helpers.«qualified_use»choose(x -> x);
}
";
    let context = fixture(&[
        ("p/IntFn.java", function),
        ("p/Helpers.java", methods),
        ("q/ImportedUse.java", imported),
        ("q/QualifiedUse.java", qualified),
    ]);

    (context, methods)
}

#[test]
fn overloaded_static_import_with_lambda_argument() {
    let (context, methods) = overloaded_lambda_fixture();
    assert_target(
        &context,
        "q/ImportedUse.java",
        "«imported_use»",
        "p/Helpers.java",
        methods,
        "«functional»",
    );
}

#[test]
fn overloaded_qualified_static_call_with_lambda_argument() {
    let (context, methods) = overloaded_lambda_fixture();
    assert_target(
        &context,
        "q/QualifiedUse.java",
        "«qualified_use»",
        "p/Helpers.java",
        methods,
        "«functional»",
    );
}

#[test]
fn ambiguous_static_wildcard_import_has_no_definition() {
    let first = "package p; public class First { public static int VALUE; }\n";
    let second = "package p; public class Second { public static int VALUE; }\n";
    let source = r"package q;
import static p.First.*;
import static p.Second.*;
class Use {
    int value = «field_use»VALUE;
}
";
    let context = fixture(&[
        ("p/First.java", first),
        ("p/Second.java", second),
        ("q/Use.java", source),
    ]);

    let response: Option<GotoDefinitionResponse> = context
        .goto_definition("q/Use.java", "«field_use»")
        .expect("navigation request failed");
    assert!(
        goto_response_to_locations(response).is_empty(),
        "ambiguous imports must not select an arbitrary declaration"
    );
}

#[test]
fn static_imported_constants_declaration_shapes() {
    for (declaration, member) in [
        (
            "public class Constants { public static final int «target»VALUE = 7; }",
            "VALUE",
        ),
        (
            "public interface Constants { int «target»VALUE = 7; }",
            "VALUE",
        ),
        (
            "public @interface Constants { int «target»VALUE = 7; }",
            "VALUE",
        ),
        ("public enum Constants { «target»VALUE }", "VALUE"),
        (
            "public class Constants { public static final int FIRST = 1, «target»VALUE = 7; }",
            "VALUE",
        ),
    ] {
        for import in [format!("p.Constants.{member}"), "p.Constants.*".to_owned()] {
            let declaration = format!("package p; {declaration}\n");
            let source = format!("package q; import static {import}; class Use {{ Object value = «use»{member}; }}\n");
            let context = fixture(&[("p/Constants.java", &declaration), ("q/Use.java", &source)]);
            assert!(
                context
                    .goto_definition("q/Use.java", "«use»")
                    .unwrap()
                    .is_some(),
                "{declaration}\n{source}"
            );
            assert_target(
                &context,
                "q/Use.java",
                "«use»",
                "p/Constants.java",
                &declaration,
                "«target»",
            );
        }
    }
}

#[test]
fn later_field_declarators_resolve_through_instances_inheritance_and_lexical_scope() {
    let fields = r"package p;
public class Fields {
    public int first, «target»second;
    public int value() { return «local»second; }
}
";
    let source = r"package q;
class Use extends p.Fields {
    int value(p.Fields fields) { return fields.«instance»second + «inherited»second; }
}
";
    let context = fixture(&[("p/Fields.java", fields), ("q/Use.java", source)]);
    for marker in ["«instance»", "«inherited»"] {
        assert_target(
            &context,
            "q/Use.java",
            marker,
            "p/Fields.java",
            fields,
            "«target»",
        );
    }
    assert_target(
        &context,
        "p/Fields.java",
        "«local»",
        "p/Fields.java",
        fields,
        "«target»",
    );
}

#[test]
fn annotation_members_are_indexed_for_qualified_navigation() {
    let annotation = r"package p;
public @interface Settings {
    int «constant»VALUE = 7;
    int «element»value();
    class «nested»Nested {}
}
";
    let source = r"package q;
class Use {
    int constant = p.Settings.«constant_use»VALUE;
    int value(p.Settings settings) { return settings.«element_use»value(); }
    p.Settings.«nested_use»Nested nested;
}
";
    let context = fixture(&[("p/Settings.java", annotation), ("q/Use.java", source)]);
    for (usage, target) in [
        ("«constant_use»", "«constant»"),
        ("«element_use»", "«element»"),
        ("«nested_use»", "«nested»"),
    ] {
        assert_target(
            &context,
            "q/Use.java",
            usage,
            "p/Settings.java",
            annotation,
            target,
        );
    }
}
