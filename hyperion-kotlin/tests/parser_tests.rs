// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive Tests for Kotlin Parser
//!
//! Tests parsing of all supported Kotlin constructs including functions,
//! properties, classes, objects, and error recovery.

use hyperion_kotlin::lexer::Lexer;
use hyperion_kotlin::parser::{ParseError, Parser};
use hyperion_kotlin::syntax::{KotlinLanguage, SyntaxKind};
use rowan::Language;

fn parse_source(source: &str) -> (rowan::GreenNode, Vec<ParseError>) {
    let tokens = Lexer::new(source).collect();
    Parser::parse(tokens)
}

fn assert_no_errors(source: &str) {
    let (_, errors) = parse_source(source);
    if !errors.is_empty() {
        eprintln!("Parse errors for: {source}");
        for err in &errors {
            eprintln!("  {err}");
        }
    }
    assert!(errors.is_empty(), "Expected no parse errors");
}

fn assert_root_kind(source: &str) {
    let (green, _) = parse_source(source);
    assert_eq!(
        green.kind(),
        KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
    );
}

#[test]
fn test_empty_file() {
    assert_no_errors("");
    assert_root_kind("");
}

#[test]
fn test_whitespace_only() {
    assert_no_errors("   \n\t  ");
    assert_root_kind("   \n\t  ");
}

mod function_tests {
    use super::*;
    use rowan::SyntaxNode;

    #[test]
    fn test_simple_function() {
        assert_no_errors("fun main() { }");
    }

    #[test]
    fn test_function_with_return_type() {
        assert_no_errors("fun foo(): Int { return 0 }");
    }

    #[test]
    fn test_function_with_parameters() {
        assert_no_errors("fun add(a: Int, b: Int): Int { return a + b }");
    }

    #[test]
    fn test_function_with_function_type_parameter() {
        let (_, errors) = parse_source("fun test(module: () -> String) { }");
        for err in &errors {
            eprintln!("  {err}");
        }
        assert!(
            errors.is_empty(),
            "Expected no errors for function type parameter, got: {errors:?}"
        );
    }

    #[test]
    fn test_function_single_expression_body() {
        assert_no_errors("fun double(x: Int) = x * 2");
    }

    #[test]
    fn test_function_no_return_type() {
        assert_no_errors("fun greet() { println(\"Hello\") }");
    }

    #[test]
    fn test_function_with_modifiers() {
        assert_no_errors("public fun foo() { }");
        assert_no_errors("private fun bar() { }");
        assert_no_errors("inline fun baz() { }");
        assert_no_errors("suspend fun async() { }");
    }

    #[test]
    fn test_function_with_type_parameters() {
        assert_no_errors("fun <T> identity(x: T): T { return x }");
    }

    #[test]
    fn test_function_default_parameters() {
        assert_no_errors("fun greet(name: String = \"World\") { }");
    }

    #[test]
    fn test_empty_function() {
        assert_no_errors("fun empty() { }");
    }

    #[test]
    fn test_function_with_nested_blocks() {
        assert_no_errors("fun foo() { { } }");
    }

    #[test]
    fn test_multiple_functions() {
        let source = r"
fun foo() { }
fun bar() { }
fun baz() { }
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_nullable_extension_receiver_tree_shape() {
        let source = "fun Int?.orZero(): Int = this ?: 0";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        assert_eq!(
            root.text().to_string(),
            source,
            "combined `?.` token must round-trip"
        );
        let function = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::FUNCTION_DECL)
            .expect("extension should be FUNCTION_DECL");
        assert!(
            function
                .descendants_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .any(|token| token.kind() == SyntaxKind::SAFE_CALL && token.text() == "?."),
            "nullable receiver separator should remain one lossless SAFE_CALL token"
        );
    }
}

mod property_tests {
    use super::*;

    #[test]
    fn test_val_property() {
        assert_no_errors("val x = 42");
    }

    #[test]
    fn test_var_property() {
        assert_no_errors("var y = 10");
    }

    #[test]
    fn test_property_with_type() {
        assert_no_errors("val x: Int = 42");
    }

    #[test]
    fn test_property_no_initializer() {
        assert_no_errors("val x: Int");
    }

    #[test]
    fn test_property_with_modifiers() {
        assert_no_errors("public val x = 1");
        assert_no_errors("private var y = 2");
    }

    #[test]
    fn test_multiple_properties() {
        let source = r"
val a = 1
val b = 2
var c = 3
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_property_nullable_type() {
        assert_no_errors("val x: String? = null");
    }

    #[test]
    fn test_property_complex_initializer() {
        assert_no_errors("val x = foo() + bar()");
    }

    // === Property Accessor Tests ===

    // Helper functions for AST verification
    use rowan::SyntaxNode;

    fn find_node(
        root: &SyntaxNode<KotlinLanguage>,
        kind: SyntaxKind,
    ) -> Option<SyntaxNode<KotlinLanguage>> {
        if root.kind() == kind {
            return Some(root.clone());
        }
        for child in root.children() {
            if let Some(found) = find_node(&child, kind) {
                return Some(found);
            }
        }
        None
    }

    fn find_child(
        node: &SyntaxNode<KotlinLanguage>,
        kind: SyntaxKind,
    ) -> Option<SyntaxNode<KotlinLanguage>> {
        node.children().find(|c| c.kind() == kind)
    }

    fn has_text_in_descendants(node: &SyntaxNode<KotlinLanguage>, text: &str) -> bool {
        for element in node.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.text() == text {
                    return true;
                }
            }
        }
        false
    }

    fn count_nodes(root: &SyntaxNode<KotlinLanguage>, kind: SyntaxKind) -> usize {
        let mut count = 0;
        if root.kind() == kind {
            count += 1;
        }
        for child in root.children() {
            count += count_nodes(&child, kind);
        }
        count
    }

    #[test]
    fn test_property_with_expression_getter() {
        let source = r"
class Test {
    val x: Int
        get() = 5
}
        ";
        let (green, errors) = parse_source(source);
        assert_eq!(errors.len(), 0, "Should parse without errors");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);

        // Verify PROPERTY_DECL exists
        let property_decl =
            find_node(&root, SyntaxKind::PROPERTY_DECL).expect("Should find PROPERTY_DECL");

        // Verify PROPERTY_ACCESSOR exists inside PROPERTY_DECL
        let accessor = find_child(&property_decl, SyntaxKind::PROPERTY_ACCESSOR)
            .expect("Should find PROPERTY_ACCESSOR inside PROPERTY_DECL");

        // Verify "get" identifier exists in accessor
        assert!(
            has_text_in_descendants(&accessor, "get"),
            "Accessor should contain 'get'"
        );
    }

    #[test]
    fn test_property_with_block_setter() {
        let source = r"
class Test {
    var x: Int = 0
        set(value) {
            field = value
        }
}
        ";
        let (green, errors) = parse_source(source);
        assert_eq!(errors.len(), 0, "Should parse without errors");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let property_decl =
            find_node(&root, SyntaxKind::PROPERTY_DECL).expect("Should find PROPERTY_DECL");

        let accessor = find_child(&property_decl, SyntaxKind::PROPERTY_ACCESSOR)
            .expect("Should find accessor");

        // Should have "set"
        assert!(
            has_text_in_descendants(&accessor, "set"),
            "Should contain 'set'"
        );

        // Should have parameter list with "value"
        let param_list =
            find_child(&accessor, SyntaxKind::PARAMETER_LIST).expect("Should have parameter list");
        assert!(
            has_text_in_descendants(&param_list, "value"),
            "Should contain 'value' parameter"
        );
    }

    #[test]
    fn test_property_with_both_accessors() {
        let source = r"
class Test {
    var x: Int = 0
        get() = field
        set(value) { field = value }
}
        ";
        let (green, errors) = parse_source(source);
        assert_eq!(errors.len(), 0, "Should parse without errors");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);

        // Should have TWO PROPERTY_ACCESSOR nodes
        let accessor_count = count_nodes(&root, SyntaxKind::PROPERTY_ACCESSOR);
        assert_eq!(accessor_count, 2, "Should have two accessors");
    }

    #[test]
    fn test_property_with_visibility_on_setter() {
        let source = r"
class Test {
    var x: Int = 0
        private set
}
        ";
        let (green, errors) = parse_source(source);
        assert_eq!(errors.len(), 0, "Should parse without errors");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let accessor =
            find_node(&root, SyntaxKind::PROPERTY_ACCESSOR).expect("Should find accessor");

        // Should have "private" and "set"
        assert!(
            has_text_in_descendants(&accessor, "private"),
            "Should contain 'private' modifier"
        );
        assert!(
            has_text_in_descendants(&accessor, "set"),
            "Should contain 'set'"
        );
    }

    #[test]
    fn test_property_getter_with_block_body() {
        let source = r#"
class Test {
    val name: String
        get() {
            return "test"
        }
}
        "#;
        let (green, errors) = parse_source(source);
        assert_eq!(errors.len(), 0, "Should parse without errors");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let accessor =
            find_node(&root, SyntaxKind::PROPERTY_ACCESSOR).expect("Should find accessor");

        // Should have block body
        let _block =
            find_child(&accessor, SyntaxKind::BLOCK).expect("Getter should have block body");

        assert!(
            has_text_in_descendants(&accessor, "get"),
            "Should contain 'get'"
        );
    }

    #[test]
    fn test_property_accessor_with_type_annotation() {
        let source = r"
class Test {
    var x: Int = 0
        set(value: Int) {
            field = value
        }
}
        ";
        let (green, errors) = parse_source(source);
        assert_eq!(errors.len(), 0, "Should parse with type annotation");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let accessor =
            find_node(&root, SyntaxKind::PROPERTY_ACCESSOR).expect("Should find accessor");

        let param_list =
            find_child(&accessor, SyntaxKind::PARAMETER_LIST).expect("Should have parameter list");

        // Should contain both "value" and "Int"
        assert!(
            has_text_in_descendants(&param_list, "value"),
            "Should have parameter name"
        );
        assert!(
            has_text_in_descendants(&param_list, "Int"),
            "Should have type annotation"
        );
    }

    #[test]
    fn test_property_getter_no_body() {
        let source = r"
interface Test {
    val x: Int
        get
}
        ";
        let (green, _errors) = parse_source(source);
        // Note: No body is valid for interface properties
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);

        // Should still find PROPERTY_ACCESSOR even without body
        let property_decl = find_node(&root, SyntaxKind::PROPERTY_DECL);
        assert!(property_decl.is_some(), "Should find property declaration");
    }

    #[test]
    fn test_top_level_property_with_getter() {
        let source = r#"
val topLevel: String
    get() = "value"
        "#;
        let (green, errors) = parse_source(source);
        assert_eq!(
            errors.len(),
            0,
            "Should parse top-level property with getter"
        );

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let accessor = find_node(&root, SyntaxKind::PROPERTY_ACCESSOR)
            .expect("Should find accessor in top-level property");

        assert!(
            has_text_in_descendants(&accessor, "get"),
            "Should contain 'get'"
        );
    }
}

mod class_tests {
    use super::*;
    use rowan::SyntaxNode;

    #[test]
    fn test_simple_class() {
        assert_no_errors("class Foo { }");
    }

    #[test]
    fn test_empty_class() {
        assert_no_errors("class Empty");
    }

    #[test]
    fn test_class_with_primary_constructor() {
        assert_no_errors("class Person(val name: String, val age: Int)");
    }

    #[test]
    fn test_class_with_constructor_keyword() {
        assert_no_errors("class Foo constructor() { }");
    }

    #[test]
    fn test_class_with_modifiers() {
        assert_no_errors("public class Foo { }");
        assert_no_errors("data class Bar { }");
        assert_no_errors("sealed class Baz { }");
        assert_no_errors("abstract class Abstract { }");
    }

    #[test]
    fn test_class_with_type_parameters() {
        assert_no_errors("class Box<T> { }");
        assert_no_errors("class Pair<T, U> { }");
    }

    #[test]
    fn test_class_with_supertype() {
        assert_no_errors("class Child : Parent { }");
    }

    #[test]
    fn test_class_with_multiple_supertypes() {
        assert_no_errors("class MyClass : Interface1, Interface2, Interface3 { }");
    }

    #[test]
    fn test_interface_with_multiple_supertypes() {
        assert_no_errors("interface Child : Left, Right { }");
    }

    #[test]
    fn test_class_with_parent_and_interfaces() {
        assert_no_errors("class MyClass : Parent(), Interface1, Interface2 { }");
    }

    #[test]
    fn test_class_with_generic_supertypes() {
        assert_no_errors("class MyList : ArrayList<String>, Comparable<MyList> { }");
    }

    #[test]
    fn test_class_with_constructor_call_supertype() {
        assert_no_errors("class Child : Parent(\"arg\", 42) { }");
    }

    #[test]
    fn test_class_interface_delegation_preserves_later_supertypes_and_body() {
        let source = concat!(
            "class Delegating(private val delegate: I) : ",
            "I by decorate(delegate, listOf(1, 2)), J { fun body() {} }"
        );
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let class = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CLASS_DECL)
            .expect("class declaration");
        assert_eq!(class.text().to_string(), source);
        assert!(class
            .descendants()
            .any(|node| node.kind() == SyntaxKind::FUNCTION_DECL));
        assert!(class
            .children()
            .any(|node| node.kind() == SyntaxKind::CALL_EXPR));
        assert_eq!(
            class
                .children()
                .filter(|node| node.kind() == SyntaxKind::TYPE_REF)
                .count(),
            2
        );
    }

    #[test]
    fn test_bare_delegate_does_not_consume_the_class_body_as_a_lambda() {
        let source = "class Delegating(private val delegate: I) : I by delegate { fun body() {} }";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let class = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CLASS_DECL)
            .expect("class declaration");
        let body = class
            .children()
            .find(|node| node.kind() == SyntaxKind::CLASS_BODY)
            .expect("outer class body");

        assert_eq!(body.text().to_string(), "{ fun body() {} }");
        assert!(body
            .children()
            .any(|node| node.kind() == SyntaxKind::FUNCTION_DECL));
        assert!(!class
            .descendants()
            .any(|node| node.kind() == SyntaxKind::LAMBDA_EXPR));
    }

    #[test]
    fn test_object_delegate_and_outer_class_body_remain_distinct() {
        let source = concat!(
            "class Delegating : I by object : I { override fun run() {} } ",
            "{ fun body() {} }"
        );
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let class = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CLASS_DECL)
            .expect("class declaration");
        let object = class
            .children()
            .find(|node| node.kind() == SyntaxKind::OBJECT_DECL)
            .expect("structured object delegate");
        let body = class
            .children()
            .find(|node| node.kind() == SyntaxKind::CLASS_BODY)
            .expect("outer class body");

        assert!(object
            .children()
            .any(|node| node.kind() == SyntaxKind::CLASS_BODY));
        assert_eq!(body.text().to_string(), "{ fun body() {} }");
        assert!(body
            .children()
            .any(|node| node.kind() == SyntaxKind::FUNCTION_DECL));
    }

    #[test]
    fn test_primary_lambda_delegate_remains_structured() {
        let source = "class Delegating : I by { make() } { fun body() {} }";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let class = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CLASS_DECL)
            .expect("class declaration");
        let body = class
            .children()
            .find(|node| node.kind() == SyntaxKind::CLASS_BODY)
            .expect("outer class body");

        assert!(class
            .children()
            .any(|node| node.kind() == SyntaxKind::LAMBDA_EXPR));
        assert_eq!(body.text().to_string(), "{ fun body() {} }");
    }

    #[test]
    fn test_nested_lambdas_in_delegate_calls_remain_structured() {
        let source = concat!(
            "class Delegating : I by ",
            "decorate({ make() }, wrapper { nested() }) { fun body() {} }"
        );
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let class = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CLASS_DECL)
            .expect("class declaration");
        let body = class
            .children()
            .find(|node| node.kind() == SyntaxKind::CLASS_BODY)
            .expect("outer class body");

        assert!(class
            .children()
            .any(|node| node.kind() == SyntaxKind::CALL_EXPR));
        assert_eq!(
            class
                .descendants()
                .filter(|node| node.kind() == SyntaxKind::LAMBDA_EXPR)
                .count(),
            2
        );
        assert_eq!(body.text().to_string(), "{ fun body() {} }");
        assert!(body
            .children()
            .any(|node| node.kind() == SyntaxKind::FUNCTION_DECL));
    }

    #[test]
    fn test_enum_and_annotation_class_modifiers_are_lossless() {
        let source = "enum class Choice { ONE }\nannotation class Marker";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        assert_eq!(root.text().to_string(), source);
        assert_eq!(
            root.descendants()
                .filter(|node| node.kind() == SyntaxKind::CLASS_DECL)
                .count(),
            2
        );
    }

    #[test]
    fn test_class_with_methods() {
        let source = r"
class Foo {
    fun bar() { }
    fun baz() { }
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_class_with_properties() {
        let source = r"
class Person {
    val name: String
    var age: Int
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_class_with_init_block() {
        let source = r"
class Foo {
    init { }
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_class_with_secondary_constructor() {
        let source = r"
class Foo {
    constructor() { }
    constructor(x: Int) { }
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_secondary_constructor_delegation_is_lossless() {
        let source = concat!(
            "open class Base\n",
            "class Derived : Base {\n",
            "  constructor(x: Int) : super(make(x, listOf(1, 2))) {}\n",
            "  constructor() : this(0) {}\n",
            "}"
        );
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let constructors = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::CONSTRUCTOR_DECL)
            .map(|node| node.text().to_string())
            .collect::<Vec<_>>();
        assert_eq!(constructors.len(), 2);
        assert!(constructors[0].contains(": super(make(x, listOf(1, 2)))"));
        assert!(constructors[1].contains(": this(0)"));
    }

    #[test]
    fn test_secondary_constructor_rejects_a_non_constructor_delegate() {
        let (_, errors) = parse_source("class Bad { constructor() : other() {} }");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_nested_classes() {
        let source = r"
class Outer {
    class Inner { }
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_data_class() {
        assert_no_errors("data class Person(val name: String, val age: Int)");
    }

    #[test]
    fn test_sealed_class() {
        assert_no_errors("sealed class Result { }");
    }

    #[test]
    fn test_multiple_classes() {
        let source = r"
class Foo { }
class Bar { }
class Baz { }
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_local_class_tree_shape() {
        let source = "fun make() { class Local(val value: Int) { fun get() = value }; Local(1) }";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let block = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK)
            .expect("function should have a BLOCK");
        assert!(
            block
                .children()
                .any(|node| node.kind() == SyntaxKind::CLASS_DECL),
            "local class must be a direct BLOCK declaration, not recovery tokens"
        );
    }

    #[test]
    fn test_qualified_this_expression_body_tree_shape() {
        let source = "class Outer { inner class Inner { fun outer() = this@Outer.name } }";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let member_accesses: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::MEMBER_ACCESS_EXPR)
            .map(|node| node.text().to_string())
            .collect();
        assert!(
            member_accesses
                .iter()
                .any(|text| text.trim_end() == "this@Outer.name"),
            "qualified `this` should remain the receiver of its member access; got {member_accesses:?}"
        );
    }
}

mod object_tests {
    use super::*;

    #[test]
    fn test_simple_object() {
        assert_no_errors("object Singleton { }");
    }

    #[test]
    fn test_object_with_members() {
        let source = r"
object Utils {
    fun helper() { }
    val constant = 42
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_object_with_supertype() {
        assert_no_errors("object Impl : Interface { }");
    }

    #[test]
    fn test_companion_object() {
        let source = r"
class Foo {
    companion object { }
}
        ";
        assert_no_errors(source);
    }
}

mod interface_tests {
    use super::*;
    use rowan::SyntaxNode;

    #[test]
    fn test_simple_interface() {
        assert_no_errors("interface Foo { }");
    }

    #[test]
    fn test_interface_with_methods() {
        let source = r"
interface Foo {
    fun bar()
    fun baz(): Int
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_interface_with_type_parameters() {
        assert_no_errors("interface Comparable<T> { }");
    }

    #[test]
    fn test_interface_with_supertype() {
        assert_no_errors("interface Child : Parent { }");
    }

    #[test]
    fn test_interface_with_properties() {
        let source = r"
interface Named {
    val name: String
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_fun_interface_tree_shape() {
        let source = "fun interface Runner { fun run() }";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let interface = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::INTERFACE_DECL)
            .expect("SAM declaration should be INTERFACE_DECL");
        assert_eq!(interface.text().to_string(), source);
        assert!(
            interface
                .children()
                .any(|node| node.kind() == SyntaxKind::CLASS_BODY),
            "SAM body should use the regular interface/class member body"
        );
    }
}

mod package_import_tests {
    use super::*;

    #[test]
    fn test_package_declaration() {
        assert_no_errors("package com.example");
    }

    #[test]
    fn test_package_nested() {
        assert_no_errors("package com.example.app");
    }

    #[test]
    fn test_import_declaration() {
        assert_no_errors("import kotlin.collections.List");
    }

    #[test]
    fn test_import_with_alias() {
        assert_no_errors("import kotlin.collections.List as KList");
    }

    #[test]
    fn test_package_and_imports() {
        let source = r"
package com.example

import kotlin.collections.List
import kotlin.collections.Map

class Foo { }
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_multiple_imports() {
        let source = r"
import foo.Bar
import foo.Baz
import foo.Qux
        ";
        assert_no_errors(source);
    }
}

mod annotation_tests {
    use super::*;
    use rowan::SyntaxNode;

    #[test]
    fn test_simple_annotation() {
        assert_no_errors("@Deprecated class Foo { }");
    }

    #[test]
    fn test_annotation_with_arguments() {
        assert_no_errors("@Suppress(\"unused\") fun foo() { }");
    }

    #[test]
    fn test_multiple_annotations() {
        let source = r#"
@Deprecated
@Suppress("unused")
class Foo { }
        "#;
        assert_no_errors(source);
    }

    #[test]
    fn test_qualified_annotation() {
        assert_no_errors("@kotlin.Deprecated class Foo { }");
    }

    #[test]
    fn test_annotation_constructor_value_tree_shape() {
        let source = "@Deprecated2(\"use new()\", ReplaceWith(\"new()\")) fun old() {}";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let argument_lists: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::ANNOTATION_ARG_LIST)
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(
            argument_lists,
            ["(\"use new()\", ReplaceWith(\"new()\"))", "(\"new()\")"]
        );
    }
}

mod type_tests {
    use super::*;
    use rowan::SyntaxNode;

    #[test]
    fn test_simple_type() {
        assert_no_errors("val x: Int");
    }

    #[test]
    fn test_nullable_type() {
        assert_no_errors("val x: String?");
    }

    #[test]
    fn test_generic_type() {
        assert_no_errors("val list: List<Int>");
    }

    #[test]
    fn test_nested_generics() {
        assert_no_errors("val map: Map<String, List<Int>>");
    }

    #[test]
    fn test_qualified_type() {
        assert_no_errors("val x: kotlin.String");
    }

    #[test]
    fn test_multiple_type_arguments() {
        assert_no_errors("class Pair<T, U>");
    }

    #[test]
    fn test_star_projection() {
        // Single star projection: List<*>
        assert_no_errors("val x: KClass<*>");
    }

    #[test]
    fn test_star_projection_in_list() {
        // Star projection in List
        assert_no_errors("val list: List<*>");
    }

    #[test]
    fn test_star_projection_multi_argument() {
        // Star projection mixed with regular types: Map<String, *>
        assert_no_errors("val map: Map<String, *>");
    }

    #[test]
    fn test_star_projection_multiple_stars() {
        // Multiple star projections: Pair<*, *>
        assert_no_errors("val pair: Pair<*, *>");
    }

    #[test]
    fn test_star_projection_with_nullability() {
        // Star projection with nullable: List<*>?
        assert_no_errors("val x: List<*>?");
    }

    #[test]
    fn test_star_projection_nested() {
        // Nested generics with star: List<Map<String, *>>
        assert_no_errors("val x: List<Map<String, *>>");
    }

    #[test]
    fn test_use_site_variance_type_argument_tree_shape() {
        let source = "fun copy(from: Array<out Any>, to: Array<in String>) {}";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let type_argument_texts: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::TYPE_ARGUMENT_LIST)
            .map(|node| node.text().to_string())
            .collect();
        assert_eq!(type_argument_texts, ["<out Any>", "<in String>"]);
    }

    #[test]
    fn test_suspend_function_type_tree_shape() {
        let source = "fun schedule(action: suspend (Int) -> Boolean) {}";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let function_type = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::FUNCTION_TYPE)
            .expect("suspend function type should have FUNCTION_TYPE node");
        assert_eq!(function_type.text().to_string(), "suspend (Int) -> Boolean");
    }

    #[test]
    fn test_receiver_function_type_tree_shape_is_lossless() {
        let source = "fun render(block: Body.() -> Unit) {}";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        assert_eq!(
            root.text().to_string(),
            source,
            "receiver dot must appear exactly once"
        );

        let function_type = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::FUNCTION_TYPE)
            .expect("receiver function type should have FUNCTION_TYPE node");
        assert_eq!(function_type.text().to_string(), ".() -> Unit");

        let parameter = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PARAMETER)
            .expect("function parameter should remain a PARAMETER node");
        assert!(
            parameter
                .children()
                .any(|node| node.kind() == SyntaxKind::TYPE_REF && node.text() == "Body"),
            "receiver must remain its own TYPE_REF before FUNCTION_TYPE"
        );
    }

    #[test]
    fn test_class_literal_expression_tree_shape() {
        let source = "fun typeName() = String::class.simpleName";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        assert_eq!(
            root.text().to_string(),
            source,
            "class literal must round-trip"
        );
        assert!(
            root.descendants().any(|node| {
                node.kind() == SyntaxKind::MEMBER_ACCESS_EXPR && node.text() == "String::class"
            }),
            "`String::class` should form a member-like class-literal expression"
        );
    }
}

mod typealias_and_reified_tests {
    use super::*;
    use rowan::SyntaxNode;

    fn find_node(
        root: &SyntaxNode<KotlinLanguage>,
        kind: SyntaxKind,
    ) -> Option<SyntaxNode<KotlinLanguage>> {
        if root.kind() == kind {
            return Some(root.clone());
        }
        for child in root.children() {
            if let Some(found) = find_node(&child, kind) {
                return Some(found);
            }
        }
        None
    }

    fn has_text_in_descendants(node: &SyntaxNode<KotlinLanguage>, text: &str) -> bool {
        for element in node.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.text() == text {
                    return true;
                }
            }
        }
        false
    }

    // === `reified` type-parameter modifier ===

    #[test]
    fn test_reified_type_parameter_no_errors() {
        assert_no_errors("inline fun <reified T> f() {}");
    }

    #[test]
    fn test_reified_type_parameter_tree_shape() {
        let (green, errors) = parse_source("inline fun <reified T> f() {}");
        assert_eq!(errors.len(), 0, "should parse without errors");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);

        let type_param_list = find_node(&root, SyntaxKind::TYPE_PARAMETER_LIST)
            .expect("should find TYPE_PARAMETER_LIST");
        let type_param = find_node(&type_param_list, SyntaxKind::TYPE_PARAMETER)
            .expect("should find TYPE_PARAMETER inside the list");

        // The `reified` keyword must have been consumed as part of the type
        // parameter (not left dangling or dropped), and the type parameter
        // name `T` must still be present alongside it.
        assert!(
            has_text_in_descendants(&type_param, "reified"),
            "TYPE_PARAMETER should contain the `reified` modifier"
        );
        assert!(
            has_text_in_descendants(&type_param, "T"),
            "TYPE_PARAMETER should still contain its name `T`"
        );
    }

    #[test]
    fn test_reified_combined_with_variance() {
        // Syntactically, Kotlin's grammar allows more than one type-parameter
        // modifier (variance + reification), even though `out`/`in` and
        // `reified` are rarely combined in practice.
        assert_no_errors("inline fun <reified T, out U> f() {}");
        assert_no_errors("class Box<in T>");
    }

    #[test]
    fn test_reified_as_soft_keyword_identifier() {
        // `reified` is a soft keyword: it is only a type-parameter modifier in
        // type-parameter position. Everywhere else it must remain a valid
        // identifier, e.g. `val reified = 1`.
        assert_no_errors("val reified = 1");
        assert_no_errors("fun f(reified: Int) = reified");
        assert_no_errors("fun reified() = 1");
    }

    // === `typealias` declarations ===

    #[test]
    fn test_simple_typealias_no_errors() {
        assert_no_errors("typealias Name = String");
    }

    #[test]
    fn test_generic_typealias_no_errors() {
        assert_no_errors("typealias Handler<T> = (T) -> Unit");
    }

    #[test]
    fn test_simple_typealias_tree_shape() {
        let (green, errors) = parse_source("typealias Name = String");
        assert_eq!(errors.len(), 0, "should parse without errors");
        assert_root_kind("typealias Name = String");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);

        let typealias_node = find_node(&root, SyntaxKind::TYPEALIAS_DECL)
            .expect("should find a real TYPEALIAS_DECL node, not a silently-swallowed decl");

        assert!(
            has_text_in_descendants(&typealias_node, "Name"),
            "TYPEALIAS_DECL should contain the alias name `Name`"
        );

        // The right-hand side must be a real TYPE_REF, not just leftover
        // tokens dropped into the tree by error recovery.
        let type_ref = find_node(&typealias_node, SyntaxKind::TYPE_REF)
            .expect("TYPEALIAS_DECL should contain a TYPE_REF for the aliased type");
        assert!(
            has_text_in_descendants(&type_ref, "String"),
            "TYPE_REF should contain the aliased type `String`"
        );
    }

    #[test]
    fn test_generic_typealias_tree_shape() {
        let (green, errors) = parse_source("typealias Handler<T> = (T) -> Unit");
        assert_eq!(errors.len(), 0, "should parse without errors");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);

        let typealias_node = find_node(&root, SyntaxKind::TYPEALIAS_DECL)
            .expect("should find a real TYPEALIAS_DECL node");

        assert!(
            has_text_in_descendants(&typealias_node, "Handler"),
            "TYPEALIAS_DECL should contain the alias name `Handler`"
        );

        let type_param_list = find_node(&typealias_node, SyntaxKind::TYPE_PARAMETER_LIST)
            .expect("TYPEALIAS_DECL should have its own TYPE_PARAMETER_LIST for `<T>`");
        assert!(
            has_text_in_descendants(&type_param_list, "T"),
            "TYPE_PARAMETER_LIST should contain `T`"
        );

        let function_type = find_node(&typealias_node, SyntaxKind::FUNCTION_TYPE)
            .expect("TYPEALIAS_DECL should contain a FUNCTION_TYPE for `(T) -> Unit`");
        assert!(
            has_text_in_descendants(&function_type, "Unit"),
            "FUNCTION_TYPE should contain the return type `Unit`"
        );
    }

    #[test]
    fn test_typealias_in_class_body_is_an_error() {
        // `typealias` is file-scoped only in Kotlin; it is never valid inside
        // a class body. This must be a clear, recorded parse error rather
        // than a silent swallow or a parser hang.
        let (_, errors) = parse_source("class A { typealias X = Int }");
        assert!(
            !errors.is_empty(),
            "class-body typealias should produce at least one parse error"
        );
    }
}

mod expression_tests {
    use super::*;

    #[test]
    fn test_simple_expression() {
        assert_no_errors("val x = 1 + 2");
    }

    #[test]
    fn test_parenthesized_expression() {
        assert_no_errors("val x = (1 + 2) * 3");
    }

    #[test]
    fn test_function_call() {
        assert_no_errors("val x = foo()");
    }

    #[test]
    fn test_lambda_expression() {
        assert_no_errors("val f = { x -> x + 1 }");
    }

    #[test]
    fn test_empty_lambda() {
        assert_no_errors("val f = { }");
    }

    #[test]
    fn test_nested_lambdas() {
        assert_no_errors("val f = { { } }");
    }

    #[test]
    fn test_lambda_explicit_no_params() {
        assert_no_errors("val f = { -> println(\"hello\") }");
    }

    #[test]
    fn test_lambda_single_param() {
        assert_no_errors("val f = { x -> x * 2 }");
    }

    #[test]
    fn test_lambda_multiple_params() {
        assert_no_errors("val f = { x, y -> x + y }");
    }

    #[test]
    fn test_lambda_typed_param() {
        assert_no_errors("val f = { x: Int -> x * 2 }");
    }

    #[test]
    fn test_lambda_multiple_typed_params() {
        assert_no_errors("val f = { x: Int, y: String -> println(y) }");
    }

    #[test]
    fn test_lambda_with_generic_type_param() {
        assert_no_errors("val f = { list: List<String> -> list.size }");
    }

    #[test]
    fn test_lambda_in_function_call() {
        assert_no_errors("list.map { x -> x * 2 }");
    }

    #[test]
    fn test_lambda_with_for_loop() {
        assert_no_errors("val f = { items: List<Int> -> for (i in items) { println(i) } }");
    }
}

mod for_while_tests {
    use super::*;

    #[test]
    fn test_simple_for_loop() {
        assert_no_errors("fun main() { for (x in items) { println(x) } }");
    }

    #[test]
    fn test_for_loop_with_range() {
        assert_no_errors("fun main() { for (i in 1..10) { println(i) } }");
    }

    #[test]
    fn test_for_loop_with_until_range() {
        assert_no_errors("fun main() { for (i in 0 until 10) { println(i) } }");
    }

    #[test]
    fn test_for_loop_with_step() {
        assert_no_errors("fun main() { for (i in 0..10 step 2) { println(i) } }");
    }

    #[test]
    fn test_for_loop_with_index() {
        assert_no_errors("fun main() { for ((index, value) in list.withIndex()) { } }");
    }

    #[test]
    fn test_for_loop_with_method_call() {
        assert_no_errors("fun main() { for (item in list.filter { it > 0 }) { } }");
    }

    #[test]
    fn test_nested_for_loops() {
        assert_no_errors("fun main() { for (i in 1..10) { for (j in 1..10) { } } }");
    }

    #[test]
    fn test_simple_while_loop() {
        assert_no_errors("fun main() { while (x > 0) { x = x - 1 } }");
    }

    #[test]
    fn test_while_with_complex_condition() {
        assert_no_errors("fun main() { while (x > 0 && y < 10) { } }");
    }

    #[test]
    fn test_nested_while_loops() {
        assert_no_errors("fun main() { while (a) { while (b) { } } }");
    }
}

mod error_recovery_tests {
    use super::*;

    #[test]
    fn test_missing_closing_brace() {
        let (_, errors) = parse_source("class Foo {");
        // Should have parse errors but not crash
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_missing_closing_paren() {
        let (_, errors) = parse_source("fun foo(");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_unexpected_token() {
        let (_, errors) = parse_source("class } Foo");
        // Parser should recover and report errors
        assert!(
            !errors.is_empty(),
            "Invalid syntax 'class }} Foo' should produce parser errors"
        );
    }

    #[test]
    fn test_incomplete_property() {
        let (_, errors) = parse_source("val");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_incomplete_function() {
        let (_, errors) = parse_source("fun");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_malformed_type() {
        let (_, errors) = parse_source("val x: <>");
        // Malformed type should produce parser errors
        assert!(
            !errors.is_empty(),
            "Malformed type '<>' should produce parser errors"
        );
    }

    #[test]
    fn test_recovery_continues_parsing() {
        let source = r"
class Bad {
fun good1() { }
fun good2() { }
        ";
        let (green, _) = parse_source(source);

        // Should still parse the valid functions
        assert_eq!(
            green.kind(),
            KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }
}

mod complex_tests {
    use super::*;

    #[test]
    fn test_complete_file() {
        let source = r#"
package com.example

import kotlin.collections.List

data class Person(val name: String, val age: Int)

class PersonRepository {
    private val people: MutableList<Person>

    fun add(person: Person) { }

    fun findByName(name: String): Person? { return null }
}

fun main() {
    val repo = PersonRepository()
    val person = Person("Alice", 30)
    repo.add(person)
}
        "#;
        assert_no_errors(source);
    }

    #[test]
    fn test_nested_structures() {
        let source = r"
class Outer {
    class Inner {
        class Innermost {
            fun foo() { }
        }
    }
}
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_modifiers_combination() {
        let source = r"
public abstract class Base { }
private data class Data(val x: Int)
sealed class Result { }
inline fun helper() { }
suspend fun async() { }
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_generics_everywhere() {
        let source = r"
class Box<T>(val value: T)
fun <T> identity(x: T): T = x
interface Comparable<T> { }
        ";
        assert_no_errors(source);
    }

    #[test]
    fn test_mixed_declarations() {
        let source = r"
val globalVar = 42
fun globalFunction() { }
class GlobalClass { }
object GlobalObject { }
interface GlobalInterface { }
        ";
        assert_no_errors(source);
    }
}

mod losslessness_tests {
    use super::*;

    #[test]
    fn test_preserves_whitespace() {
        let source = "fun   foo  (  )  {  }";
        let (green, _) = parse_source(source);

        // Green tree should preserve all whitespace
        // (Can't easily verify without converting to red tree)
        assert_eq!(
            green.kind(),
            KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }

    #[test]
    fn test_preserves_comments() {
        let source = r"
// Line comment
fun foo() { }
/* Block comment */
        ";
        let (green, _) = parse_source(source);

        // Should include comment tokens in the tree
        assert_eq!(
            green.kind(),
            KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }

    #[test]
    fn test_preserves_newlines() {
        let source = "fun foo() {\n\n\n}";
        let (_, errors) = parse_source(source);

        // Should handle multiple newlines
        assert!(errors.is_empty());
    }

    #[test]
    fn test_preserves_semicolons() {
        let source = "val x = 1;";
        assert_no_errors(source);
    }
}

mod performance_tests {
    use super::*;

    #[test]
    fn test_large_file() {
        // Generate a large file with many declarations
        let mut source = String::new();
        source.push_str("package test\n\n");

        for i in 0..100 {
            source.push_str(&format!("class Class{i} {{}}\n"));
        }

        let (_, errors) = parse_source(&source);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_deep_nesting() {
        let mut source = String::from("class A { ");
        for _ in 0..20 {
            source.push_str("class B { ");
        }
        for _ in 0..20 {
            source.push_str("} ");
        }
        source.push('}');

        let (_, errors) = parse_source(&source);
        // Deep nesting is valid Kotlin and should parse without errors
        assert!(
            errors.is_empty(),
            "Deep nesting should parse without errors, got: {errors:?}"
        );
    }
}

mod modifier_tests {
    use super::*;
    use rowan::SyntaxNode;

    fn find_node(
        root: &SyntaxNode<KotlinLanguage>,
        kind: SyntaxKind,
    ) -> Option<SyntaxNode<KotlinLanguage>> {
        if root.kind() == kind {
            return Some(root.clone());
        }
        root.children().find_map(|child| find_node(&child, kind))
    }

    #[test]
    fn test_all_visibility_modifiers() {
        assert_no_errors("public class A { }");
        assert_no_errors("private class B { }");
        assert_no_errors("protected class C { }");
        assert_no_errors("internal class D { }");
    }

    #[test]
    fn test_all_class_modifiers() {
        assert_no_errors("open class A { }");
        assert_no_errors("abstract class B { }");
        assert_no_errors("data class C(val x: Int)");
        assert_no_errors("sealed class D { }");
    }

    #[test]
    fn test_all_function_modifiers() {
        assert_no_errors("inline fun a() { }");
        assert_no_errors("suspend fun b() { }");
        assert_no_errors("operator fun c() { }");
        assert_no_errors("infix fun d() { }");
        assert_no_errors("override fun e() { }");
    }

    #[test]
    fn test_multiple_modifiers() {
        assert_no_errors("public abstract class Foo { }");
        assert_no_errors("private inline fun bar() { }");
    }

    #[test]
    fn test_primary_constructor_property_parameter_modifiers_tree_shape() {
        let source = "class Account(private val id: Int, override val name: String)";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let parameters: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::PARAMETER)
            .collect();
        assert_eq!(
            parameters.len(),
            2,
            "both constructor parameters stay PARAMETER nodes"
        );

        for (parameter, expected) in parameters
            .iter()
            .zip(["private val id", "override val name"])
        {
            assert!(
                parameter.text().to_string().contains(expected),
                "PARAMETER should retain its modifier and property marker: {expected}"
            );
        }

        assert!(
            find_node(&root, SyntaxKind::CLASS_DECL).is_some(),
            "constructor parameters must remain attached to CLASS_DECL"
        );
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn test_single_keyword() {
        let (_, errors) = parse_source("fun");
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_only_comments() {
        assert_no_errors("// just a comment");
        assert_no_errors("/* block comment */");
    }

    #[test]
    fn test_unicode_in_identifiers() {
        // Note: may not be supported by current lexer
        let source = "class 世界 { }";
        let (_, _) = parse_source(source);
    }

    #[test]
    fn test_very_long_identifier() {
        let long_name = "a".repeat(1000);
        let source = format!("class {long_name} {{}}");
        assert_no_errors(&source);
    }

    #[test]
    fn test_empty_parameter_list() {
        assert_no_errors("fun foo() { }");
    }

    #[test]
    fn test_empty_class_body() {
        assert_no_errors("class Foo { }");
    }

    #[test]
    fn test_trailing_comma_in_parameters() {
        // Kotlin allows trailing commas
        assert_no_errors("fun foo(a: Int, b: Int,) { }");
    }
}

// Unit test for smart cast detection
#[test]
fn test_smart_cast_detection_in_scope_graph() {
    let source = r"
fun test(x: Any) {
    if (x is String) {
        x.length
    }
}
    ";

    use hyperion_kotlin::{build_scope_graph, KotlinType};

    let (green, _) = parse_source(source);
    let tree = rowan::SyntaxNode::<KotlinLanguage>::new_root(green);
    let scope_graph = build_scope_graph(&tree);

    // Find the scope for the if-then block
    // The "x.length" is at approximately position 55
    let pos = 55; // Inside the if block

    if let Some(smart_cast) = scope_graph.resolve_smart_cast("x", pos) {
        assert_eq!(smart_cast.variable_name, "x");
        assert_eq!(smart_cast.narrowed_type, KotlinType::simple("String"));
        println!(
            "Smart cast found: {} -> {:?}",
            smart_cast.variable_name, smart_cast.narrowed_type
        );
    } else {
        panic!("Expected smart cast for 'x' at position {pos}");
    }
}

mod expression_parser_tests {
    use super::*;
    use rowan::SyntaxNode;

    // Helper to find a specific syntax kind in the tree
    fn find_kind(green: &rowan::GreenNode, kind: SyntaxKind) -> bool {
        if green.kind() == KotlinLanguage::kind_to_raw(kind) {
            return true;
        }
        for child in green.children() {
            if let rowan::NodeOrToken::Node(n) = child {
                if find_kind(&n.to_owned(), kind) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn test_binary_arithmetic() {
        assert_no_errors("val x = 1 + 2");
        assert_no_errors("val x = a - b");
        assert_no_errors("val x = a * b");
        assert_no_errors("val x = a / b");
        assert_no_errors("val x = a % b");
    }

    #[test]
    fn test_binary_comparison() {
        assert_no_errors("val x = a < b");
        assert_no_errors("val x = a > b");
        assert_no_errors("val x = a <= b");
        assert_no_errors("val x = a >= b");
        assert_no_errors("val x = a == b");
        assert_no_errors("val x = a != b");
    }

    #[test]
    fn test_binary_logical() {
        assert_no_errors("val x = a && b");
        assert_no_errors("val x = a || b");
        assert_no_errors("val x = a && b || c");
    }

    #[test]
    fn test_unary_operators() {
        assert_no_errors("val x = -a");
        assert_no_errors("val x = !flag");
        assert_no_errors("val x = ++count");
        assert_no_errors("val x = --count");
    }

    #[test]
    fn test_null_safety_operators() {
        assert_no_errors("val x = a ?: b");
        assert_no_errors("val x = a?.b");
        assert_no_errors("val x = a!!");
    }

    #[test]
    fn test_method_chain() {
        assert_no_errors("val x = list.filter { it > 0 }.map { it * 2 }");
    }

    #[test]
    fn test_safe_call_chain() {
        assert_no_errors("val x = user?.name?.length");
    }

    #[test]
    fn test_function_call() {
        assert_no_errors("val x = foo()");
        assert_no_errors("val x = foo(a, b)");
        assert_no_errors("val x = foo(1, \"hello\", 3.14)");
    }

    #[test]
    fn test_function_call_with_trailing_lambda() {
        assert_no_errors("val x = list.map { it * 2 }");
        assert_no_errors("val x = run { doSomething() }");
    }

    #[test]
    fn test_array_access() {
        assert_no_errors("val x = arr[0]");
        assert_no_errors("val x = map[key]");
        assert_no_errors("val x = matrix[i][j]");
    }

    #[test]
    fn test_operator_precedence() {
        // Higher precedence operators bind tighter
        assert_no_errors("val x = a + b * c"); // * before +
        assert_no_errors("val x = a * b + c"); // * before +
        assert_no_errors("val x = a || b && c"); // && before ||
        assert_no_errors("val x = a && b || c"); // && before ||
    }

    #[test]
    fn test_parenthesized_expression() {
        assert_no_errors("val x = (a + b)");
        assert_no_errors("val x = (a + b) * c");
        assert_no_errors("val x = ((a + b) * (c - d))");
    }

    #[test]
    fn test_if_expression() {
        assert_no_errors("val x = if (a) 1 else 2");
        assert_no_errors("val x = if (a > b) { max } else { min }");
    }

    #[test]
    fn test_when_expression() {
        assert_no_errors("val x = when { a > b -> 1 else -> 2 }");
        assert_no_errors("val x = when (value) { 1 -> \"one\" else -> \"other\" }");
    }

    #[test]
    fn test_consecutive_generic_star_when_entries_tree_shape() {
        let source = "fun f(x: Any) = when (x) {\n is List<*> -> \"list\"\n is Map<*, *> -> \"map\"\n else -> \"other\"\n}";
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");

        let root = SyntaxNode::<KotlinLanguage>::new_root(green);
        let when_expr = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::WHEN_EXPR)
            .expect("WHEN_EXPR");
        assert_eq!(
            when_expr
                .children()
                .filter(|node| node.kind() == SyntaxKind::WHEN_ENTRY)
                .count(),
            3,
            "each generic-star branch must remain its own WHEN_ENTRY"
        );
    }

    fn binary_texts(source: &str) -> Vec<String> {
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "expected clean parse: {errors:?}");
        SyntaxNode::<KotlinLanguage>::new_root(green)
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::BINARY_EXPR)
            .map(|node| node.text().to_string().trim().to_string())
            .collect()
    }

    #[test]
    fn test_named_infix_simple_and_chained_tree_shapes() {
        assert_eq!(binary_texts("fun f() = 1 shl 2"), ["1 shl 2"]);

        let chained = binary_texts("fun f() = 1 to 2 to 3");
        assert_eq!(chained.len(), 2);
        assert!(chained.contains(&"1 to 2".to_string()));
        assert!(chained.contains(&"1 to 2 to 3".to_string()));
    }

    #[test]
    fn test_named_infix_precedence_tree_shapes() {
        let texts = binary_texts(
            "fun f(a: Int, b: Int, c: Int, x: Int, n: Int) { val stepped = a until b step c; val contained = x in 1..n }",
        );
        for expected in ["a until b", "a until b step c", "1..n", "x in 1..n"] {
            assert!(
                texts.contains(&expected.to_string()),
                "missing {expected:?} in {texts:?}"
            );
        }
    }

    #[test]
    fn test_named_infix_does_not_cross_newline_statement_boundary() {
        for source in [
            "fun f() {\n    val value = 1\n    shl(2)\n}",
            "fun f() {\n    val list = listOf(1)\n    list.filter { it > 0 }\n}",
        ] {
            let (green, errors) = parse_source(source);
            assert!(errors.is_empty(), "expected clean parse: {errors:?}");
            let root = SyntaxNode::<KotlinLanguage>::new_root(green);
            let block = root
                .descendants()
                .find(|node| node.kind() == SyntaxKind::BLOCK)
                .expect("function BLOCK");
            assert_eq!(
                block.children().map(|node| node.kind()).collect::<Vec<_>>(),
                [SyntaxKind::PROPERTY_DECL, SyntaxKind::EXPR_STMT],
                "the following-line identifier must remain a separate statement for {source:?}"
            );
        }
    }

    #[test]
    fn test_named_infix_in_call_arguments_and_when_subjects() {
        let source = "fun f() { consume(1 shl 2); when (1 shl 2) { 4 -> consume(1 to 2); else -> consume(0) } }";
        let texts = binary_texts(source);
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.as_str() == "1 shl 2")
                .count(),
            2
        );
        assert!(texts.contains(&"1 to 2".to_string()));
    }

    #[test]
    fn test_lambda_expression() {
        let (green, errors) = parse_source("val f = { x: Int -> x * 2 }");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::LAMBDA_EXPR));
    }

    #[test]
    fn test_range_operators() {
        assert_no_errors("val x = 1..10");
        assert_no_errors("val x = 0..<list.size");
    }

    #[test]
    fn test_is_and_in_operators() {
        assert_no_errors("val x = a is String");
        assert_no_errors("val x = a in list");
        assert_no_errors("val x = a !in list");
        assert_no_errors("val x = a !is String");
    }

    #[test]
    fn test_complex_expression() {
        // Complex real-world expression
        assert_no_errors(
            r#"val result = users
                .filter { it.age >= 18 }
                .map { it.name }
                .sorted()
                .firstOrNull() ?: "Unknown""#,
        );
    }

    #[test]
    fn test_member_access() {
        let (green, errors) = parse_source("val x = user.name");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::MEMBER_ACCESS_EXPR));
    }

    #[test]
    fn test_safe_access() {
        let (green, errors) = parse_source("val x = user?.name");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::SAFE_ACCESS_EXPR));
    }

    #[test]
    fn test_call_expr() {
        let (green, errors) = parse_source("val x = foo(1, 2)");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::CALL_EXPR));
    }

    #[test]
    fn test_literal_expr() {
        let (green, errors) = parse_source("val x = 42");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::LITERAL_EXPR));
    }

    #[test]
    fn test_string_literal_expr() {
        let (green, errors) = parse_source("val x = \"hello\"");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::LITERAL_EXPR));
    }

    #[test]
    fn test_return_expr() {
        let (green, errors) = parse_source("fun foo() { return 42 }");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::RETURN_EXPR));
    }

    #[test]
    fn test_throw_expr() {
        let (green, errors) = parse_source("fun foo() { throw Exception() }");
        assert!(errors.is_empty());
        assert!(find_kind(&green, SyntaxKind::THROW_EXPR));
    }
}

/// Local `val`/`var` initializers go through the real expression parser.
///
/// Before this, `parse_local_property_decl` consumed the initializer with a
/// flat token scan delimited by newlines and `val`/`var`/`fun` keywords, and
/// handed any `{` to a brace-counting `parse_lambda_expression`. So
/// `val r = when (x) { ... }` produced bare `PROPERTY_DECL` tokens plus one
/// spurious `LAMBDA_EXPR` for the `when` body — no `WHEN_EXPR`, no
/// `WHEN_ENTRY`. Structured nodes appeared in every *other* position
/// (statement, return, call argument, top-level `val`, expression-body `fun`),
/// which is what these tests pin: the initializer subtree must be
/// shape-identical to the same expression in statement position.
mod local_property_initializer_tests {
    use super::*;
    use hyperion_kotlin::SyntaxNode;

    fn tree(source: &str) -> SyntaxNode {
        let (green, _) = parse_source(source);
        SyntaxNode::new_root(green)
    }

    /// Kinds of the *node* children of a function body's `BLOCK`, i.e. the
    /// statement list. Tokens are ignored.
    fn block_statement_kinds(source: &str) -> Vec<SyntaxKind> {
        let root = tree(source);
        let block = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::BLOCK)
            .unwrap_or_else(|| panic!("no BLOCK in tree for {source:?}"));
        block.children().map(|c| c.kind()).collect()
    }

    /// Node children of the first `PROPERTY_DECL` in the tree.
    fn property_child_kinds(source: &str) -> Vec<SyntaxKind> {
        let root = tree(source);
        let prop = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PROPERTY_DECL)
            .unwrap_or_else(|| panic!("no PROPERTY_DECL in tree for {source:?}"));
        prop.children().map(|c| c.kind()).collect()
    }

    /// Recursive kind skeleton, used to prove that initializer position and
    /// statement position build byte-for-byte identical subtrees.
    fn skeleton(node: &SyntaxNode) -> String {
        let mut out = format!("{:?}", node.kind());
        let children: Vec<String> = node.children().map(|c| skeleton(&c)).collect();
        if !children.is_empty() {
            out.push('(');
            out.push_str(&children.join(","));
            out.push(')');
        }
        out
    }

    fn first_skeleton(source: &str, kind: SyntaxKind) -> String {
        let root = tree(source);
        let node = root
            .descendants()
            .find(|n| n.kind() == kind)
            .unwrap_or_else(|| panic!("no {kind:?} in tree for {source:?}"));
        skeleton(&node)
    }

    // ---------------------------------------------------------------- shapes

    #[test]
    fn local_val_when_initializer_builds_a_when_expr() {
        let src = "fun f(x: Int) {\n    val r = when (x) {\n        1 -> 1\n        else -> 2\n    }\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(
            property_child_kinds(src),
            vec![SyntaxKind::WHEN_EXPR],
            "the when must be the sole node child of PROPERTY_DECL"
        );
        // ...and no flat lambda anywhere.
        let root = tree(src);
        assert!(
            !root
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
            "the when body braces must not be misparsed as a LAMBDA_EXPR"
        );
        assert_eq!(
            root.descendants()
                .filter(|n| n.kind() == SyntaxKind::WHEN_ENTRY)
                .count(),
            2
        );
    }

    #[test]
    fn local_val_try_initializer_builds_a_try_expr() {
        let src = "fun f() {\n    val r = try { a() } catch (e: E) { b() }\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(property_child_kinds(src), vec![SyntaxKind::TRY_EXPR]);
        let root = tree(src);
        assert_eq!(
            root.descendants()
                .filter(|n| n.kind() == SyntaxKind::CATCH_CLAUSE)
                .count(),
            1
        );
        assert!(
            !root
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
            "try/catch bodies must be BLOCKs, not LAMBDA_EXPRs"
        );
    }

    #[test]
    fn local_val_if_initializer_builds_an_if_expr() {
        let src = "fun f(c: Boolean) {\n    val r = if (c) 1 else 2\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(property_child_kinds(src), vec![SyntaxKind::IF_EXPR]);
    }

    #[test]
    fn local_val_do_while_initializer_builds_a_do_while_expr() {
        let src = "fun f(c: Boolean) {\n    val r = do { a() } while (c)\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(property_child_kinds(src), vec![SyntaxKind::DO_WHILE_EXPR]);
    }

    #[test]
    fn local_val_lambda_initializer_builds_a_structured_lambda() {
        // The old flat `parse_lambda_expression` produced a LAMBDA_EXPR with no
        // LAMBDA_PARAMETERS and no statements inside — just raw tokens.
        let src = "fun f() {\n    val g = { a: Int -> a + 1 }\n    h()\n}";
        assert_no_errors(src);
        assert_eq!(property_child_kinds(src), vec![SyntaxKind::LAMBDA_EXPR]);
        let root = tree(src);
        let lambda = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
            .expect("lambda");
        assert!(lambda
            .children()
            .any(|c| c.kind() == SyntaxKind::LAMBDA_PARAMETERS));
        assert!(lambda.children().any(|c| c.kind() == SyntaxKind::EXPR_STMT));
    }

    #[test]
    fn var_initializer_gets_the_same_treatment_as_val() {
        let src = "fun f(x: Int) {\n    var r = when (x) {\n        else -> 2\n    }\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(property_child_kinds(src), vec![SyntaxKind::WHEN_EXPR]);
    }

    #[test]
    fn by_delegate_expression_is_also_structured() {
        let src = "fun f() {\n    val x by lazy { 1 }\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(property_child_kinds(src), vec![SyntaxKind::CALL_EXPR]);
        let root = tree(src);
        let call = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CALL_EXPR)
            .expect("call");
        assert!(call.children().any(|c| c.kind() == SyntaxKind::LAMBDA_EXPR));
    }

    #[test]
    fn initializer_position_and_statement_position_build_identical_subtrees() {
        let body = "when (x) {\n        1 -> 1\n        2, 3 -> 2L\n        is A -> 3\n        else -> 4\n    }";
        let as_initializer = format!("fun f(x: Any) {{\n    val r = {body}\n    g()\n}}");
        let as_statement = format!("fun f(x: Any) {{\n    {body}\n    g()\n}}");
        assert_eq!(
            first_skeleton(&as_initializer, SyntaxKind::WHEN_EXPR),
            first_skeleton(&as_statement, SyntaxKind::WHEN_EXPR)
        );

        let body = "try {\n        a()\n    } catch (e: E) {\n        b()\n    } finally {\n        c()\n    }";
        let as_initializer = format!("fun f() {{\n    val r = {body}\n    g()\n}}");
        let as_statement = format!("fun f() {{\n    {body}\n    g()\n}}");
        assert_eq!(
            first_skeleton(&as_initializer, SyntaxKind::TRY_EXPR),
            first_skeleton(&as_statement, SyntaxKind::TRY_EXPR)
        );
    }

    // ------------------------------------------------- newline termination

    /// Kotlin has no statement terminator, so the initializer must not swallow
    /// the next line. The Pratt loop stops at any token that is neither an
    /// infix nor a postfix operator, which covers the common cases below.
    #[test]
    fn newline_terminates_the_initializer_before_a_call_statement() {
        assert_eq!(
            block_statement_kinds("fun f() {\n    val x = a\n    f()\n}"),
            vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::EXPR_STMT]
        );
    }

    #[test]
    fn newline_terminates_the_initializer_before_other_statement_forms() {
        for (src, expected) in [
            (
                "fun f() {\n    val x = a\n    val y = b\n}",
                vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::PROPERTY_DECL],
            ),
            (
                "fun f() {\n    val x = a\n    return x\n}",
                vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::RETURN_EXPR],
            ),
            (
                "fun f() {\n    val x = a\n    if (x) g()\n}",
                vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::IF_EXPR],
            ),
            (
                "fun f() {\n    val x = a\n    while (x) { g() }\n}",
                vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::WHILE_EXPR],
            ),
            (
                "fun f() {\n    val x = a\n    for (i in y) { g() }\n}",
                vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::FOR_EXPR],
            ),
            (
                "fun f() {\n    val x = a\n    fun g() {}\n}",
                vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::FUNCTION_DECL],
            ),
            (
                "fun f() {\n    val x = a\n    \"lit\"\n}",
                vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::EXPR_STMT],
            ),
        ] {
            assert_eq!(block_statement_kinds(src), expected, "for {src:?}");
        }
    }

    #[test]
    fn semicolon_terminates_the_initializer_on_one_line() {
        assert_eq!(
            block_statement_kinds("fun f() { val x = a; g() }"),
            vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::EXPR_STMT]
        );
    }

    #[test]
    fn initializer_ending_at_the_block_close_brace_does_not_escape() {
        let src = "fun f() {\n    val x = a\n}\nfun g() {}";
        let root = tree(src);
        assert_eq!(
            root.descendants()
                .filter(|n| n.kind() == SyntaxKind::FUNCTION_DECL)
                .count(),
            2,
            "the initializer must not swallow the closing brace"
        );
    }

    /// A `.` on the following line *continues* the initializer. This is the
    /// one place the old token scan and the expression parser disagree: the
    /// scan broke at the newline and left `.bar()` to the block's error
    /// recovery, which produced an `ERROR` node that then swallowed the rest
    /// of the block. The expression parser continues the chain, which is both
    /// what Kotlin means and what statement position already did.
    #[test]
    fn dot_continuation_on_the_next_line_continues_the_initializer() {
        let src = "fun f() {\n    val x = foo()\n        .bar()\n    g()\n}";
        assert_eq!(
            block_statement_kinds(src),
            vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::EXPR_STMT],
            "the chained call belongs to the initializer, and g() stays a separate statement"
        );
        let root = tree(src);
        let prop = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PROPERTY_DECL)
            .expect("property");
        assert!(prop.text().to_string().contains(".bar()"));
        assert!(
            !root.descendants().any(|n| n.kind() == SyntaxKind::ERROR),
            "no error recovery node should be needed"
        );
    }

    /// Trailing-lambda ambiguity across a newline. Kotlin's own grammar allows
    /// `NL*` before a trailing lambda (`annotatedLambda: annotation* label?
    /// NL* lambdaLiteral`), and statement position already parsed it that way,
    /// so the initializer now does too. Pinned deliberately: the old token
    /// scan broke at the newline and left the braces to be parsed as a
    /// standalone block statement.
    #[test]
    fn lambda_on_the_next_line_is_absorbed_as_a_trailing_lambda() {
        let src = "fun f() {\n    val x = a\n    { g() }\n}";
        assert_eq!(
            block_statement_kinds(src),
            vec![SyntaxKind::PROPERTY_DECL],
            "pinned behaviour: the brace block binds to `a` as a trailing lambda"
        );
        assert_eq!(property_child_kinds(src), vec![SyntaxKind::CALL_EXPR]);
    }

    #[test]
    fn destructuring_declaration_still_parses_its_initializer() {
        let src = "fun f() {\n    val (a, b) = pair\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(
            block_statement_kinds(src),
            vec![SyntaxKind::PROPERTY_DECL, SyntaxKind::EXPR_STMT]
        );
    }

    #[test]
    fn explicitly_typed_local_still_parses_its_initializer() {
        let src =
            "fun f(x: Int) {\n    val r: Long = when (x) {\n        else -> 2L\n    }\n    g()\n}";
        assert_no_errors(src);
        assert_eq!(
            property_child_kinds(src),
            vec![SyntaxKind::TYPE_REF, SyntaxKind::WHEN_EXPR]
        );
    }
}

mod control_structure_body_tests {
    use super::*;
    use hyperion_kotlin::SyntaxNode;

    fn tree(source: &str) -> SyntaxNode {
        let (green, errors) = parse_source(source);
        assert!(errors.is_empty(), "parse errors for {source:?}: {errors:?}");
        SyntaxNode::new_root(green)
    }

    fn child_kinds(node: &SyntaxNode) -> Vec<SyntaxKind> {
        node.children().map(|child| child.kind()).collect()
    }

    #[test]
    fn semicolon_separates_when_entries() {
        let root = tree("fun f(x: Int) = when (x) { 1 -> 1; else -> 2 }");
        let when_expr = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::WHEN_EXPR)
            .expect("WHEN_EXPR");

        assert_eq!(
            child_kinds(&when_expr),
            vec![SyntaxKind::WHEN_ENTRY, SyntaxKind::WHEN_ENTRY]
        );
    }

    #[test]
    fn single_statement_loop_bodies_are_owned_by_the_loop() {
        let root = tree("fun f() { while (x) g() }");
        let block = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK)
            .expect("function BLOCK");
        assert_eq!(child_kinds(&block), vec![SyntaxKind::WHILE_EXPR]);
        let while_expr = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::WHILE_EXPR)
            .expect("WHILE_EXPR");
        assert_eq!(child_kinds(&while_expr), vec![SyntaxKind::EXPR_STMT]);

        let root = tree("fun f() { for (i in xs) g() }");
        let for_expr = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::FOR_EXPR)
            .expect("FOR_EXPR");
        assert_eq!(
            child_kinds(&for_expr),
            vec![SyntaxKind::FOR_LOOP_VAR, SyntaxKind::EXPR_STMT]
        );

        let root = tree("fun f() { while (a) while (b) f() }");
        let while_exprs: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::WHILE_EXPR)
            .collect();
        assert_eq!(while_exprs.len(), 2);
        assert_eq!(child_kinds(&while_exprs[0]), vec![SyntaxKind::WHILE_EXPR]);
        assert_eq!(child_kinds(&while_exprs[1]), vec![SyntaxKind::EXPR_STMT]);

        let root = tree("fun f() { do { g() } while (x) }");
        let do_while_expr = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::DO_WHILE_EXPR)
            .expect("DO_WHILE_EXPR");
        assert_eq!(child_kinds(&do_while_expr), vec![SyntaxKind::BLOCK]);
    }
}

#[test]
fn file_annotation_keeps_arguments_after_whitespace() {
    // `@file:` annotations are never types, so the space before `(` must not split
    // the argument list off the annotation.
    let source = "@file:JvmName (\"Runner\")\npackage declared.path\nfun main() {}\n";
    let tree = hyperion_kotlin::parse(source);
    let root = hyperion_kotlin::SyntaxNode::new_root(tree.green_node);

    let annotation = root
        .children()
        .find(|node| node.kind() == hyperion_kotlin::SyntaxKind::ANNOTATION)
        .expect("file annotation is a direct child of the file");
    assert!(
        annotation
            .children()
            .any(|node| node.kind() == hyperion_kotlin::SyntaxKind::ANNOTATION_ARG_LIST),
        "argument list belongs to the annotation: {annotation:#?}"
    );
    assert!(
        root.children_with_tokens()
            .all(|element| element.kind() != hyperion_kotlin::SyntaxKind::L_PAREN),
        "no stray parenthesis at file level"
    );
    assert_eq!(root.text().to_string(), source, "tree stays lossless");
}

#[test]
fn primary_constructors_with_modifiers_annotations_and_defaults_parse_cleanly() {
    let sources = [
        "class A(private val x: Int, internal var y: String = \"\")\n",
        "class B(@Named(\"a\") val x: Int, vararg val rest: String)\n",
        "class C private constructor(val x: Int)\n",
        "class D constructor(val x: Int, y: Int = 2)\n",
        "data class E(val x: Int, val y: List<Map<String, Int>>?)\n",
        "class F(vararg values: Int, @Suppress(\"x\") val tag: String = \"t\")\n",
    ];
    for source in sources {
        let tree = hyperion_kotlin::parse(source);
        assert!(
            tree.errors.is_empty(),
            "unexpected parse errors for {source:?}: {:?}",
            tree.errors
        );
        let root = hyperion_kotlin::SyntaxNode::new_root(tree.green_node);
        assert_eq!(root.text().to_string(), source, "tree stays lossless");
    }
}
