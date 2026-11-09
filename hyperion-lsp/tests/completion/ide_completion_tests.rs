// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! IDE-grade completion, driven through the LSP
//! completion entry point: receivers that are call results, static access
//! through type names, type-name search over the JDK, lambda parameters,
//! ranking, camel humps, imports, annotations, constructors and snippets.
//!
//! One workspace (with the JDK indexed once) serves every test; each scenario
//! is its own file with one `<|>` cursor.

use crate::lsp_test_framework::{LspTest, SharedLspContext};
use std::collections::HashSet;
use std::sync::OnceLock;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionItemTag};

const SCENARIOS: &[(&str, &str)] = &[
    (
        "Customer.java",
        "public class Customer {
            private String email;
            public String getEmail() { return email; }
            public static Customer create() { return new Customer(); }
            public static final int MAX = 3;
            public int age() { return 1; }
        }",
    ),
    (
        "Status.java",
        "public enum Status { ACTIVE, INACTIVE; public static Status parse(String s) { return ACTIVE; } }",
    ),
    ("lombok/Builder.java", "package lombok; public @interface Builder {}"),
    (
        "Person.java",
        "import lombok.Builder; @Builder public class Person { private String name; private int age; }",
    ),
    ("chain_stream.java", "import java.util.*; class T { void m(List<String> names) { names.stream().<|> } }"),
    ("chain_sysout.java", "class T { void m() { System.out.<|> } }"),
    ("chain_workspace.java", "class T { void m(Customer c) { c.getEmail().<|> } }"),
    ("chain_listget.java", "import java.util.*; class T { void m(List<String> l) { l.get(0).<|> } }"),
    (
        "chain_optional.java",
        "import java.util.*; class T { void m(String x) { Optional.of(x).map(s -> s.length()).<|> } }",
    ),
    ("chain_builder.java", "class T { void m() { Person.builder().<|> } }"),
    ("chain_builder2.java", "class T { void m() { Person.builder().name(\"x\").<|> } }"),
    (
        "chain_requirenonnull.java",
        "import java.util.*; class T { void m(Customer c) { Objects.requireNonNull(c).<|> } }",
    ),
    ("chain_newsb.java", "class T { void m() { new StringBuilder().append(\"x\").<|> } }"),
    (
        "chain_field.java",
        "import java.util.*; class T { private final Map<String, List<Customer>> by = new HashMap<>(); void m() { by.get(\"a\").get(0).<|> } }",
    ),
    (
        "chain_var.java",
        "import java.util.*; class T { void m() { var l = new ArrayList<String>(); l.stream().<|> } }",
    ),
    ("chain_prefix.java", "import java.util.*; class T { void m(List<String> names) { names.stream().fi<|> } }"),
    ("static_customer.java", "class T { void m() { Customer.<|> } }"),
    ("static_collections.java", "import java.util.*; class T { void m() { Collections.<|> } }"),
    ("static_math.java", "class T { void m() { Math.<|> } }"),
    ("static_status.java", "class T { void m() { Status.<|> } }"),
    (
        "static_own.java",
        "class Own { static int count; static void util() {} int inst; void im() {} void m() { Own.<|> } }",
    ),
    (
        "static_qualified.java",
        "class T { void m() { java.util.Collections.<|> } }",
    ),
    (
        "lambda_foreach.java",
        "import java.util.*; class T { void m(List<String> names) { names.forEach(n -> n.<|>); } }",
    ),
    (
        "lambda_entry.java",
        "import java.util.*; class T { void m(Map<String,Integer> m) { m.entrySet().stream().map(e -> e.<|>); } }",
    ),
    (
        "lambda_grouping.java",
        "import java.util.*; import java.util.stream.*; class T { void m(List<String> l) { l.stream().collect(Collectors.groupingBy(s -> s.<|>)); } }",
    ),
    (
        "lambda_filter.java",
        "import java.util.*; class T { void m(List<Customer> l) { l.stream().filter(c -> c.<|>); } }",
    ),
    (
        "collectors.java",
        "import java.util.*; import java.util.stream.*; class T { void m(List<String> l) { l.stream().collect(Collectors.<|>); } }",
    ),
    (
        "subclass_method_body.java",
        "import java.util.*; class BaseX { int f; } class SubX extends BaseX { SubX() { Arr<|>\n this.hashCode(); } }",
    ),
    ("type_arr.java", "class T { void m() { Arr<|> } }"),
    ("type_str.java", "class T { void m() { Str<|> } }"),
    (
        "type_str_wild.java",
        "import java.util.stream.*; class T { void m() { Str<|> } }",
    ),
    ("type_opt.java", "class T { void m() { Opt<|> } }"),
    ("type_coll.java", "class T { void m() { Coll<|> } }"),
    ("type_hash.java", "class T { void m() { Hash<|> } }"),
    ("type_hump.java", "class T { void m() { HM<|> } }"),
    ("type_loc.java", "class T { void m() { Loc<|> } }"),
    ("type_ent.java", "import java.util.*; class T { void m() { Ent<|> } }"),
    ("nested_static.java", "import java.util.*; class T { void m() { Map.Ent<|> } }"),
    ("hump_member.java", "class T { void m(Customer c) { c.gEm<|> } }"),
    (
        "rank_locals.java",
        "class T { int nameField; void m(String name, int count) { String nameX = \"\"; na<|> } }",
    ),
    ("rank_own_members.java", "class T { void m(Customer c) { c.<|> } }"),
    (
        "rank_expected.java",
        "class T { void m(Customer c) { String s = c.<|> } }",
    ),
    ("rank_deprecated.java", "import java.util.*; class T { void m(Date d) { d.<|> } }"),
    ("kw_return.java", "class T { int m() { ret<|> } }"),
    ("annotation_override.java", "class T { @Over<|> void m() { } }"),
    ("annotation_bare.java", "class T { @<|> void m() { } }"),
    ("import_pkg.java", "import java.util.<|>\nclass T { }"),
    ("import_partial_pkg.java", "import java.ut<|>\nclass T { }"),
    ("import_class.java", "import java.util.Arr<|>\nclass T { }"),
    ("import_static.java", "import static java.lang.Math.a<|>\nclass T { }"),
    (
        "new_arr.java",
        "import java.util.*; class T { void m() { List<String> x = new Arr<|> } }",
    ),
    ("new_generic.java", "import java.util.*; class T { void m() { Object x = new HashM<|> } }"),
    ("snippet_method.java", "class T { void m() { \"abc\".<|> } }"),
];

fn context() -> &'static SharedLspContext {
    static CONTEXT: OnceLock<SharedLspContext> = OnceLock::new();
    CONTEXT.get_or_init(|| {
        let mut test = LspTest::new();
        for (name, source) in SCENARIOS {
            test.add_file(name, source);
        }
        test.into_shared_context().expect("shared context")
    })
}

/// Completion items in the order a client would show them (`sortText`).
fn complete(scenario: &str) -> Vec<CompletionItem> {
    complete_with(scenario, true)
}

fn complete_with(scenario: &str, snippets: bool) -> Vec<CompletionItem> {
    let file = format!("{scenario}.java");
    let mut items = context()
        .completion_with_snippets(&file, "<|>", snippets)
        .unwrap_or_else(|e| panic!("{scenario}: {e}"));
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

fn position_of(items: &[CompletionItem], label: &str) -> usize {
    items
        .iter()
        .position(|i| i.label == label)
        .unwrap_or_else(|| panic!("`{label}` missing from {:?}", labels(items)))
}

fn assert_offers(scenario: &str, expected: &[&str]) -> Vec<CompletionItem> {
    let items = complete(scenario);
    for label in expected {
        assert!(
            items.iter().any(|i| i.label == *label),
            "{scenario}: expected `{label}`, got {:?}",
            labels(&items)
        );
    }
    items
}

fn assert_lacks(scenario: &str, items: &[CompletionItem], unwanted: &[&str]) {
    for label in unwanted {
        assert!(
            !items.iter().any(|i| i.label == *label),
            "{scenario}: `{label}` must not be offered, got {:?}",
            labels(items)
        );
    }
}

fn fqn(item: &CompletionItem) -> &str {
    item.detail.as_deref().unwrap_or("")
}

fn assert_no_internal_or_duplicate_types(scenario: &str, items: &[CompletionItem]) {
    let mut seen = HashSet::new();
    for item in items {
        if matches!(
            item.kind,
            Some(
                CompletionItemKind::CLASS
                    | CompletionItemKind::INTERFACE
                    | CompletionItemKind::ENUM
            )
        ) {
            let name = fqn(item);
            for internal in ["sun.", "jdk.internal.", "com.sun.org.apache."] {
                assert!(
                    !name.starts_with(internal),
                    "{scenario}: JDK-internal type offered: {name}"
                );
            }
            assert!(
                seen.insert((item.label.clone(), name.to_string())),
                "{scenario}: duplicate item {} ({name})",
                item.label
            );
            assert!(
                !name.contains("from workspace"),
                "{scenario}: type detail must be its FQN, got {name}"
            );
        }
    }
}

// ---------------------------------------------------------------- call results

#[test]
fn member_completion_after_a_stream_call() {
    let items = assert_offers("chain_stream", &["filter", "map", "collect", "forEach"]);
    assert_lacks("chain_stream", &items, &["charAt", "add", "get"]);
}

#[test]
fn member_completion_after_system_out() {
    let items = assert_offers("chain_sysout", &["println", "printf", "print"]);
    assert_lacks("chain_sysout", &items, &["filter", "charAt"]);
}

#[test]
fn member_completion_after_a_workspace_method_returning_a_library_type() {
    let items = assert_offers("chain_workspace", &["trim", "length", "charAt"]);
    assert_lacks("chain_workspace", &items, &["getEmail"]);
}

#[test]
fn member_completion_after_list_get_substitutes_the_element_type() {
    let items = assert_offers("chain_listget", &["charAt", "length"]);
    assert_lacks("chain_listget", &items, &["add", "stream"]);
}

#[test]
fn member_completion_after_a_generic_chain_through_optional() {
    let items = assert_offers("chain_optional", &["isPresent", "orElse", "get"]);
    assert_lacks("chain_optional", &items, &["charAt"]);
}

#[test]
fn lombok_builder_chain_completes_builder_then_built_type() {
    let items = assert_offers("chain_builder", &["name", "age", "build"]);
    assert_lacks("chain_builder", &items, &["builder"]);
    // The fluent setters come first; `build()` ends the chain.
    assert!(position_of(&items, "name") < position_of(&items, "build"));
    assert!(position_of(&items, "age") < position_of(&items, "build"));
    assert!(position_of(&items, "build") < position_of(&items, "hashCode"));
    assert_offers("chain_builder2", &["age", "build"]);
}

#[test]
fn member_completion_after_static_call_with_an_argument() {
    let items = assert_offers("chain_requirenonnull", &["getEmail", "age"]);
    assert_lacks("chain_requirenonnull", &items, &["trim"]);
}

#[test]
fn member_completion_after_new_expression_call() {
    assert_offers("chain_newsb", &["append", "reverse", "toString"]);
}

#[test]
fn member_completion_through_field_map_get_and_list_get() {
    let items = assert_offers("chain_field", &["getEmail", "age"]);
    assert_lacks("chain_field", &items, &["put", "size"]);
}

#[test]
fn member_completion_on_var_initialised_from_a_diamond_free_new() {
    assert_offers("chain_var", &["filter", "map"]);
}

#[test]
fn member_completion_with_a_typed_prefix_after_a_call_result() {
    let items = assert_offers("chain_prefix", &["filter"]);
    assert_lacks("chain_prefix", &items, &["map", "collect"]);
}

// ------------------------------------------------------------ static members

#[test]
fn static_members_after_a_workspace_type_name() {
    let items = assert_offers("static_customer", &["create", "MAX"]);
    assert_lacks("static_customer", &items, &["getEmail", "age"]);
}

#[test]
fn static_members_after_jdk_type_names() {
    assert_offers(
        "static_collections",
        &["sort", "emptyList", "unmodifiableList"],
    );
    assert_offers("static_math", &["abs", "max", "PI"]);
    assert_offers("static_qualified", &["sort", "emptyList"]);
}

#[test]
fn enum_constants_and_statics_after_an_enum_name() {
    let items = assert_offers("static_status", &["ACTIVE", "INACTIVE", "parse", "values"]);
    let active = items.iter().find(|i| i.label == "ACTIVE").unwrap();
    assert_eq!(active.kind, Some(CompletionItemKind::ENUM_MEMBER));
}

#[test]
fn own_class_name_lists_statics_not_instance_members() {
    let items = assert_offers("static_own", &["count", "util"]);
    assert_lacks("static_own", &items, &["inst", "im", "m"]);
}

// ------------------------------------------------------------ lambda parameters

#[test]
fn lambda_parameter_of_foreach_is_the_element_type() {
    let items = assert_offers("lambda_foreach", &["charAt", "length"]);
    assert_lacks("lambda_foreach", &items, &["add", "forEach", "stream"]);
}

#[test]
fn lambda_parameter_of_map_over_entry_set_is_the_entry_type() {
    let items = assert_offers("lambda_entry", &["getKey", "getValue"]);
    assert_lacks("lambda_entry", &items, &["filter", "stream"]);
}

#[test]
fn lambda_parameter_inside_collectors_grouping_by_is_the_element_type() {
    let items = assert_offers("lambda_grouping", &["charAt", "length"]);
    assert_lacks("lambda_grouping", &items, &["filter", "groupingBy"]);
}

#[test]
fn lambda_parameter_of_filter_is_the_workspace_element_type() {
    let items = assert_offers("lambda_filter", &["getEmail", "age"]);
    assert_lacks("lambda_filter", &items, &["filter", "charAt"]);
}

#[test]
fn collectors_argument_position_offers_collectors_statics() {
    let items = assert_offers("collectors", &["toList", "groupingBy", "joining"]);
    assert_lacks("collectors", &items, &["filter", "map"]);
}

// ------------------------------------------------------------ type names

#[test]
fn type_prefix_finds_the_jdk_classes_a_user_means() {
    for (scenario, wanted) in [
        ("type_arr", vec!["ArrayList", "Arrays"]),
        ("type_str", vec!["String", "StringBuilder", "Stream"]),
        ("type_opt", vec!["Optional"]),
        ("type_coll", vec!["Collection", "Collections", "Collectors"]),
        ("type_hash", vec!["HashMap", "HashSet"]),
        ("type_loc", vec!["LocalDate", "LocalDateTime"]),
    ] {
        let items = complete(scenario);
        assert_no_internal_or_duplicate_types(scenario, &items);
        let top: Vec<&str> = labels(&items).into_iter().take(10).collect();
        for label in wanted {
            assert!(
                top.contains(&label),
                "{scenario}: `{label}` should be in the top 10, got {top:?}"
            );
        }
    }
}

#[test]
fn types_are_offered_in_a_method_of_a_class_with_supertypes() {
    // Not override/implement stubs and not the class's fields: the cursor is
    // in a method body even though the following statement makes the parser
    // read `Arr this.hashCode()` as a member header.
    let items = complete("subclass_method_body");
    assert!(
        items.iter().any(|i| i.label == "ArrayList"),
        "{:?}",
        labels(&items)
    );
    assert_lacks("subclass_method_body", &items, &["f"]);
    assert!(items
        .iter()
        .all(|i| i.label.to_lowercase().starts_with("arr")));
}

#[test]
fn type_items_are_labelled_with_package_and_kind() {
    let items = complete("type_arr");
    let list = items
        .iter()
        .find(|i| i.label == "ArrayList")
        .expect("ArrayList");
    assert_eq!(list.kind, Some(CompletionItemKind::CLASS));
    assert_eq!(fqn(list), "java.util.ArrayList");
    assert_eq!(
        list.label_details
            .as_ref()
            .and_then(|d| d.description.as_deref()),
        Some("java.util")
    );
    let items = complete("type_coll");
    let collection = items
        .iter()
        .find(|i| i.label == "Collection")
        .expect("Collection");
    assert_eq!(collection.kind, Some(CompletionItemKind::INTERFACE));
}

#[test]
fn frequently_used_jdk_classes_outrank_obscure_ones() {
    let items = complete("type_arr");
    assert!(position_of(&items, "ArrayList") < position_of(&items, "ArrayStoreException"));
    assert!(position_of(&items, "ArrayList") < position_of(&items, "ArrayBlockingQueue"));
    let items = complete("type_str");
    assert_eq!(items[0].label, "String");
    let items = complete("type_opt");
    assert_eq!(items[0].label, "Optional");
    // A wildcard import does not promote its whole package above `String`.
    let items = complete("type_str_wild");
    assert!(position_of(&items, "String") < position_of(&items, "StreamSupport"));
}

#[test]
fn type_search_has_no_jdk_internals_or_duplicates() {
    for scenario in [
        "type_arr",
        "type_str",
        "type_opt",
        "type_coll",
        "type_hash",
        "type_hump",
    ] {
        assert_no_internal_or_duplicate_types(scenario, &complete(scenario));
    }
}

#[test]
fn camel_hump_matches_types_and_members() {
    let items = complete("type_hump");
    assert_eq!(items[0].label, "HashMap", "got {:?}", labels(&items));
    let items = complete("hump_member");
    assert_eq!(labels(&items), vec!["getEmail"]);
    // A client that only prefix-filters on `filterText` must still see it.
    assert!(items[0].filter_text.as_deref().unwrap().starts_with("gEm"));
}

#[test]
fn nested_types_are_offered_and_imported_through_their_outer_type() {
    let items = complete("type_ent");
    let entry = items
        .iter()
        .find(|i| fqn(i) == "java.util.Map.Entry")
        .unwrap_or_else(|| panic!("Map.Entry missing from {:?}", labels(&items)));
    assert_eq!(entry.insert_text.as_deref(), Some("Map.Entry"));
    assert_eq!(
        entry
            .label_details
            .as_ref()
            .and_then(|d| d.description.as_deref()),
        Some("java.util.Map")
    );
}

#[test]
fn accepting_a_type_adds_its_import() {
    let items = complete("type_arr");
    let list = items.iter().find(|i| i.label == "ArrayList").unwrap();
    let edits = list.additional_text_edits.as_ref().expect("import edit");
    assert!(edits[0].new_text.contains("import java.util.ArrayList;"));
    let items = complete("type_str");
    let string = items.iter().find(|i| i.label == "String").unwrap();
    assert!(
        string.additional_text_edits.is_none(),
        "java.lang needs no import"
    );
}

// ------------------------------------------------------------ ranking

#[test]
fn locals_and_parameters_rank_before_fields_and_keywords() {
    let items = complete("rank_locals");
    assert!(position_of(&items, "name") < position_of(&items, "nameField"));
    assert!(position_of(&items, "nameField") < position_of(&items, "native"));
    assert!(position_of(&items, "name") < position_of(&items, "NavigableMap"));
}

#[test]
fn own_members_rank_before_inherited_object_methods() {
    let items = complete("rank_own_members");
    let own = position_of(&items, "getEmail");
    for object_method in ["getClass", "hashCode", "notify", "wait"] {
        assert!(
            own < position_of(&items, object_method),
            "getEmail should precede {object_method}: {:?}",
            labels(&items)
        );
    }
}

#[test]
fn expected_type_match_ranks_first() {
    let items = complete("rank_expected");
    // `String s = c.` - getEmail returns String, age returns int.
    assert!(position_of(&items, "getEmail") < position_of(&items, "age"));
    assert_eq!(items[0].label, "getEmail");
}

#[test]
fn deprecated_members_are_tagged_and_last() {
    let items = complete("rank_deprecated");
    let get_year = items
        .iter()
        .find(|i| i.label == "getYear")
        .expect("getYear");
    assert!(get_year
        .tags
        .as_ref()
        .is_some_and(|tags| tags.contains(&CompletionItemTag::DEPRECATED)));
    let last_live = items
        .iter()
        .rposition(|i| i.tags.is_none())
        .expect("non-deprecated items");
    let first_deprecated = items.iter().position(|i| i.tags.is_some()).unwrap();
    assert!(first_deprecated > last_live, "{:?}", labels(&items));
}

// ------------------------------------------------------------ keywords, annotations, imports

#[test]
fn keyword_completion_ranks_return_first() {
    let items = complete("kw_return");
    assert_eq!(items[0].label, "return");
    assert_eq!(items[0].kind, Some(CompletionItemKind::KEYWORD));
}

#[test]
fn annotation_prefix_completes_annotation_types_only() {
    let items = complete("annotation_override");
    assert_eq!(items[0].label, "Override");
    assert!(items
        .iter()
        .all(|i| fqn(i).contains("Override") || i.label.starts_with("Over")));
    assert_lacks(
        "annotation_override",
        &items,
        &["OverlappingFileLockException"],
    );
    let items = assert_offers(
        "annotation_bare",
        &["Override", "Deprecated", "SuppressWarnings"],
    );
    assert_lacks("annotation_bare", &items, &["String", "return"]);
}

#[test]
fn import_statement_completes_packages_and_classes() {
    let items = assert_offers(
        "import_pkg",
        &["ArrayList", "List", "concurrent", "function", "*"],
    );
    let concurrent = items.iter().find(|i| i.label == "concurrent").unwrap();
    assert_eq!(concurrent.kind, Some(CompletionItemKind::MODULE));
    let list = items.iter().find(|i| i.label == "List").unwrap();
    assert_eq!(list.kind, Some(CompletionItemKind::INTERFACE));
    assert_lacks("import_pkg", &items, &["ArrayPrefixHelpers", "java"]);
    let items = assert_offers("import_partial_pkg", &["util"]);
    assert_lacks("import_partial_pkg", &items, &["io"]);
    let items = assert_offers("import_class", &["ArrayList", "Arrays", "ArrayDeque"]);
    assert_lacks("import_class", &items, &["List", "ArrayPrefixHelpers"]);
    let items = assert_offers("import_static", &["abs", "acos"]);
    assert_lacks("import_static", &items, &["max"]);
}

// ------------------------------------------------------------ constructors and snippets

#[test]
fn new_completes_constructors_with_the_diamond_and_import() {
    let items = complete("new_arr");
    let list = items
        .iter()
        .find(|i| i.label == "ArrayList<>()")
        .unwrap_or_else(|| panic!("ArrayList<>() missing from {:?}", labels(&items)));
    assert_eq!(list.kind, Some(CompletionItemKind::CONSTRUCTOR));
    assert!(
        list.additional_text_edits.is_none(),
        "java.util.* is imported"
    );
    assert!(position_of(&items, "ArrayList<>()") < position_of(&items, "ArrayStoreException()"));
    assert_lacks("new_arr", &items, &["Arrays()", "Array()"]);
    let items = complete("new_generic");
    assert!(items.iter().any(|i| i.label.starts_with("HashMap<>(")));
}

#[test]
fn method_completion_inserts_argument_placeholders_only_for_snippet_clients() {
    let items = complete("snippet_method");
    let char_at = items.iter().find(|i| i.label == "charAt").expect("charAt");
    assert_eq!(char_at.insert_text.as_deref(), Some("charAt(${1:index})$0"));
    assert_eq!(
        char_at.insert_text_format,
        Some(tower_lsp::lsp_types::InsertTextFormat::SNIPPET)
    );
    let length = items.iter().find(|i| i.label == "length").expect("length");
    assert_eq!(length.insert_text.as_deref(), Some("length()"));
    assert_eq!(
        char_at
            .label_details
            .as_ref()
            .and_then(|d| d.detail.as_deref()),
        Some("(int index)")
    );
    assert_eq!(
        char_at
            .label_details
            .as_ref()
            .and_then(|d| d.description.as_deref()),
        Some("char")
    );

    // A client without snippet support gets text it can insert verbatim.
    let plain = complete_with("snippet_method", false);
    let char_at = plain.iter().find(|i| i.label == "charAt").unwrap();
    assert_eq!(char_at.insert_text.as_deref(), Some("charAt()"));
    assert_eq!(
        char_at.insert_text_format,
        Some(tower_lsp::lsp_types::InsertTextFormat::PLAIN_TEXT)
    );
    for item in &plain {
        let text = item.insert_text.as_deref().unwrap_or(&item.label);
        assert!(
            !text.contains("${") && !text.contains("$0"),
            "snippet syntax leaked to a plain client: {:?} for {}",
            text,
            item.label
        );
    }
}
