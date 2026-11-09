// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Navigation must retain each lambda's own functional target while selecting
//! overloads, including declarations reached through static imports.

use crate::lsp_test_framework::{goto_response_to_locations, LspTest, SharedLspContext, TestFile};

fn fixture(files: &[(&str, &str)]) -> SharedLspContext {
    let mut test = LspTest::new();
    for &(path, source) in files {
        test.add_file(path, source);
    }
    test.into_shared_context_with_options(false).unwrap()
}

fn assert_target(
    context: &SharedLspContext,
    source_file: &str,
    use_marker: &str,
    target_file: &str,
    target_source: &str,
    declaration_marker: &str,
) {
    let response = context.goto_definition(source_file, use_marker).unwrap();
    let locations = goto_response_to_locations(response);
    let target = TestFile::new(target_source);
    let expected = target.find_marker(declaration_marker).unwrap();
    assert_eq!(
        locations.len(),
        1,
        "{source_file} {use_marker}: {locations:?}"
    );
    assert!(locations[0].uri.path().ends_with(target_file));
    assert_eq!(locations[0].range.start.line, expected.line);
    assert_eq!(locations[0].range.start.character, expected.character);
}

fn assert_unresolved(context: &SharedLspContext, source_file: &str, use_marker: &str) {
    let response = context.goto_definition(source_file, use_marker).unwrap();
    assert!(
        goto_response_to_locations(response).is_empty(),
        "{source_file}: ambiguous or incompatible lambda must stay unresolved"
    );
}

#[test]
fn lambda_rejects_primitive_sibling_and_wrong_arity() {
    let functions = "package p; public interface Fn { int apply(int value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «functional»choose(Fn fn) { return fn.apply(1); }
    public static int «primitive»choose(int value) { return value; }
}
";
    let source = r"package q;
import p.Helpers;
class Use {
    int good = Helpers.«good»choose(x -> x);
    int bad = Helpers.«bad»choose((x, y) -> x);
}
";
    let context = fixture(&[
        ("p/Fn.java", functions),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«good»",
        "p/Helpers.java",
        methods,
        "«functional»",
    );
    assert_unresolved(&context, "q/Use.java", "«bad»");
}

#[test]
fn incomparable_functional_overloads_remain_ambiguous() {
    let first = "package p; public interface IntFn { int apply(int value); }\n";
    let second = "package p; public interface BoolFn { boolean apply(boolean value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int choose(IntFn fn) { return 1; }
    public static int choose(BoolFn fn) { return 2; }
}
";
    let source = r"package q;
import static p.Helpers.choose;
class Use { int value = «call»choose(x -> x); }
";
    let context = fixture(&[
        ("p/IntFn.java", first),
        ("p/BoolFn.java", second),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_unresolved(&context, "q/Use.java", "«call»");
}

#[test]
fn two_imported_inherited_names_share_one_declaration() {
    let function = "package p; public interface Fn { int apply(int value); }\n";
    let base = r"package p;
public class Base {
    public static int «choose»choose(Fn fn) { return fn.apply(1); }
}
";
    let first = "package p; public class First extends Base {}\n";
    let second = "package p; public class Second extends Base {}\n";
    let source = r"package q;
import static p.First.choose;
import static p.Second.choose;
class Use { int value = «call»choose(x -> x); }
";
    let context = fixture(&[
        ("p/Fn.java", function),
        ("p/Base.java", base),
        ("p/First.java", first),
        ("p/Second.java", second),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«call»",
        "p/Base.java",
        base,
        "«choose»",
    );
}

#[test]
fn lambda_body_uses_its_candidate_parameter_type() {
    let box_type = r"package p;
public class Box {
    public int value() { return 1; }
}
";
    let function = "package p; public interface Fn { int apply(Box box); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «choose»choose(Fn fn) { return fn.apply(new Box()); }
    public static int choose(int value) { return value; }
}
";
    let source = r"package q;
import p.Helpers;
class Use { int result = Helpers.«call»choose(box -> box.value()); }
";
    let context = fixture(&[
        ("p/Box.java", box_type),
        ("p/Fn.java", function),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«call»",
        "p/Helpers.java",
        methods,
        "«choose»",
    );
}

#[test]
fn value_only_lambda_does_not_match_void_function() {
    let valued = "package p; public interface IntFn { int apply(int value); }\n";
    let voided = "package p; public interface VoidFn { void accept(int value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «valued»choose(IntFn fn) { return fn.apply(1); }
    public static int «voided»choose(VoidFn fn) { fn.accept(1); return 0; }
}
";
    let source = r"package q;
import p.Helpers;
class Use { int value = Helpers.«call»choose(x -> x); }
";
    let context = fixture(&[
        ("p/IntFn.java", valued),
        ("p/VoidFn.java", voided),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«call»",
        "p/Helpers.java",
        methods,
        "«valued»",
    );
}

#[test]
fn statement_expression_lambda_matches_void_function() {
    let function = "package p; public interface VoidFn { void accept(int value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «choose»choose(VoidFn fn) { fn.accept(1); return 1; }
    public static int choose(int value) { return value; }
    public static int log(int value) { return value; }
}
";
    let source = r"package q;
import p.Helpers;
class Use { int value = Helpers.«call»choose(x -> Helpers.log(x)); }
";
    let context = fixture(&[
        ("p/VoidFn.java", function),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«call»",
        "p/Helpers.java",
        methods,
        "«choose»",
    );
}

#[test]
fn incompatible_enclosing_method_does_not_hide_static_import() {
    let function = "package p; public interface Fn { int apply(int value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «choose»choose(Fn fn) { return fn.apply(1); }
}
";
    let source = r"package q;
import static p.Helpers.choose;
class Use {
    int choose(int value) { return value; }
    int value = «call»choose(x -> x);
}
";
    let context = fixture(&[
        ("p/Fn.java", function),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«call»",
        "p/Helpers.java",
        methods,
        "«choose»",
    );
}

#[test]
fn unavailable_functional_signature_blocks_sibling_selection() {
    let function = "package p; public interface Fn { int apply(int value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int choose(Fn fn) { return fn.apply(1); }
    public static int choose(UnindexedFn fn) { return 2; }
}
";
    let source = r"package q;
import p.Helpers;
class Use { int value = Helpers.«call»choose(x -> x); }
";
    let context = fixture(&[
        ("p/Fn.java", function),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_unresolved(&context, "q/Use.java", "«call»");
}

#[test]
fn throw_only_lambda_can_target_a_value_function() {
    let function = "package p; public interface Fn { int apply(); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «choose»choose(Fn fn) { return 1; }
    public static int choose(int value) { return value; }
}
";
    let source = r"package q;
import p.Helpers;
class Use { int value = Helpers.«call»choose(() -> { throw null; }); }
";
    let context = fixture(&[
        ("p/Fn.java", function),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«call»",
        "p/Helpers.java",
        methods,
        "«choose»",
    );
}

#[test]
fn unary_comment_does_not_make_lambda_void_compatible() {
    let valued = "package p; public interface IntFn { int apply(int value); }\n";
    let voided = "package p; public interface VoidFn { void accept(int value); }\n";
    let methods = r"package p;
public class Helpers {
    public static int «valued»choose(IntFn fn) { return fn.apply(1); }
    public static int choose(VoidFn fn) { fn.accept(1); return 0; }
}

";
    let source = r"package q;
import p.Helpers;
class Use { int value = Helpers.«call»choose(x -> -(x /* ++ */)); }
";
    let context = fixture(&[
        ("p/IntFn.java", valued),
        ("p/VoidFn.java", voided),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«call»",
        "p/Helpers.java",
        methods,
        "«valued»",
    );
}

#[test]
fn parameterized_receiver_preserves_lambda_input_and_rejects_wrong_explicit_type() {
    let function = "package p; public interface Fn<T> { boolean test(T value); }";
    let methods = r"package p;
public class Values<T> {
    public Values<T> «filter»filter(Fn<? super T> predicate) { return this; }
}
";
    let source = r"package q;
import p.Values;
class Use {
    void run(Values<String> values) {
        values.«implicit»filter(value -> value.isEmpty());
        values.«explicit»filter((String value) -> value.isEmpty());
        values.«wider»filter((Object value) -> true);
        values.«wrong»filter((Integer value) -> true);
    }
}
";
    let context = fixture(&[
        ("p/Fn.java", function),
        ("p/Values.java", methods),
        ("q/Use.java", source),
    ]);
    for marker in ["«implicit»", "«explicit»", "«wider»"] {
        assert_target(
            &context,
            "q/Use.java",
            marker,
            "p/Values.java",
            methods,
            "«filter»",
        );
    }
    assert_unresolved(&context, "q/Use.java", "«wrong»");
}

#[test]
fn raw_function_target_uses_erased_lambda_parameter() {
    let function = "package p; public interface Fn<T> { boolean test(T value); }";
    let methods = r"package p;
public class Helpers {
    public static boolean «test»test(Fn predicate) { return predicate.test(null); }
}
";
    let source = r"package q;
import p.Helpers;
class Use {
    boolean good = Helpers.«good»test((Object value) -> true);
    boolean bad = Helpers.«bad»test((String value) -> true);
}
";
    let context = fixture(&[
        ("p/Fn.java", function),
        ("p/Helpers.java", methods),
        ("q/Use.java", source),
    ]);
    assert_target(
        &context,
        "q/Use.java",
        "«good»",
        "p/Helpers.java",
        methods,
        "«test»",
    );
    assert_unresolved(&context, "q/Use.java", "«bad»");
}

#[test]
fn real_jdk_stream_filter_keeps_parameterized_receiver_contract() {
    use hyperion_parser::{lexer::Lexer, parser::Parser, SyntaxKind, SyntaxNode};
    use hyperion_semantics::{resolver::ResolverBuilder, TypeInferenceEngine};

    let source = r"import java.util.List;
class Use {
    long run(List<String> values) {
        return values.stream().«filter»filter(value -> value.length() > 0).count();
    }
}";
    let mut test = LspTest::new();
    test.add_file("Use.java", source);
    let context = test.into_shared_context_with_options(true).unwrap();
    // This harness indexes binary JDK metadata without a JDK source provider.
    // Exercise the complete real receiver chain through semantic inference;
    // the source-backed test below verifies the navigation location.
    let file_id = context.index.find_symbol("Use").unwrap().file_id;
    let clean = TestFile::new(source).clean_content;
    let (green, errors) = Parser::parse(Lexer::new(&clean).collect());
    assert!(errors.is_empty(), "{errors:?}");
    let root = SyntaxNode::new_root(green);
    let engine = TypeInferenceEngine::new(context.index.clone(), file_id);
    let resolver =
        ResolverBuilder::with_workspace(file_id, context.index.clone()).build_from_syntax(&root);
    let filter = root
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::METHOD_CALL_EXPR
                && node
                    .text()
                    .to_string()
                    .ends_with("filter(value -> value.length() > 0)")
        })
        .unwrap();
    assert_eq!(
        engine
            .infer_expression(&filter, &resolver, None)
            .to_string(),
        "java.util.stream.Stream<java.lang.String>"
    );
}

#[test]
fn jdk_stream_chain_navigates_to_source_backed_filter() {
    let source = r"import java.util.List;
class Use {
    long run(List<String> values) {
        return values.stream().«call»filter(value -> value.length() > 0).count();
    }
}";
    let stream = r"package java.util.stream;
import java.util.function.Predicate;
public interface Stream<T> {
    Stream<T> «filter»filter(Predicate<? super T> predicate);
    long count();
}";
    let mut test = LspTest::new();
    test.add_file("Use.java", source);
    test.add_file("java/util/stream/Stream.java", stream);
    let context = test.into_shared_context_with_options(true).unwrap();
    assert_target(
        &context,
        "Use.java",
        "«call»",
        "java/util/stream/Stream.java",
        stream,
        "«filter»",
    );
}
