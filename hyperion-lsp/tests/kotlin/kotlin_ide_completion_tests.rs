// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin completion as an IDE user meets it.

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::{CompletionItem, TextEdit};

fn complete(files: &[(&str, &str)], file: &str) -> Vec<CompletionItem> {
    let mut test = LspTest::new();
    for (path, text) in files {
        test.add_file(path, text);
    }
    test.completion(file, "<|>").expect("completion")
}

fn import_edit(items: &[CompletionItem], label: &str) -> TextEdit {
    let item = items
        .iter()
        .find(|item| item.label == label)
        .unwrap_or_else(|| panic!("no completion item {label:?}"));
    item.additional_text_edits
        .as_ref()
        .and_then(|edits| edits.first().cloned())
        .unwrap_or_else(|| panic!("{label:?} carries no auto-import edit"))
}

#[test]
fn auto_import_goes_after_the_package_declaration_without_a_semicolon() {
    let items = complete(
        &[(
            "app/Main.kt",
            "package app\n\nclass Main {\n    val x = BigDec<|>\n}\n",
        )],
        "app/Main.kt",
    );
    let edit = import_edit(&items, "BigDecimal");
    assert_eq!(
        edit.range.start.line, 2,
        "after `package app` and its blank line: {edit:?}"
    );
    assert_eq!(edit.new_text, "import java.math.BigDecimal\n\n");
}

#[test]
fn auto_import_is_sorted_among_existing_imports() {
    let items = complete(
        &[(
            "app/Main.kt",
            "package app\n\nimport java.io.File\nimport java.util.UUID\n\nclass Main {\n    val x = BigDec<|>\n}\n",
        )],
        "app/Main.kt",
    );
    let edit = import_edit(&items, "BigDecimal");
    assert_eq!(
        edit.range.start.line, 3,
        "between java.io.File and java.util.UUID: {edit:?}"
    );
    assert_eq!(edit.new_text, "import java.math.BigDecimal\n");
}

#[test]
fn auto_import_after_the_last_import_when_it_sorts_last() {
    let items = complete(
        &[(
            "app/Main.kt",
            "package app\n\nimport java.io.File\n\nclass Main {\n    val x = BigDec<|>\n}\n",
        )],
        "app/Main.kt",
    );
    let edit = import_edit(&items, "BigDecimal");
    assert_eq!(edit.range.start.line, 3, "{edit:?}");
    assert_eq!(edit.new_text, "import java.math.BigDecimal\n");
}

fn labels(items: &[CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.clone()).collect()
}

fn assert_offers(items: &[CompletionItem], expected: &[&str]) {
    let labels = labels(items);
    for name in expected {
        assert!(
            labels.iter().any(|label| label == name),
            "expected {name:?} among {} items: {:?}",
            labels.len(),
            &labels[..labels.len().min(60)]
        );
    }
}

fn assert_hides(items: &[CompletionItem], hidden: &[&str]) {
    let labels = labels(items);
    for name in hidden {
        assert!(
            !labels.iter().any(|label| label == name),
            "did not expect {name:?} among {:?}",
            &labels[..labels.len().min(60)]
        );
    }
}

#[test]
fn string_literal_receiver_offers_kotlin_string_members_and_extensions() {
    let items = complete(&[("Main.kt", "fun f() {\n    \"x\".<|>\n}\n")], "Main.kt");
    assert_offers(
        &items,
        &["length", "uppercase", "trim", "substring", "toInt", "let"],
    );
    assert_hides(&items, &["getBytes", "size", "getClass"]);
}

#[test]
fn list_local_offers_size_and_iterable_extensions() {
    let items = complete(
        &[(
            "Main.kt",
            "fun f() {\n    val shapes = listOf(1, 2)\n    shapes.<|>\n}\n",
        )],
        "Main.kt",
    );
    assert_offers(
        &items,
        &["size", "first", "map", "filter", "forEach", "isEmpty"],
    );
    assert_hides(&items, &["length", "stream"]);
}

#[test]
fn chained_call_receiver_is_inferred_from_the_call() {
    let items = complete(
        &[(
            "Main.kt",
            "fun f(items: List<String>) {\n    items.first().<|>\n}\n",
        )],
        "Main.kt",
    );
    assert_offers(&items, &["length", "uppercase"]);
}

#[test]
fn java_getters_are_kotlin_properties_and_object_noise_is_hidden() {
    let items = complete(
        &[
            (
                "core/Circle.java",
                "package demo.core;\n\npublic class Circle {\n    private final double r;\n    public Circle(double r) { this.r = r; }\n    public double getRadius() { return r; }\n    public boolean isRound() { return true; }\n    public void setRadius(double r2) {}\n    public String describe() { return \"c\"; }\n}\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nimport demo.core.Circle\n\nfun f(c: Circle) {\n    c.<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_offers(
        &items,
        &["radius", "isRound", "describe", "toString", "let"],
    );
    assert_hides(
        &items,
        &["getRadius", "setRadius", "getClass", "wait", "notify"],
    );
}

#[test]
fn kotlin_class_members_are_properties_and_functions_not_accessors() {
    let items = complete(
        &[
            (
                "billing/Invoice.kt",
                "package billing\n\nclass Invoice(val id: String) {\n    var note: String? = null\n    fun total(): Int = 1\n}\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nimport billing.Invoice\n\nfun f(inv: Invoice) {\n    inv.<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_offers(&items, &["id", "note", "total"]);
    assert_hides(&items, &["getId", "getNote", "setNote", "getClass"]);
}

#[test]
fn project_extensions_are_offered_with_their_import() {
    let items = complete(
        &[
            (
                "util/Ext.kt",
                "package util\n\nfun String.shout(): String = uppercase()\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nfun f(s: String) {\n    s.<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_offers(&items, &["shout"]);
    let edit = import_edit(&items, "shout");
    assert_eq!(edit.new_text, "import util.shout\n\n");
}

#[test]
fn type_name_receiver_offers_companion_members_not_instance_members() {
    let items = complete(
        &[
            (
                "money/Money.kt",
                "package money\n\nclass Money(val v: Int) {\n    fun instanceOnly() = v\n    companion object {\n        val ZERO = Money(0)\n        fun of(v: Int) = Money(v)\n    }\n}\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nimport money.Money\n\nfun f() {\n    Money.<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_offers(&items, &["ZERO", "of"]);
    assert_hides(&items, &["instanceOnly", "v"]);
}

#[test]
fn enum_entries_are_offered_after_the_enum_name() {
    let items = complete(
        &[
            (
                "model/Kind.kt",
                "package model\n\nenum class Kind { PRODUCT, SERVICE }\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nimport model.Kind\n\nfun f() {\n    Kind.<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_offers(&items, &["PRODUCT", "SERVICE"]);
}

#[test]
fn constructor_property_receiver_offers_the_members_of_its_type() {
    let items = complete(
        &[
            (
                "billing/Service.kt",
                "package billing\n\nclass Service {\n    fun create() = 1\n}\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nimport billing.Service\n\nclass Use(private val service: Service) {\n    fun f() {\n        service.<|>\n    }\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_offers(&items, &["create"]);
}

#[test]
fn read_only_kotlin_list_has_no_mutators_and_rows_are_not_repeated() {
    let items = complete(
        &[(
            "app/Use.kt",
            "package app\n\nclass Use(val names: List<String>) {\n    fun f() {\n        names.<|>\n    }\n}\n",
        )],
        "app/Use.kt",
    );
    assert_offers(&items, &["size", "get", "map", "first"]);
    assert_hides(&items, &["add", "addAll", "removeAt", "addFirst"]);
    let mut rows: Vec<(String, Option<String>)> = items
        .iter()
        .map(|item| {
            (
                item.label.clone(),
                item.label_details.as_ref().and_then(|d| d.detail.clone()),
            )
        })
        .collect();
    let total = rows.len();
    rows.sort();
    rows.dedup();
    assert_eq!(rows.len(), total, "repeated rows for the same member");
}

#[test]
fn classes_declared_after_an_enum_do_not_inherit_enum_members() {
    let items = complete(
        &[
            (
                "billing/Model.kt",
                "package billing\n\nenum class Kind { A }\n\nclass Invoice(val id: String) {\n    fun total() = 1\n}\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nimport billing.Invoice\n\nfun f(inv: Invoice) {\n    inv.<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_offers(&items, &["id", "total"]);
    assert_hides(
        &items,
        &["ordinal", "name", "describeConstable", "declaringClass"],
    );
}

/// Each detail offered under `label`, sorted.
fn details(items: &[CompletionItem], label: &str) -> Vec<String> {
    let mut details = items
        .iter()
        .filter(|item| item.label == label)
        .filter_map(|item| item.detail.clone())
        .collect::<Vec<_>>();
    details.sort();
    details
}

/// `actual` offers each of `labels` with the details `expected` does.
fn assert_same_details(actual: &[CompletionItem], expected: &[CompletionItem], labels: &[&str]) {
    for label in labels {
        let expected = details(expected, label);
        assert!(!expected.is_empty(), "{label} is offered");
        assert_eq!(details(actual, label), expected, "{label}");
    }
}

/// Members of a parameter declared `<declared>`, with `aliases` declared in
/// the same package.
fn members_of_declared(aliases: &str, declared: &str) -> Vec<CompletionItem> {
    complete(
        &[
            ("app/Aliases.kt", &format!("package app\n\n{aliases}")),
            (
                "app/Use.kt",
                &format!("package app\n\nfun f(roster: {declared}) {{\n    roster.<|>\n}}\n"),
            ),
        ],
        "app/Use.kt",
    )
}

/// A typealias to a generic class with concrete type arguments,
/// used as a declared type, keeps them: the receiver's members are those of
/// the class with the alias's arguments, as when the class is written out.
#[test]
fn typealias_receiver_keeps_the_type_arguments_of_its_expansion() {
    let aliases = "typealias Names = java.util.ArrayList<String>\n\
                   typealias Pairs = java.util.HashMap<String, Int>\n";
    let names = members_of_declared(aliases, "Names");
    assert_eq!(details(&names, "get"), ["String get(int index)"]);
    let written_out = members_of_declared(aliases, "java.util.ArrayList<String>");
    assert_same_details(&names, &written_out, &["get", "add", "first", "set"]);

    let pairs = members_of_declared(aliases, "Pairs");
    let written_out = members_of_declared(aliases, "java.util.HashMap<String, Int>");
    assert!(
        details(&pairs, "put")
            .iter()
            .any(|detail| detail.contains("String") && detail.contains("Int")),
        "both arguments: {:?}",
        details(&pairs, "put")
    );
    assert_same_details(&pairs, &written_out, &["get", "put", "keys", "values"]);
}

/// An alias whose expansion has no type arguments, to a generic
/// class written raw or to a class that takes none, offers what the class
/// written out offers, as before.
#[test]
fn typealias_receiver_without_expansion_arguments_is_unchanged() {
    let aliases = "typealias Raw = java.util.ArrayList\n\
                   typealias Text = java.lang.StringBuilder\n";
    let raw = members_of_declared(aliases, "Raw");
    let written_out = members_of_declared(aliases, "java.util.ArrayList");
    assert_eq!(details(&raw, "get"), ["Object get(int index)"]);
    assert_same_details(&raw, &written_out, &["get", "add", "first"]);

    let text = members_of_declared(aliases, "Text");
    let written_out = members_of_declared(aliases, "java.lang.StringBuilder");
    assert_offers(&text, &["append", "length", "reverse"]);
    assert_same_details(&text, &written_out, &["append", "length", "reverse"]);
}

/// A generic typealias binds its type parameter to the use's
/// argument wherever its expansion names it: `StringMap<Int>` under
/// `typealias StringMap<V> = java.util.HashMap<String, V>` offers the members
/// of `java.util.HashMap<String, Int>`, as does `Inverse<Int, String>`, whose
/// parameters the expansion takes in the other order.
#[test]
fn generic_typealias_receiver_binds_its_type_parameters_to_the_use_arguments() {
    let aliases = "typealias StringMap<V> = java.util.HashMap<String, V>\n\
                   typealias Inverse<K, V> = java.util.HashMap<V, K>\n";
    let written_out = members_of_declared(aliases, "java.util.HashMap<String, Int>");
    let string_to_int = details(&written_out, "put");
    assert!(
        string_to_int
            .iter()
            .any(|detail| detail.contains("String") && detail.contains("Int")),
        "{string_to_int:?}"
    );

    let map = members_of_declared(aliases, "StringMap<Int>");
    assert_eq!(details(&map, "put"), string_to_int);
    assert_same_details(&map, &written_out, &["get", "put", "keys", "values"]);

    let inverse = members_of_declared(aliases, "Inverse<Int, String>");
    assert_same_details(&inverse, &written_out, &["get", "put", "keys", "values"]);
}

/// A use argument that is itself generic keeps its own arguments
/// through the substitution: `StringMap<List<Int>>` under `typealias
/// StringMap<V> = java.util.HashMap<String, V>` is a `HashMap` of `String` to
/// `List<Int>`, not to the bare `List`, and offers what that map written out
/// offers. So does an argument nested in the expansion itself (`Grid<Int>`
/// under `typealias Grid<T> = java.util.ArrayList<java.util.List<T>>`), and
/// one nested twice.
#[test]
fn generic_typealias_receiver_keeps_the_arguments_of_a_nested_use_argument() {
    let aliases = "typealias StringMap<V> = java.util.HashMap<String, V>\n\
                   typealias Grid<T> = java.util.ArrayList<java.util.List<T>>\n";
    let map = members_of_declared(aliases, "StringMap<List<Int>>");
    assert_eq!(details(&map, "get"), ["List<Int> get(Object key)"]);
    assert_eq!(details(&map, "values"), ["Collection<List<Int>> values()"]);
    let written_out = members_of_declared(aliases, "java.util.HashMap<String, List<Int>>");
    assert_same_details(&map, &written_out, &["get", "put", "keys", "values"]);

    let grid = members_of_declared(aliases, "Grid<Int>");
    assert_eq!(details(&grid, "get"), ["List<Int> get(int index)"]);

    let nested_twice = members_of_declared(aliases, "StringMap<Map<String, List<Int>>>");
    assert_eq!(
        details(&nested_twice, "get"),
        ["Map<String, List<Int>> get(Object key)"]
    );
}

/// A use argument with no arguments of its own is substituted as
/// before, and a nested argument with no spelling (a star projection) leaves
/// its class bare, as before, rather than guessing one.
#[test]
fn generic_typealias_receiver_with_a_plain_use_argument_is_unchanged() {
    let aliases = "typealias StringMap<V> = java.util.HashMap<String, V>\n";
    let map = members_of_declared(aliases, "StringMap<Int>");
    assert_eq!(details(&map, "get"), ["Int get(Object key)"]);
    assert_eq!(details(&map, "put"), ["Int put(String key, Int value)"]);
    assert_eq!(details(&map, "values"), ["Collection<Int> values()"]);

    let projected = members_of_declared(aliases, "StringMap<List<*>>");
    assert_eq!(details(&projected, "get"), ["List get(Object key)"]);
}

/// Members after `<call>.`, in a function whose body first declares `<declared>`.
fn members_of_call_result(declared: &str, call: &str) -> Vec<CompletionItem> {
    members_of_call_result_importing("", declared, call)
}

/// [`members_of_call_result`], in a file whose package is followed by `imports`.
fn members_of_call_result_importing(
    imports: &str,
    declared: &str,
    call: &str,
) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!("package app\n\n{imports}fun f() {{\n    {declared}\n    {call}.<|>\n}}\n"),
        )],
        "app/Use.kt",
    )
}

/// A Java member called on a receiver with type arguments returns
/// its declared type with the receiver's arguments substituted for the
/// declaring class's type parameters: `roster.get(0)` on an `ArrayList` of
/// `String` is a `String`, not `E`, and `map.get(k)` on a `HashMap` of
/// `String` to `List<Int>` is a `List<Int>`.
#[test]
fn java_member_call_result_substitutes_the_receiver_type_arguments() {
    let string = members_of_call_result(
        "val roster: java.util.ArrayList<String> = java.util.ArrayList()",
        "roster.get(0)",
    );
    assert_offers(&string, &["length", "uppercase", "substring"]);
    assert_hides(&string, &["size", "add", "keys"]);

    let list = members_of_call_result(
        "val map: java.util.HashMap<String, List<Int>> = java.util.HashMap()",
        "map.get(\"k\")",
    );
    assert_offers(&list, &["size", "first", "map"]);
    assert_hides(&list, &["length", "uppercase", "keys"]);
}

/// Members after `<access>.`, in a function that holds `roster`, a
/// `java.util.ArrayList<String>`, and `names`, an `Array<String>`.
fn members_of_roster_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!(
                "package app\n\nfun f() {{\n    val roster: java.util.ArrayList<String> = java.util.ArrayList()\n    val names = arrayOf(\"a\")\n    {access}.<|>\n}}\n"
            ),
        )],
        "app/Use.kt",
    )
}

/// A call to a member whose overloads disagree about their type
/// takes the one overload its arguments fit. `ArrayList` declares `Object[]
/// toArray()` and `<T> T[] toArray(T[] a)`, and inherits `<T> T[]
/// toArray(IntFunction<T[]> generator)` from `Collection`: `roster.toArray()`
/// is an `Array<Any>`, and `roster.toArray(names)`, `names` an
/// `Array<String>`, takes `toArray(T[] a)` with `T` bound to `String`, so it
/// is an `Array<String>` and its elements offer String's members.
#[test]
fn same_arity_overload_is_chosen_by_the_argument_types() {
    for call in ["roster.toArray()", "roster.toArray(names)"] {
        let array = members_of_roster_access(call);
        assert_offers(&array, &["size", "get", "iterator", "copyOf"]);
        assert_hides(&array, &["add", "stream", "length"]);
    }

    let any = members_of_roster_access("roster.toArray().forEach { it");
    assert_offers(&any, &["equals", "hashCode", "let"]);
    assert_hides(&any, &["length", "uppercase", "substring"]);

    let string = members_of_roster_access("roster.toArray(names).forEach { it");
    assert_offers(&string, &["length", "uppercase", "substring"]);
}

/// Members after `<access>.`, in a function that holds `picker`, a
/// `lib.Picker` whose `pick` takes an `Object` and returns a `String`, or a
/// `CharSequence` and returns a `StringBuilder`, whose `either` takes a
/// `CharSequence` and returns a `String`, or a `java.io.Serializable` and
/// returns a `StringBuilder`, and whose `only` has one overload, and `names`,
/// an `Array<String>`.
fn members_of_picker_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[
            (
                "lib/Picker.java",
                "package lib;\n\npublic class Picker {\n    public String pick(Object value) { return \"\"; }\n    public StringBuilder pick(CharSequence text) { return new StringBuilder(); }\n    public String either(CharSequence text) { return \"\"; }\n    public StringBuilder either(java.io.Serializable value) { return new StringBuilder(); }\n    public StringBuilder only(Object value) { return new StringBuilder(); }\n}\n",
            ),
            (
                "app/Use.kt",
                &format!(
                    "package app\n\nfun f(picker: lib.Picker) {{\n    val names = arrayOf(\"a\")\n    {access}.<|>\n}}\n"
                ),
            ),
        ],
        "app/Use.kt",
    )
}

/// Arguments that more than one overload of a name can take, none
/// of them more specific than the others, and overloads disagreeing about
/// their type, leave the call with no type: `picker.either("x")` fits both
/// `either(CharSequence)` and `either(Serializable)`, neither of which
/// extends the other, and an argument of no known type, as `missing` is,
/// rules out no overload of `roster.toArray(missing)`. An array fits only
/// `pick(Object)`, so `picker.pick(names)` is a `String`. A name with one
/// overload keeps its type, whatever its argument: `picker.only("x")` is a
/// `StringBuilder`, and `roster.get(0)` a `String`.
#[test]
fn ambiguous_same_arity_overloads_give_no_type_and_a_single_overload_is_unchanged() {
    let ambiguous = members_of_picker_access("picker.either(\"x\")");
    assert_hides(&ambiguous, &["length", "uppercase", "append", "reverse"]);
    let unknown = members_of_roster_access("roster.toArray(missing)");
    assert_hides(&unknown, &["size", "copyOf", "add", "length"]);

    let string = members_of_picker_access("picker.pick(names)");
    assert_offers(&string, &["length", "uppercase", "substring"]);
    assert_hides(&string, &["append", "reverse"]);

    let only = members_of_picker_access("picker.only(\"x\")");
    assert_offers(&only, &["append", "reverse"]);
    let element = members_of_roster_access("roster.get(0)");
    assert_offers(&element, &["length", "uppercase", "substring"]);
}

/// Of several overloads an argument fits, the call takes the most
/// specific, the one whose parameters each extend the others': `"x"` fits
/// both `pick(Object)` and `pick(CharSequence)`, and `CharSequence` extends
/// `Object`, so `picker.pick("x")` is `pick(CharSequence)`'s `StringBuilder`;
/// an `Int` fits only `pick(Object)`, so `picker.pick(1)` is a `String`. An
/// overload the argument may not fit is not chosen: `missing` has no known
/// type, so `picker.pick(missing)` has none either.
#[test]
fn the_most_specific_of_several_fitting_overloads_types_the_call() {
    let builder = members_of_picker_access("picker.pick(\"x\")");
    assert_offers(&builder, &["append", "reverse"]);
    assert_hides(&builder, &["uppercase"]);

    let string = members_of_picker_access("picker.pick(1)");
    assert_offers(&string, &["length", "uppercase", "substring"]);
    assert_hides(&string, &["append", "reverse"]);

    let unknown = members_of_picker_access("picker.pick(missing)");
    assert_hides(&unknown, &["length", "uppercase", "append", "reverse"]);
}

/// Members after `<access>.`, in a function that holds `picker`, a `Chooser`
/// declared in the same file, whose `pick` takes an `Any` and returns an
/// `Int`, or a `CharSequence` and returns a `String`, whose `twin` takes an
/// `Any` or an `Int` and returns a `String` either way, and whose `only` has
/// one overload, and `heir`, a `Heir`, which extends `Chooser`.
fn members_of_chooser_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!(
                "package app\n\nopen class Chooser {{\n    fun pick(x: Any): Int = 1\n    fun pick(x: CharSequence): String = \"\"\n    fun twin(x: Any): String = \"\"\n    fun twin(x: Int): String = \"\"\n    fun only(x: Any): String = \"\"\n}}\n\nclass Heir : Chooser()\n\nfun f() {{\n    val picker = Chooser()\n    val heir = Heir()\n    {access}.<|>\n}}\n"
            ),
        )],
        "app/Use.kt",
    )
}

/// The index keeps one of the overloads a Kotlin source class
/// declares under one name, so a call to them is not typed from the one
/// kept: `picker.pick("x")` is `pick(CharSequence)`'s `String`, not
/// `pick(Any)`'s `Int`, and is left with no type until the index keeps every
/// overload. So is the call on a subclass that inherits them,
/// `heir.pick("x")`. A name with one overload keeps its type, and so do
/// overloads that declare the same type: `picker.only("x")` and
/// `picker.twin(1)`, on either receiver, are `String`s.
#[test]
fn a_source_declared_overloaded_member_call_is_not_typed_from_the_kept_overload() {
    for receiver in ["picker", "heir"] {
        let overloaded = members_of_chooser_access(&format!("{receiver}.pick(\"x\")"));
        assert_hides(
            &overloaded,
            &["toLong", "toByte", "inc", "dec", "length", "uppercase"],
        );

        for call in ["only(\"x\")", "twin(1)"] {
            let typed = members_of_chooser_access(&format!("{receiver}.{call}"));
            assert_offers(&typed, &["length", "uppercase", "substring"]);
        }
    }
}

/// Members after `<access>.`, in a function of a file that declares `Box`.
fn members_of_box_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!(
                "package app\n\nclass Box<T>(val item: T) {{\n    fun get(): T = item\n    fun <T> make(): T = TODO()\n}}\n\nfun f(box: Box<String>) {{\n    {access}.<|>\n}}\n"
            ),
        )],
        "app/Use.kt",
    )
}

/// The same holds for a Kotlin member, a function or a property,
/// whose declared type is a type parameter of its class.
#[test]
fn kotlin_member_call_result_substitutes_the_receiver_type_arguments() {
    for access in ["box.get()", "box.item"] {
        let items = members_of_box_access(access);
        assert_offers(&items, &["length", "uppercase", "substring"]);
    }
}

/// A raw receiver has no arguments to substitute, so the declared
/// type parameter is read as before: no member of `String`, `List` or the
/// receiver's class is guessed for `roster.get(0)`. Nor is a member's own type
/// parameter replaced by the class's argument of the same name.
#[test]
fn member_call_result_without_a_receiver_argument_is_not_substituted() {
    let raw = members_of_call_result(
        "val roster: java.util.ArrayList = java.util.ArrayList()",
        "roster.get(0)",
    );
    assert_hides(&raw, &["length", "uppercase", "substring", "size", "get"]);

    let own = members_of_box_access("box.make()");
    assert_hides(&own, &["length", "uppercase", "substring", "item"]);
}

/// Members after `<access>.`, with `table` a `Table<Int, String>` whose
/// members return types that take the class's type parameters as arguments.
fn members_of_table_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!(
                "package app\n\nimport java.util.ArrayList\nimport java.util.HashMap\n\nclass Table<K, V> {{\n    fun rows(): HashMap<K, ArrayList<V>> = HashMap()\n    fun <V> column(): ArrayList<V> = ArrayList()\n}}\n\nfun f(table: Table<Int, String>) {{\n    {access}.<|>\n}}\n"
            ),
        )],
        "app/Use.kt",
    )
}

/// A member whose declared type takes the class's type parameter as
/// an argument, at any depth, has the receiver's argument there:
/// `roster.iterator()` on an `ArrayList` of `String` is an `Iterator<String>`,
/// so its `next()` is a `String`; the inherited `roster.stream()` is a
/// `Stream<String>`, so `findFirst().get()` is one too; and `table.rows()` on a
/// `Table<Int, String>` declared `HashMap<K, ArrayList<V>>` is a map of `Int`
/// to `ArrayList<String>`.
#[test]
fn nested_type_parameter_in_a_member_result_is_substituted() {
    let declared = "val roster: java.util.ArrayList<String> = java.util.ArrayList()";
    for call in [
        "roster.iterator().next()",
        "roster.stream().findFirst().get()",
    ] {
        let items = members_of_call_result(declared, call);
        assert_offers(&items, &["length", "uppercase", "substring"]);
        assert_hides(&items, &["size", "add", "hasNext", "isPresent"]);
    }

    let row = members_of_table_access("table.rows().get(1)");
    assert_offers(&row, &["size", "add", "get"]);
    assert_hides(&row, &["length", "uppercase", "keys"]);
    let cell = members_of_table_access("table.rows().get(1).get(0)");
    assert_offers(&cell, &["length", "uppercase", "substring"]);
    assert_hides(&cell, &["size", "add", "keys"]);
}

/// With no argument to substitute, a nested type parameter is read
/// as before: on a raw receiver `roster.iterator().next()` offers no member of
/// `String`, and a member's own type parameter that shadows the class's
/// (`fun <V> column(): ArrayList<V>`) is not replaced by the receiver's
/// argument. A declared type that is the parameter itself, `roster.get(0)`,
/// is still substituted.
#[test]
fn nested_type_parameter_without_a_receiver_argument_is_not_substituted() {
    let raw = members_of_call_result(
        "val roster: java.util.ArrayList = java.util.ArrayList()",
        "roster.iterator().next()",
    );
    assert_hides(
        &raw,
        &["length", "uppercase", "substring", "size", "hasNext"],
    );

    let own = members_of_table_access("table.column().get(0)");
    assert_hides(&own, &["length", "uppercase", "substring", "size"]);

    let element = members_of_call_result(
        "val roster: java.util.ArrayList<String> = java.util.ArrayList()",
        "roster.get(0)",
    );
    assert_offers(&element, &["length", "uppercase", "substring"]);
    assert_hides(&element, &["size", "add", "keys"]);
}

/// A nested type argument written as a qualified name is that
/// class, with its own arguments: `table.pairs()` declared
/// `java.util.HashMap<K, java.util.ArrayList<V>>`, with no import, on a
/// `Pairs<Int, String>` is a map of `Int` to `ArrayList<String>`, so
/// `get(1).get(0)` is a `String`.
#[test]
fn qualified_nested_type_argument_in_a_member_result_is_substituted() {
    let members = |access: &str| {
        complete(
            &[(
                "app/Use.kt",
                &format!(
                    "package app\n\nclass Pairs<K, V> {{\n    fun pairs(): java.util.HashMap<K, java.util.ArrayList<V>> = java.util.HashMap()\n}}\n\nfun f(table: Pairs<Int, String>) {{\n    {access}.<|>\n}}\n"
                ),
            )],
            "app/Use.kt",
        )
    };
    let row = members("table.pairs().get(1)");
    assert_offers(&row, &["size", "add", "get"]);
    assert_hides(&row, &["length", "uppercase", "keys"]);
    let cell = members("table.pairs().get(1).get(0)");
    assert_offers(&cell, &["length", "uppercase", "substring"]);
    assert_hides(&cell, &["size", "add", "keys"]);
}

/// Members after `<access>.`, with `box` a `Box<String>` whose members
/// return types that project the class's type parameter.
fn members_of_projected_box_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!(
                "package app\n\nclass Box<T> {{\n    fun items(): java.util.ArrayList<out T> = java.util.ArrayList()\n    fun sink(): java.util.ArrayList<in T> = java.util.ArrayList()\n    fun all(): java.util.ArrayList<*> = java.util.ArrayList<String>()\n}}\n\nfun f(box: Box<String>) {{\n    {access}.<|>\n}}\n"
            ),
        )],
        "app/Use.kt",
    )
}

/// The receiver's argument replaces the class's type parameter
/// inside a projection: `box.items()` declared `java.util.ArrayList<out T>`
/// on a `Box<String>` is an `ArrayList<out String>`, so what it gives out,
/// `get(0)` and `iterator().next()`, is a `String`.
#[test]
fn out_projected_type_parameter_in_a_member_result_is_substituted() {
    for access in ["box.items().get(0)", "box.items().iterator().next()"] {
        let items = members_of_projected_box_access(access);
        assert_offers(&items, &["length", "uppercase", "substring"]);
        assert_hides(&items, &["size", "add", "hasNext"]);
    }
    let list = members_of_projected_box_access("box.items()");
    assert_offers(&list, &["size", "get", "iterator"]);
    assert_eq!(details(&list, "get"), ["String get(int index)"]);
}

/// What an `in`-projected or star-projected list gives out is not
/// its argument, so `box.sink().get(0)` and `box.all().get(0)` offer no
/// member of `String`.
#[test]
fn in_or_star_projected_member_result_gives_no_argument_type() {
    for access in ["box.sink().get(0)", "box.all().get(0)"] {
        let items = members_of_projected_box_access(access);
        assert_hides(&items, &["length", "uppercase", "substring"]);
    }
}

/// A generic typealias used with more or fewer arguments than it
/// has type parameters gives the receiver no type: no member of the class it
/// expands to is offered, with arguments matched by position or not at all.
#[test]
fn generic_typealias_receiver_with_unmatched_arguments_has_no_members() {
    let aliases = "typealias StringMap<V> = java.util.HashMap<String, V>\n";
    assert!(!details(&members_of_declared(aliases, "StringMap<Int>"), "put").is_empty());
    for declared in ["StringMap<Int, String>", "StringMap"] {
        let items = members_of_declared(aliases, declared);
        for member in ["put", "get", "keys"] {
            assert!(
                details(&items, member).is_empty(),
                "{declared} offers {member}: {:?}",
                details(&items, member)
            );
        }
    }
}

/// A member the receiver's class inherits, transitively, has a
/// type as well as one it declares: `roster.stream()` on a
/// `java.util.ArrayList<String>` calls the default method of
/// `java.util.Collection`, reached through `AbstractList` and `List`, and
/// offers the members of a `Stream`.
#[test]
fn inherited_jdk_member_call_result_offers_the_members_of_its_type() {
    let stream = members_of_call_result(
        "val roster: java.util.ArrayList<String> = java.util.ArrayList()",
        "roster.stream()",
    );
    assert_offers(&stream, &["filter", "map", "collect", "anyMatch"]);
    assert_hides(&stream, &["size", "get", "length"]);
}

/// An inherited property has its type too, whether it is a Java
/// getter read as a property (`e.message` on an `IllegalStateException` is
/// `Throwable.getMessage()`) or a Kotlin property or function of a Kotlin
/// superclass.
#[test]
fn inherited_property_and_kotlin_member_offer_the_members_of_their_type() {
    let message = members_of_call_result(
        "val e: java.lang.IllegalStateException = java.lang.IllegalStateException()",
        "e.message",
    );
    assert_offers(&message, &["length", "uppercase", "substring"]);

    let zoo = "package zoo\n\nopen class Animal(val name: String) {\n    fun sound(): StringBuilder = StringBuilder()\n}\n\nclass Dog : Animal(\"d\")\n";
    for (access, expected) in [
        ("dog.name", ["length", "uppercase"]),
        ("dog.sound()", ["append", "reverse"]),
    ] {
        let items = complete(
            &[
                ("zoo/Animal.kt", zoo),
                (
                    "app/Use.kt",
                    &format!("package app\n\nfun f(dog: zoo.Dog) {{\n    {access}.<|>\n}}\n"),
                ),
            ],
            "app/Use.kt",
        );
        assert_offers(&items, &expected);
    }
}

/// Members after `<access>.`, with `leaf` a `lib.Leaf`: Java classes
/// `Base`, `Middle extends Base` and `Leaf extends Middle`, where `Middle`
/// overrides `Object value()` of `Base` with `String value()`, and `Leaf`
/// declares `own()`.
fn members_of_leaf_access(access: &str) -> Vec<CompletionItem> {
    members_of_leaf_access_declared("", "lib.Leaf", access)
}

/// [`members_of_leaf_access`], with `leaf` declared `<declared>` in a file
/// whose package is followed by `imports`.
fn members_of_leaf_access_declared(
    imports: &str,
    declared: &str,
    access: &str,
) -> Vec<CompletionItem> {
    complete(
        &[
            (
                "lib/Base.java",
                "package lib;\n\npublic class Base {\n    public Object value() { return null; }\n    public Base copy() { return this; }\n}\n",
            ),
            (
                "lib/Middle.java",
                "package lib;\n\npublic class Middle extends Base {\n    @Override\n    public String value() { return \"\"; }\n}\n",
            ),
            (
                "lib/Leaf.java",
                "package lib;\n\npublic class Leaf extends Middle {\n    public StringBuilder own() { return new StringBuilder(); }\n}\n",
            ),
            (
                "app/Use.kt",
                &format!(
                    "package app\n\n{imports}fun f(leaf: {declared}) {{\n    {access}.<|>\n}}\n"
                ),
            ),
        ],
        "app/Use.kt",
    )
}

/// A member declared on the receiver's own class keeps its type,
/// and a name declared at two levels of the hierarchy is listed once, with
/// the nearer declaration's type: `leaf.value()` is the `String` of
/// `Middle`, not the `Object` of `Base`. A JDK class likewise: `ArrayList`
/// declares `spliterator()`, which `Collection` declares too, and inherits
/// `stream()`.
#[test]
fn own_members_keep_their_type_and_an_overridden_member_is_listed_once() {
    let leaf = members_of_leaf_access("leaf");
    assert_eq!(details(&leaf, "value"), ["String value()"]);
    assert_eq!(details(&leaf, "own"), ["StringBuilder own()"]);
    assert_offers(
        &members_of_leaf_access("leaf.own()"),
        &["append", "reverse"],
    );
    assert_offers(
        &members_of_leaf_access("leaf.value()"),
        &["length", "uppercase"],
    );
    assert_offers(&members_of_leaf_access("leaf.copy()"), &["value", "copy"]);

    let declared = "val roster: java.util.ArrayList<String> = java.util.ArrayList()";
    let roster = members_of_call_result(declared, "roster");
    assert_eq!(details(&roster, "stream"), ["Stream<String> stream()"]);
    assert_eq!(
        details(&roster, "spliterator"),
        ["Spliterator<String> spliterator()"]
    );
    let spliterator = members_of_call_result(declared, "roster.spliterator()");
    assert_offers(&spliterator, &["tryAdvance", "estimateSize"]);
    assert_hides(&spliterator, &["filter", "size"]);
}

/// A receiver whose declared type is a simple name the file
/// imports has the call results its qualified spelling gives it, with its
/// type arguments: under `import java.util.ArrayList`, `roster.get(0)` on an
/// `ArrayList<String>` is a `String`, and the inherited `roster.stream()` a
/// `Stream`. A source class likewise: `leaf.own()` under `import lib.Leaf`.
#[test]
fn imported_receiver_type_has_its_member_call_results() {
    let imports = "import java.util.ArrayList\n\n";
    let declared = "val roster: ArrayList<String> = ArrayList()";
    let string = members_of_call_result_importing(imports, declared, "roster.get(0)");
    assert_offers(&string, &["length", "uppercase", "substring"]);
    assert_hides(&string, &["size", "add", "keys"]);

    let stream = members_of_call_result_importing(imports, declared, "roster.stream()");
    assert_offers(&stream, &["filter", "map", "collect"]);
    assert_hides(&stream, &["size", "get", "length"]);

    let own = members_of_leaf_access_declared("import lib.Leaf\n\n", "Leaf", "leaf.own()");
    assert_offers(&own, &["append", "reverse"]);
}

/// A qualified receiver type keeps its call results with the
/// import present, and a simple name the file does not import gives none,
/// although its class is indexed: `LinkedList` with no `import
/// java.util.LinkedList`, and `Leaf` with no `import lib.Leaf`.
#[test]
fn qualified_receiver_type_keeps_its_call_results_and_an_unimported_one_has_none() {
    let qualified = members_of_call_result_importing(
        "import java.util.ArrayList\n\n",
        "val roster: java.util.ArrayList<String> = java.util.ArrayList()",
        "roster.get(0)",
    );
    assert_offers(&qualified, &["length", "uppercase", "substring"]);

    let unimported = members_of_call_result(
        "val roster: LinkedList<String> = LinkedList()",
        "roster.get(0)",
    );
    assert_hides(&unimported, &["length", "uppercase", "substring", "size"]);

    let own = members_of_leaf_access_declared("", "Leaf", "leaf.own()");
    assert_hides(&own, &["append", "reverse", "length"]);
}

/// A simple name that binds a typealias is not read as the class
/// it expands to with the use's arguments, which are the alias's, not the
/// class's: `inverse.get(1)` on an `Inverse<Int, String>` under `typealias
/// Inverse<K, V> = java.util.HashMap<V, K>` is not a `String`.
#[test]
fn aliased_receiver_type_does_not_pass_its_arguments_to_the_class_by_position() {
    let items = complete(
        &[
            (
                "app/Aliases.kt",
                "package app\n\ntypealias Inverse<K, V> = java.util.HashMap<V, K>\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nfun f(inverse: Inverse<Int, String>) {\n    inverse.get(1).<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_hides(&items, &["length", "uppercase", "substring"]);
}

/// A value with no declared type, initialized by a constructor
/// call with explicit type arguments, has them as its type's arguments, as a
/// declared type does: with `import java.util.ArrayList`, `val roster =
/// ArrayList<String>()` offers String's members on `roster.get(0).`, Stream's
/// on `roster.stream().`, and `String get(int index)` on `roster.`. The
/// qualified spelling does the same, and so does the simple name with no
/// import line, through the default import of kotlin.collections.ArrayList.
#[test]
fn constructor_call_type_arguments_are_the_value_type_arguments() {
    let declared = members_of_call_result_importing(
        "import java.util.ArrayList\n\n",
        "val roster: ArrayList<String> = ArrayList()",
        "roster",
    );
    for (imports, initialized) in [
        (
            "import java.util.ArrayList\n\n",
            "val roster = ArrayList<String>()",
        ),
        ("", "val roster = java.util.ArrayList<String>()"),
        ("", "val roster = ArrayList<String>()"),
    ] {
        let string = members_of_call_result_importing(imports, initialized, "roster.get(0)");
        assert_offers(&string, &["length", "uppercase", "substring"]);
        assert_hides(&string, &["size", "add", "keys"]);

        let stream = members_of_call_result_importing(imports, initialized, "roster.stream()");
        assert_offers(&stream, &["filter", "map", "collect"]);

        let own = members_of_call_result_importing(imports, initialized, "roster");
        assert_eq!(
            details(&own, "get"),
            ["String get(int index)"],
            "{initialized}"
        );
        assert_same_details(&own, &declared, &["get", "add", "first"]);
    }

    let list = members_of_call_result(
        "val map = java.util.HashMap<String, List<Int>>()",
        "map.get(\"k\")",
    );
    assert_offers(&list, &["size", "first", "map"]);
    assert_hides(&list, &["length", "uppercase", "keys"]);
}

/// A constructor call that writes no type arguments gives the
/// value none: `val roster = ArrayList()` is not guessed to hold `String`s,
/// so `roster.get(0).` offers no String member and `roster.`'s `get` is not
/// `String get(int index)`. Nor does a typealias pass the call's arguments to
/// the class by position: `Inverse<Int, String>()` under `typealias
/// Inverse<K, V> = java.util.HashMap<V, K>` does not make `inverse.get(1)` a
/// `String`.
#[test]
fn constructor_call_without_type_arguments_gives_the_value_none() {
    let imports = "import java.util.ArrayList\n\n";
    let raw = "val roster = ArrayList()";
    let string = members_of_call_result_importing(imports, raw, "roster.get(0)");
    assert_hides(&string, &["length", "uppercase", "substring"]);
    let own = members_of_call_result_importing(imports, raw, "roster");
    assert!(
        details(&own, "get")
            .iter()
            .all(|detail| !detail.contains("String")),
        "{:?}",
        details(&own, "get")
    );

    let aliased = complete(
        &[
            (
                "app/Aliases.kt",
                "package app\n\ntypealias Inverse<K, V> = java.util.HashMap<V, K>\n",
            ),
            (
                "app/Use.kt",
                "package app\n\nfun f() {\n    val inverse = Inverse<Int, String>()\n    inverse.get(1).<|>\n}\n",
            ),
        ],
        "app/Use.kt",
    );
    assert_hides(&aliased, &["length", "uppercase", "substring"]);
}

/// A constructor call through a typealias whose expansion gives
/// its class concrete type arguments has the expansion as its type: under
/// `typealias Names = java.util.ArrayList<String>`, `val roster = Names()` is
/// an `ArrayList<String>`, so `roster.get(0).` offers String's members and
/// `roster.`'s `get` is `String get(int index)`. The qualified call
/// `app.Names()` does the same, and so does the alias as a declared type.
#[test]
fn typealias_constructor_call_has_the_expansion_type_arguments() {
    let alias = "typealias Names = java.util.ArrayList<String>\n\n";
    for initialized in [
        "val roster = Names()",
        "val roster = app.Names()",
        "val roster: Names = Names()",
    ] {
        let string = members_of_call_result_importing(alias, initialized, "roster.get(0)");
        assert_offers(&string, &["length", "uppercase", "substring"]);
        assert_hides(&string, &["size", "add", "keys"]);

        let own = members_of_call_result_importing(alias, initialized, "roster");
        assert_eq!(
            details(&own, "get"),
            ["String get(int index)"],
            "{initialized}"
        );
    }
}

/// A typealias constructor call gives the value no type arguments
/// that its expansion does not fix, written as a simple or a qualified name:
/// not through an alias whose expansion has none (`typealias Raw =
/// java.util.ArrayList`), whose call is the raw class, and not through a
/// generic alias called with none (`StringMap()` under `typealias
/// StringMap<V> = java.util.HashMap<String, V>`), whose `String` is not
/// passed to the class alone. The qualified generic call is the raw class.
#[test]
fn typealias_constructor_call_without_fixed_arguments_gives_the_value_none() {
    let no_string = |items: &[CompletionItem], labels: &[&str]| {
        for label in labels {
            let details = details(items, label);
            assert!(
                details.iter().all(|detail| !detail.contains("String")),
                "{label}: {details:?}"
            );
        }
    };

    let raw_alias = "typealias Raw = java.util.ArrayList\n\n";
    for initialized in ["val roster = Raw()", "val roster = app.Raw()"] {
        let string = members_of_call_result_importing(raw_alias, initialized, "roster.get(0)");
        assert_hides(&string, &["length", "uppercase", "substring"]);
        let own = members_of_call_result_importing(raw_alias, initialized, "roster");
        assert_offers(&own, &["get", "add", "size"]);
        no_string(&own, &["get", "add"]);
    }

    let generic_alias = "typealias StringMap<V> = java.util.HashMap<String, V>\n\n";
    let map = members_of_call_result_importing(generic_alias, "val map = StringMap()", "map");
    no_string(&map, &["get", "put"]);
    let qualified =
        members_of_call_result_importing(generic_alias, "val map = app.StringMap()", "map");
    assert_offers(&qualified, &["get", "put", "keys"]);
    no_string(&qualified, &["get", "put"]);
}

/// A value whose type is a typealias with no type parameters,
/// expanding to a class with no arguments, has that class as its type, so a
/// member call on it has a result: under `typealias Text =
/// java.lang.StringBuilder`, `text.append("x").` offers StringBuilder's
/// members, whether `text` is declared `Text` or initialized by `Text()`, as
/// when the class is written out. A generic alias used with no arguments
/// still gives no type: `map.keySet().` under `typealias StringMap<V> =
/// java.util.HashMap<String, V>` offers nothing.
#[test]
fn non_generic_typealias_value_has_member_call_results() {
    let alias = "typealias Text = java.lang.StringBuilder\n\n";
    for initialized in [
        "val text: Text = Text()",
        "val text = Text()",
        "val text = java.lang.StringBuilder()",
    ] {
        let builder = members_of_call_result_importing(alias, initialized, "text.append(\"x\")");
        assert_offers(&builder, &["append", "length", "reverse"]);
        assert_hides(&builder, &["uppercase", "size", "keys"]);
    }

    let generic_alias = "typealias StringMap<V> = java.util.HashMap<String, V>\n\n";
    for initialized in ["val map: StringMap = StringMap()", "val map = StringMap()"] {
        let keys = members_of_call_result_importing(generic_alias, initialized, "map.keySet()");
        assert_hides(&keys, &["size", "iterator", "contains"]);
    }
}

/// A qualified constructor call that writes no type arguments
/// gives the value the raw class, as its unqualified form does: `val roster =
/// java.util.ArrayList()` offers ArrayList's members on `roster.`, and still
/// no String member on `roster.get(0).`.
#[test]
fn qualified_constructor_call_without_type_arguments_gives_the_raw_class() {
    let raw = "val roster = java.util.ArrayList()";
    let own = members_of_call_result(raw, "roster");
    assert_offers(&own, &["add", "size", "get"]);
    assert!(
        details(&own, "get")
            .iter()
            .all(|detail| !detail.contains("String")),
        "{:?}",
        details(&own, "get")
    );

    let string = members_of_call_result(raw, "roster.get(0)");
    assert_hides(&string, &["length", "uppercase", "substring"]);
}

/// Members after `<access>.`, in a function that holds two `Factory` values,
/// the parameter `given` and the local `factory`, whose `make` has a type
/// parameter of its own and whose class has none.
fn members_of_factory_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!(
                "package app\n\nclass Factory {{\n    fun <T> make(): List<T> = TODO()\n}}\n\nfun use(given: Factory) {{\n    val factory = Factory()\n    {access}.<|>\n}}\n"
            ),
        )],
        "app/Use.kt",
    )
}

/// A member call that writes type arguments binds the member's own
/// type parameters to them in its declared type: `factory.make<String>()`,
/// `make` declared `fun <T> make(): List<T>`, is a `List<String>`, so it
/// offers List's members and its `get(0)` is a `String`, whether `factory` is
/// a parameter or a local initialized by `Factory()`. The member's arguments
/// win over the receiver's for a type parameter the member redeclares:
/// `box.make<List<Int>>()` on a `Box<String>`, `make` declared `fun <T>
/// make(): T` in `Box<T>`, is a `List<Int>`.
#[test]
fn member_call_type_arguments_bind_the_member_type_parameters() {
    for holder in ["given", "factory"] {
        let list = members_of_factory_access(&format!("{holder}.make<String>()"));
        assert_offers(&list, &["size", "first", "map"]);
        assert_hides(&list, &["length", "uppercase"]);

        let string = members_of_factory_access(&format!("{holder}.make<String>().get(0)"));
        assert_offers(&string, &["length", "uppercase", "substring"]);
        assert_hides(&string, &["size", "subList"]);
    }

    let redeclared = members_of_box_access("box.make<List<Int>>()");
    assert_offers(&redeclared, &["size", "first", "map"]);
    assert_hides(&redeclared, &["length", "uppercase", "item"]);
}

/// A member call that writes no type arguments binds none of the
/// member's type parameters, so no type is invented for them: `factory.make()`
/// is a `List` and offers its members, but its `get(0)` offers no String
/// member.
#[test]
fn member_call_without_type_arguments_binds_no_member_type_parameter() {
    for holder in ["given", "factory"] {
        let list = members_of_factory_access(&format!("{holder}.make()"));
        assert_offers(&list, &["size", "first", "map"]);

        let element = members_of_factory_access(&format!("{holder}.make().get(0)"));
        assert_hides(&element, &["length", "uppercase", "substring"]);
    }
}

/// Members after `<access>.`, in a function that holds two `Sub` values, the
/// parameter `given` and the local `sub`, where `Sub` declares nothing and
/// inherits `make` and `one`, which each have a type parameter of their own,
/// from `Factory`.
fn members_of_sub_access(access: &str) -> Vec<CompletionItem> {
    complete(
        &[(
            "app/Use.kt",
            &format!(
                "package app\n\nopen class Factory {{\n    fun <T> make(): List<T> = TODO()\n    fun <T> one(): T = TODO()\n}}\n\nclass Sub : Factory()\n\nfun use(given: Sub) {{\n    val sub = Sub()\n    {access}.<|>\n}}\n"
            ),
        )],
        "app/Use.kt",
    )
}

/// A call with type arguments to a member the receiver's class
/// inherits binds the member's own type parameters to them as it does for a
/// member the class declares: `sub.make<String>()`, `make` declared `fun <T>
/// make(): List<T>` in `Factory` and `Sub : Factory()`, is a `List<String>`,
/// so its `get(0)` is a `String`, and `sub.one<String>()`, `one` declared
/// `fun <T> one(): T`, is a `String`.
#[test]
fn inherited_member_call_type_arguments_bind_the_member_type_parameters() {
    for holder in ["given", "sub"] {
        let list = members_of_sub_access(&format!("{holder}.make<String>()"));
        assert_offers(&list, &["size", "first", "map"]);
        assert_hides(&list, &["length", "uppercase"]);

        for access in ["make<String>().get(0)", "one<String>()"] {
            let string = members_of_sub_access(&format!("{holder}.{access}"));
            assert_offers(&string, &["length", "uppercase", "substring"]);
            assert_hides(&string, &["size", "subList"]);
        }
    }
}

/// A call without type arguments to an inherited member binds none
/// of its type parameters: `sub.make()` is a `List` and offers its members,
/// but neither its `get(0)` nor `sub.one()` offers a String member.
#[test]
fn inherited_member_call_without_type_arguments_binds_no_member_type_parameter() {
    for holder in ["given", "sub"] {
        let list = members_of_sub_access(&format!("{holder}.make()"));
        assert_offers(&list, &["size", "first", "map"]);

        for access in ["make().get(0)", "one()"] {
            let element = members_of_sub_access(&format!("{holder}.{access}"));
            assert_hides(&element, &["length", "uppercase", "substring"]);
        }
    }
}

/// A stdlib function called with type arguments binds its own type
/// parameters to them in its declared type, as a member call does, with no
/// import: `listOf("a").filterIsInstance<String>()`, declared `fun <R>
/// Iterable<*>.filterIsInstance(): List<R>`, is a `List<String>`, so a value
/// it initializes offers List's members and its `get(0)` String's, and
/// `arrayOfNulls<String>(0)`, declared `fun <T> arrayOfNulls(size: Int):
/// Array<T?>`, is an `Array` and offers Array's members. A call with no value
/// argument to bind from, `emptyList<String>()` or `listOf<String>()`, does the
/// same, and the call's argument wins over the receiver's element type:
/// `listOf("a").filterIsInstance<Int>().get(0)` is an `Int`. The typed array
/// picks an overload: `roster.toArray(arrayOfNulls<String>(0))` takes
/// `toArray(T[] a)`, so its elements offer String's members.
#[test]
fn stdlib_call_type_arguments_bind_the_function_type_parameters() {
    let names = "val names = listOf(\"a\").filterIsInstance<String>()";
    let list = members_of_call_result(names, "names");
    assert_offers(&list, &["size", "first", "map"]);
    assert_hides(&list, &["length", "uppercase"]);
    let string = members_of_call_result(names, "names.get(0)");
    assert_offers(&string, &["length", "uppercase", "substring"]);
    assert_hides(&string, &["size", "subList"]);

    let array = members_of_call_result("val arr = arrayOfNulls<String>(0)", "arr");
    assert_offers(&array, &["size", "get", "iterator", "copyOf"]);
    assert_hides(&array, &["add", "stream", "length"]);

    for call in [
        "listOf(\"a\").filterIsInstance<String>().get(0)",
        "emptyList<String>().get(0)",
        "listOf<String>().get(0)",
    ] {
        let string = members_of_call_result("", call);
        assert_offers(&string, &["length", "uppercase", "substring"]);
        assert_hides(&string, &["size", "subList"]);
    }

    let int = members_of_call_result("", "listOf(\"a\").filterIsInstance<Int>().get(0)");
    assert_offers(&int, &["dec", "div"]);
    assert_hides(&int, &["length", "uppercase", "substring"]);

    let elements = members_of_roster_access("roster.toArray(arrayOfNulls<String>(0)).forEach { it");
    assert_offers(&elements, &["length", "uppercase", "substring"]);
}

/// A stdlib function called without type arguments, and with no
/// value argument to bind from, binds none of its type parameters, so no type
/// is invented for them, from the receiver or otherwise: neither
/// `emptyList().get(0)` nor `listOf("a").filterIsInstance().get(0)` offers a
/// String member, though each call is a `List` and offers List's members.
#[test]
fn stdlib_call_without_type_arguments_binds_no_function_type_parameter() {
    for call in ["emptyList()", "listOf(\"a\").filterIsInstance()"] {
        let list = members_of_call_result("", call);
        assert_offers(&list, &["size", "first", "map"]);

        let element = members_of_call_result("", &format!("{call}.get(0)"));
        assert_hides(&element, &["length", "uppercase", "substring"]);
    }
}
