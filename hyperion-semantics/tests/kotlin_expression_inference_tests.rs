// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Expression-form coverage for the Kotlin type inference engine.
//!
//! These exercise `KotlinTypeInferenceEngine::infer_expression` directly for the
//! expression forms that previously fell into the `Type::Unknown` catch-all:
//! `when`, `try`/`catch`, lambdas (including receiver threading and the implicit
//! `it` parameter), string templates and literal wrappers.

use std::sync::Arc;

use hyperion_kotlin::{SyntaxKind as KtKind, SyntaxNode as KtSyntaxNode};
use hyperion_semantics::kotlin_resolver::KotlinResolverBuilder;
use hyperion_semantics::symbol::{ClassType, PrimitiveType, Type};
use hyperion_semantics::{KotlinTypeInferenceEngine, SymbolResolver};
use hyperion_workspace::{FileId, WorkspaceIndex};

struct Harness {
    engine: KotlinTypeInferenceEngine,
    root: KtSyntaxNode,
    resolver: SymbolResolver,
    _temp: tempfile::TempDir,
}

impl Harness {
    fn new(source: &str) -> Self {
        Self::with_extra_files(source, &[])
    }

    /// `extra` holds `(relative path, source)` pairs indexed alongside `source`
    /// so that member lookups can resolve against workspace symbols.
    fn with_extra_files(source: &str, extra: &[(&str, &str)]) -> Self {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace_index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));

        for (relative, contents) in extra {
            let path = temp.path().join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create package dir");
            }
            workspace_index
                .index_file_with_source(&path, contents)
                .expect("extra source should index");
        }

        let path = temp.path().join("Main.kt");
        workspace_index
            .index_file_with_source(&path, source)
            .expect("source should index");

        let file_id = workspace_index
            .get_file_id(&path)
            .unwrap_or_else(|| FileId::new(0));

        let tree = hyperion_kotlin::parse(source);
        let root = KtSyntaxNode::new_root(tree.green_node.clone());

        let symbol_table =
            KotlinResolverBuilder::new(hyperion_semantics::db::FileId::new(0)).build(&root);

        Self {
            engine: KotlinTypeInferenceEngine::new(workspace_index, file_id),
            root,
            resolver: SymbolResolver::with_symbol_table(symbol_table),
            _temp: temp,
        }
    }

    /// Infer the first node of `kind` whose trimmed text starts with `prefix`.
    fn infer(&mut self, kind: KtKind, prefix: &str) -> Type {
        let node = self
            .root
            .descendants()
            .find(|node| {
                node.kind() == kind && node.text().to_string().trim_start().starts_with(prefix)
            })
            .unwrap_or_else(|| {
                panic!(
                    "no {kind:?} starting with {prefix:?} in tree:\n{:#?}",
                    self.root
                )
            });
        self.engine.infer_expression(&node, &mut self.resolver)
    }

    fn infer_first(&mut self, kind: KtKind) -> Type {
        self.infer(kind, "")
    }
}

fn kotlin_class(name: &str) -> Type {
    Type::Class(ClassType {
        name: name.into(),
        package: Some("kotlin".into()),
        type_arguments: vec![],
        is_diamond: false,
    })
}

fn list_of(element: Type) -> Type {
    Type::Class(ClassType {
        name: "List".into(),
        package: Some("kotlin.collections".into()),
        type_arguments: vec![element],
        is_diamond: false,
    })
}

// ========================= literals & string templates =========================

#[test]
fn literal_expr_nodes_infer_their_literal_type() {
    let mut h = Harness::new("fun f() {\n  42\n}\n");
    assert_eq!(
        h.infer_first(KtKind::LITERAL_EXPR),
        Type::Primitive(PrimitiveType::Int)
    );
}

#[test]
fn string_template_infers_as_kotlin_string() {
    let mut h = Harness::new("fun f(name: String) {\n  \"hello $name and ${name.length}\"\n}\n");
    assert_eq!(h.infer_first(KtKind::LITERAL_EXPR), kotlin_class("String"));
}

#[test]
fn raw_string_template_infers_as_kotlin_string() {
    let mut h = Harness::new("fun f(name: String) {\n  \"\"\"raw $name\"\"\"\n}\n");
    assert_eq!(h.infer_first(KtKind::LITERAL_EXPR), kotlin_class("String"));
}

// ================================ when-expression ==============================

#[test]
fn when_with_identical_branches_infers_that_type() {
    let mut h = Harness::new(
        "fun f(x: Int): Any {\n  when (x) {\n    1 -> 1\n    2 -> 2\n    else -> 3\n  }\n}\n",
    );
    assert_eq!(
        h.infer_first(KtKind::WHEN_EXPR),
        Type::Primitive(PrimitiveType::Int)
    );
}

#[test]
fn when_with_mixed_int_and_long_branches_widens_to_long() {
    let mut h = Harness::new(
        "fun f(x: Int): Any {\n  when (x) {\n    1 -> 1\n    2 -> 2L\n    else -> 3\n  }\n}\n",
    );
    assert_eq!(
        h.infer_first(KtKind::WHEN_EXPR),
        Type::Primitive(PrimitiveType::Long)
    );
}

#[test]
fn when_with_mixed_int_and_double_branches_widens_to_double() {
    let mut h =
        Harness::new("fun f(x: Int): Any {\n  when (x) {\n    1 -> 1\n    else -> 2.5\n  }\n}\n");
    assert_eq!(
        h.infer_first(KtKind::WHEN_EXPR),
        Type::Primitive(PrimitiveType::Double)
    );
}

#[test]
fn when_with_unrelated_branch_types_falls_back_to_any() {
    let mut h = Harness::new(
        "fun f(x: Int): Any {\n  when (x) {\n    1 -> 1\n    else -> \"two\"\n  }\n}\n",
    );
    assert_eq!(h.infer_first(KtKind::WHEN_EXPR), kotlin_class("Any"));
}

#[test]
fn when_else_branch_contributes_to_the_lub() {
    // Only the else branch is typed; the LUB must still be Long.
    let mut h = Harness::new("fun f(x: Int): Any {\n  when (x) {\n    else -> 7L\n  }\n}\n");
    assert_eq!(
        h.infer_first(KtKind::WHEN_EXPR),
        Type::Primitive(PrimitiveType::Long)
    );
}

#[test]
fn when_subject_binding_is_visible_inside_branch_expressions() {
    // `when (val y = "text")` binds y: String, used as a branch result.
    let mut h =
        Harness::new("fun f(): Any {\n  when (val y = \"text\") {\n    else -> y\n  }\n}\n");
    assert_eq!(h.infer_first(KtKind::WHEN_EXPR), kotlin_class("String"));
}

#[test]
fn when_subject_binding_is_not_visible_outside_the_when() {
    // `y` is only bound inside the when; outside it must stay unresolved.
    let mut h =
        Harness::new("fun f(): Any {\n  when (val y = \"text\") {\n    else -> 1\n  }\n  y\n}\n");
    // The when itself still types from its branch.
    assert_eq!(
        h.infer_first(KtKind::WHEN_EXPR),
        Type::Primitive(PrimitiveType::Int)
    );

    let trailing = h
        .root
        .descendants()
        .filter(|n| n.kind() == KtKind::EXPR_STMT)
        .find(|n| n.text().to_string().trim() == "y")
        .expect("trailing `y` statement");
    assert_eq!(
        h.engine.infer_expression(&trailing, &mut h.resolver),
        Type::Unknown
    );
}

// ============================== try-expression =================================

#[test]
fn try_expression_is_the_lub_of_try_and_catch_bodies() {
    let mut h =
        Harness::new("fun f(): Any {\n  try {\n    1\n  } catch (e: Exception) {\n    2\n  }\n}\n");
    assert_eq!(
        h.infer_first(KtKind::TRY_EXPR),
        Type::Primitive(PrimitiveType::Int)
    );
}

#[test]
fn try_expression_widens_mixed_numeric_bodies() {
    let mut h = Harness::new(
        "fun f(): Any {\n  try {\n    1\n  } catch (e: Exception) {\n    2L\n  }\n}\n",
    );
    assert_eq!(
        h.infer_first(KtKind::TRY_EXPR),
        Type::Primitive(PrimitiveType::Long)
    );
}

#[test]
fn try_expression_ignores_the_finally_block() {
    let mut h = Harness::new(
        "fun f(): Any {\n  try {\n    \"a\"\n  } catch (e: Exception) {\n    \"b\"\n  } finally {\n    1\n  }\n}\n",
    );
    assert_eq!(h.infer_first(KtKind::TRY_EXPR), kotlin_class("String"));
}

// ================================== lambdas ====================================

#[test]
fn lambda_with_explicit_parameter_infers_a_function_type() {
    let mut h = Harness::new("val g = { a: Int -> a }\n");
    let ty = h.infer_first(KtKind::LAMBDA_EXPR);
    let Type::Class(class) = ty else {
        panic!("expected a class type, got {ty:?}");
    };
    assert_eq!(class.name, "Function1");
    assert_eq!(
        class.type_arguments,
        vec![
            Type::Primitive(PrimitiveType::Int),
            Type::Primitive(PrimitiveType::Int)
        ]
    );
}

#[test]
fn implicit_it_parameter_is_typed_from_the_receiver_element_type() {
    // `it * 2` is only Int if `it` was typed as the List<Int> element type.
    let mut h = Harness::new("val z = listOf(1, 2).map { it * 2 }\n");
    assert_eq!(
        h.infer(KtKind::BINARY_EXPR, "it"),
        Type::Primitive(PrimitiveType::Int)
    );
}

#[test]
fn map_over_a_list_of_int_infers_list_of_int() {
    let mut h = Harness::new("val z = listOf(1, 2).map { it * 2 }\n");
    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "listOf(1, 2).map"),
        list_of(Type::Primitive(PrimitiveType::Int))
    );
}

#[test]
fn map_to_a_different_element_type_is_reflected_in_the_result() {
    let mut h = Harness::new("val z = listOf(1, 2).map { \"n\" }\n");
    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "listOf(1, 2).map"),
        list_of(kotlin_class("String"))
    );
}

#[test]
fn list_of_infers_its_element_type_from_arguments() {
    let mut h = Harness::new("val z = listOf(1, 2)\n");
    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "listOf"),
        list_of(Type::Primitive(PrimitiveType::Int))
    );
}

#[test]
fn let_binds_it_to_the_receiver_type() {
    let mut h = Harness::new("fun f(s: String) {\n  s.let { it }\n}\n");
    // `it` inside `String.let` is the String receiver.
    let lambda = h
        .root
        .descendants()
        .find(|n| n.kind() == KtKind::LAMBDA_EXPR)
        .expect("lambda");
    let ty = h.engine.infer_expression(&lambda, &mut h.resolver);
    let Type::Class(class) = ty else {
        panic!("expected a function type, got {ty:?}");
    };
    assert_eq!(class.name, "Function1");
    assert_eq!(class.type_arguments.first(), Some(&kotlin_class("String")));
}

// ========================== lambda receiver threading ==========================

const STRING_BUILDER: &str = r"
package hyperion.test

class StringBuilder {
    fun append(value: String): StringBuilder = this
    fun length(): Int = 0
}
";

#[test]
fn with_receiver_resolves_unqualified_member_calls() {
    let mut h = Harness::with_extra_files(
        "package hyperion.test\n\nfun f(sb: StringBuilder) {\n  with(sb) {\n    append(\"x\")\n  }\n}\n",
        &[("hyperion/test/StringBuilder.kt", STRING_BUILDER)],
    );

    let ty = h.infer(KtKind::CALL_EXPR, "append");
    assert_eq!(
        ty,
        Type::Class(ClassType {
            name: "StringBuilder".into(),
            package: Some("hyperion.test".into()),
            type_arguments: vec![],
            is_diamond: false,
        }),
        "append should resolve against the `with` receiver"
    );
}

#[test]
fn apply_receiver_resolves_unqualified_member_calls() {
    let mut h = Harness::with_extra_files(
        "package hyperion.test\n\nfun f(sb: StringBuilder) {\n  sb.apply {\n    length()\n  }\n}\n",
        &[("hyperion/test/StringBuilder.kt", STRING_BUILDER)],
    );

    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "length"),
        Type::Primitive(PrimitiveType::Int),
        "length should resolve against the `apply` receiver"
    );
}

#[test]
fn this_inside_a_with_block_is_the_receiver() {
    let mut h = Harness::with_extra_files(
        "package hyperion.test\n\nfun f(sb: StringBuilder) {\n  with(sb) {\n    this.length()\n  }\n}\n",
        &[("hyperion/test/StringBuilder.kt", STRING_BUILDER)],
    );

    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "this.length"),
        Type::Primitive(PrimitiveType::Int)
    );
}

#[test]
fn dsl_builder_with_declared_receiver_lambda_threads_the_receiver() {
    let dsl = r"
package hyperion.test

class Html {
    fun body(): Int = 0
}

fun html(block: Html.() -> Unit) {}
";
    let mut h = Harness::with_extra_files(
        "package hyperion.test\n\nfun f() {\n  html {\n    body()\n  }\n}\n",
        &[("hyperion/test/Html.kt", dsl)],
    );

    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "body"),
        Type::Primitive(PrimitiveType::Int),
        "body() should resolve against the declared Html receiver"
    );
}

#[test]
fn out_of_scope_dsl_function_does_not_supply_a_lambda_receiver() {
    let unrelated = r#"
package unrelated

class WrongReceiver {
    fun body(): Int = 0
}

fun html(block: WrongReceiver.() -> Unit) {}
"#;
    let mut h = Harness::with_extra_files(
        "package use\n\nfun f() {\n  html {\n    body()\n  }\n}\n",
        &[("unrelated/Dsl.kt", unrelated)],
    );

    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "body"),
        Type::Unknown,
        "a global simple-name match outside the call's imports is not a receiver"
    );
}

#[test]
fn conflicting_star_imported_dsl_receivers_are_ambiguous() {
    let mut h = Harness::with_extra_files(
        concat!(
            "package use\nimport alpha.*\nimport beta.*\n",
            "fun f() { html { body() } }\n",
        ),
        &[
            (
                "alpha/Dsl.kt",
                "package alpha\nclass Alpha { fun body(): Int = 0 }\nfun html(block: Alpha.() -> Unit) {}",
            ),
            (
                "beta/Dsl.kt",
                "package beta\nclass Beta { fun body(): String = \"\" }\nfun html(block: Beta.() -> Unit) {}",
            ),
        ],
    );

    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "body"),
        Type::Unknown,
        "two in-scope receiver declarations must not be resolved by file order"
    );
}

// ============================== if-expression LUB ==============================

#[test]
fn if_expression_widens_mixed_numeric_branches() {
    let mut h = Harness::new("fun f(x: Int): Any {\n  return if (x > 0) 1 else 2L\n}\n");
    assert_eq!(
        h.infer_first(KtKind::IF_EXPR),
        Type::Primitive(PrimitiveType::Long)
    );
}

// ===================== local-initializer position ===============
//
// Every test above puts its expression in statement, return or expression-body
// position. That was not a stylistic choice: the Kotlin parser's
// `parse_local_property_decl` consumed a local `val`/`var` initializer with a
// flat token scan, so `val r = when (x) { ... }` produced no `WHEN_EXPR` at all
// (just PROPERTY_DECL tokens plus a spurious `LAMBDA_EXPR` for the braces) and
// there was nothing for the inference engine to look at. The initializer now
// goes through the real expression parser, so the same inference must hold in
// initializer position. Each test asserts the node's parent is the
// `PROPERTY_DECL` — otherwise it would silently degrade into a re-test of
// statement position.

/// The node must actually be the initializer child of a `PROPERTY_DECL`.
fn assert_is_local_initializer(h: &Harness, kind: KtKind) {
    let node = h
        .root
        .descendants()
        .find(|n| n.kind() == kind)
        .unwrap_or_else(|| panic!("no {kind:?} in tree"));
    let parent = node.parent().expect("initializer must have a parent");
    assert_eq!(
        parent.kind(),
        KtKind::PROPERTY_DECL,
        "expected {kind:?} directly under PROPERTY_DECL, got {:?}",
        parent.kind()
    );
    assert!(
        parent.ancestors().any(|a| a.kind() == KtKind::BLOCK),
        "expected a *local* property declaration (inside a function body)"
    );
}

#[test]
fn local_val_when_initializer_widens_to_long() {
    let mut h = Harness::new(
        "fun f(x: Int) {\n  val r = when (x) {\n    1 -> 1\n    2 -> 2L\n    else -> 3\n  }\n}\n",
    );
    assert_is_local_initializer(&h, KtKind::WHEN_EXPR);
    assert_eq!(
        h.infer_first(KtKind::WHEN_EXPR),
        Type::Primitive(PrimitiveType::Long)
    );
}

#[test]
fn local_val_when_initializer_with_unrelated_branches_falls_back_to_any() {
    let mut h = Harness::new(
        "fun f(x: Int) {\n  val r = when (x) {\n    1 -> 1\n    else -> \"two\"\n  }\n}\n",
    );
    assert_is_local_initializer(&h, KtKind::WHEN_EXPR);
    assert_eq!(h.infer_first(KtKind::WHEN_EXPR), kotlin_class("Any"));
}

#[test]
fn local_val_try_initializer_is_the_lub_of_try_and_catch_bodies() {
    let mut h = Harness::new(
        "fun f() {\n  val r = try {\n    1\n  } catch (e: Exception) {\n    2L\n  }\n}\n",
    );
    assert_is_local_initializer(&h, KtKind::TRY_EXPR);
    assert_eq!(
        h.infer_first(KtKind::TRY_EXPR),
        Type::Primitive(PrimitiveType::Long)
    );
}

#[test]
fn local_val_try_initializer_ignores_the_finally_block() {
    let mut h = Harness::new(
        "fun f() {\n  val r = try {\n    \"a\"\n  } catch (e: Exception) {\n    \"b\"\n  } finally {\n    1\n  }\n}\n",
    );
    assert_is_local_initializer(&h, KtKind::TRY_EXPR);
    assert_eq!(h.infer_first(KtKind::TRY_EXPR), kotlin_class("String"));
}

#[test]
fn local_val_if_initializer_widens_mixed_numeric_branches() {
    let mut h = Harness::new("fun f(x: Int) {\n  val r = if (x > 0) 1 else 2L\n}\n");
    assert_is_local_initializer(&h, KtKind::IF_EXPR);
    assert_eq!(
        h.infer_first(KtKind::IF_EXPR),
        Type::Primitive(PrimitiveType::Long)
    );
}

#[test]
fn local_val_when_subject_binding_is_visible_inside_branch_expressions() {
    let mut h =
        Harness::new("fun f() {\n  val r = when (val y = \"text\") {\n    else -> y\n  }\n}\n");
    assert_is_local_initializer(&h, KtKind::WHEN_EXPR);
    assert_eq!(h.infer_first(KtKind::WHEN_EXPR), kotlin_class("String"));
}

#[test]
fn local_val_when_subject_binding_is_not_visible_outside_the_when() {
    let mut h = Harness::new(
        "fun f() {\n  val r = when (val y = \"text\") {\n    else -> 1\n  }\n  y\n}\n",
    );
    assert_is_local_initializer(&h, KtKind::WHEN_EXPR);
    assert_eq!(
        h.infer_first(KtKind::WHEN_EXPR),
        Type::Primitive(PrimitiveType::Int)
    );

    let trailing = h
        .root
        .descendants()
        .filter(|n| n.kind() == KtKind::EXPR_STMT)
        .find(|n| n.text().to_string().trim() == "y")
        .expect("trailing `y` statement");
    assert_eq!(
        h.engine.infer_expression(&trailing, &mut h.resolver),
        Type::Unknown
    );
}

#[test]
fn local_val_lambda_initializer_infers_a_function_type() {
    let mut h = Harness::new("fun f() {\n  val g = { a: Int -> a }\n}\n");
    assert_is_local_initializer(&h, KtKind::LAMBDA_EXPR);
    let ty = h.infer_first(KtKind::LAMBDA_EXPR);
    let Type::Class(class) = ty else {
        panic!("expected a class type, got {ty:?}");
    };
    assert_eq!(class.name, "Function1");
    assert_eq!(
        class.type_arguments,
        vec![
            Type::Primitive(PrimitiveType::Int),
            Type::Primitive(PrimitiveType::Int)
        ]
    );
}

#[test]
fn local_val_map_over_a_list_of_int_infers_list_of_int() {
    let mut h = Harness::new("fun f() {\n  val z = listOf(1, 2).map { it * 2 }\n}\n");
    assert_is_local_initializer(&h, KtKind::CALL_EXPR);
    assert_eq!(
        h.infer(KtKind::CALL_EXPR, "listOf(1, 2).map"),
        list_of(Type::Primitive(PrimitiveType::Int))
    );
}

#[test]
fn local_val_do_while_initializer_is_at_least_reachable() {
    // `do { ... } while (c)` in initializer position used to be flat tokens.
    // The shape must now exist; inference for it is a separate concern.
    let h = Harness::new("fun f(c: Boolean) {\n  val r = do { 1 } while (c)\n}\n");
    assert_is_local_initializer(&h, KtKind::DO_WHILE_EXPR);
}
