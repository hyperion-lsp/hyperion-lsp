// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for Kotlin symbol extraction

use hyperion_kotlin::SymbolExtractor;
use lsp_types::SymbolKind;

#[test]
fn test_class_members() {
    let source = r#"
        package com.example

        class User {
            val name: String = ""
            var age: Int = 0

            fun greet(): String {
                return "Hello"
            }
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    // Should have: class + 2 properties + 1 method = 4 symbols
    assert!(
        result.symbols.len() >= 4,
        "Expected at least 4 symbols, got {}",
        result.symbols.len()
    );

    let class_sym = result
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::CLASS)
        .expect("Should find User class");
    assert_eq!(class_sym.fqn, "com.example.User");

    let property_count = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::PROPERTY)
        .count();
    assert_eq!(property_count, 2, "Expected 2 properties");

    let method_count = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::FUNCTION)
        .count();
    assert_eq!(method_count, 1, "Expected 1 method");
}

#[test]
fn test_nested_class() {
    let source = r"
        package com.example

        class Outer {
            class Inner
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let outer_sym = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "Outer" && s.kind == SymbolKind::CLASS)
        .expect("Should find Outer class");
    assert_eq!(outer_sym.fqn, "com.example.Outer");

    let inner_sym = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "Inner" && s.kind == SymbolKind::CLASS)
        .expect("Should find Inner class");
    assert_eq!(inner_sym.fqn, "com.example.Outer.Inner");
    assert_eq!(inner_sym.container, Some("com.example.Outer".into()));
}

#[test]
fn test_companion_object() {
    let source = r"
        package com.example

        class User {
            companion object {
                fun create(): User {
                    return User()
                }
            }
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let companion_sym = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "Companion" && s.kind == SymbolKind::OBJECT)
        .expect("Should find Companion object");
    assert_eq!(companion_sym.fqn, "com.example.User.Companion");
    assert_eq!(companion_sym.container, Some("com.example.User".into()));

    // Check companion object function
    let create_func = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "create" && s.kind == SymbolKind::FUNCTION)
        .expect("Should find create function");
    assert_eq!(create_func.fqn, "com.example.User.Companion.create");
}

#[test]
fn test_sealed_class() {
    let source = r"
        package com.example

        sealed class Result
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let class_sym = result
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::CLASS)
        .expect("Should find Result class");
    assert_eq!(class_sym.fqn, "com.example.Result");
    assert!(class_sym.modifiers.is_sealed, "Should be marked as sealed");
}

#[test]
fn test_multiple_top_level_declarations() {
    let source = r#"
        package com.example

        val PI = 3.14159

        fun greet(name: String): String {
            return "Hello, $name"
        }

        class User

        object Config

        interface Service
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    assert!(result
        .symbols
        .iter()
        .any(|s| s.simple_name == "PI" && s.kind == SymbolKind::PROPERTY));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.simple_name == "greet" && s.kind == SymbolKind::FUNCTION));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.simple_name == "User" && s.kind == SymbolKind::CLASS));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.simple_name == "Config" && s.kind == SymbolKind::OBJECT));
    assert!(result
        .symbols
        .iter()
        .any(|s| s.simple_name == "Service" && s.kind == SymbolKind::INTERFACE));
}

#[test]
fn test_bodyless_primary_constructor_class_does_not_capture_top_level_siblings() {
    let source = r#"
        package com.example

        class Point(val x: Int, val y: Int)
        fun distance(): Int = 0
        val origin = Point(0, 0)
        class Line
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    for expected in [
        "com.example.Point",
        "com.example.Point.x",
        "com.example.Point.y",
        "com.example.distance",
        "com.example.origin",
        "com.example.Line",
    ] {
        assert!(
            result.symbols.iter().any(|symbol| symbol.fqn == expected),
            "missing expected top-level or Point symbol {expected}; got: {:?}",
            result
                .symbols
                .iter()
                .map(|symbol| &symbol.fqn)
                .collect::<Vec<_>>()
        );
    }
    assert!(result.symbols.iter().all(|symbol| !matches!(
        symbol.fqn.as_str(),
        "com.example.Point.distance" | "com.example.Point.origin" | "com.example.Point.Line"
    )));
}

#[test]
fn test_several_bodyless_classes_remain_top_level_siblings() {
    let source = r#"
        package com.example

        class First(val id: Int)
        class Second(var name: String)
        class Third
        fun after(): String = "ok"
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let fqns: Vec<_> = result
        .symbols
        .iter()
        .map(|symbol| symbol.fqn.as_str())
        .collect();
    assert!(fqns.contains(&"com.example.First"));
    assert!(fqns.contains(&"com.example.Second"));
    assert!(fqns.contains(&"com.example.Third"));
    assert!(fqns.contains(&"com.example.after"));
    assert!(!fqns
        .iter()
        .any(|fqn| fqn.contains("First.Second") || fqn.contains("Second.Third")));
}

#[test]
fn test_bodyless_class_inside_object_keeps_object_siblings_and_constructor_properties() {
    let source = r#"
        package com.example

        object Shapes {
            class Point(private val x: Int, var y: Int, label: String)
            fun count(): Int = 0
        }
        fun after(): Int = 1
    "#;

    let result = SymbolExtractor::extract(source).unwrap();
    let point_x = result
        .symbols
        .iter()
        .find(|symbol| symbol.fqn == "com.example.Shapes.Point.x")
        .expect("primary-constructor val must be indexed as a property");
    assert_eq!(point_x.kind, SymbolKind::PROPERTY);
    assert_eq!(point_x.return_type.as_deref(), Some("Int"));
    assert!(point_x.modifiers.is_private);
    assert!(result
        .symbols
        .iter()
        .any(|symbol| symbol.fqn == "com.example.Shapes.Point.y"
            && symbol.kind == SymbolKind::PROPERTY));
    assert!(!result
        .symbols
        .iter()
        .any(|symbol| symbol.fqn == "com.example.Shapes.Point.label"));
    assert!(result
        .symbols
        .iter()
        .any(|symbol| symbol.fqn == "com.example.Shapes.count"));
    assert!(result
        .symbols
        .iter()
        .any(|symbol| symbol.fqn == "com.example.after"));
}

#[test]
fn test_import_extraction() {
    let source = r"
        package com.example

        import kotlin.collections.List
        import kotlin.collections.*

        class Foo
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.package.as_deref(), Some("com.example"));
    assert!(result.imports.explicit.contains_key("List"));
    assert_eq!(
        result.imports.explicit.get("List"),
        Some(&"kotlin.collections.List".to_string())
    );
    assert!(result
        .imports
        .wildcards
        .contains(&"kotlin.collections".to_string()));
}

#[test]
fn test_modifiers_extraction() {
    let source = r"
        package com.example

        public open class Base

        private data class User(val name: String)

        protected abstract class Service

        internal interface Repository
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let base = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "Base")
        .expect("Should find Base class");
    assert!(base.modifiers.is_public);
    assert!(base.modifiers.is_open);

    let user = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "User")
        .expect("Should find User class");
    assert!(user.modifiers.is_private);
    assert!(user.modifiers.is_data);

    let service = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "Service")
        .expect("Should find Service class");
    assert!(service.modifiers.is_protected);
    assert!(service.modifiers.is_abstract);

    let repo = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "Repository")
        .expect("Should find Repository interface");
    assert!(repo.modifiers.is_internal);
}

#[test]
fn test_function_return_type() {
    let source = r#"
        package com.example

        fun getString(): String {
            return ""
        }

        fun getInt(): Int {
            return 0
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    let get_string = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "getString")
        .expect("Should find getString");
    assert_eq!(get_string.return_type, Some("String".into()));

    let get_int = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "getInt")
        .expect("Should find getInt");
    assert_eq!(get_int.return_type, Some("Int".into()));
}

#[test]
fn test_property_type() {
    let source = r#"
        package com.example

        val name: String = ""
        var count: Int = 0
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    let name_prop = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "name")
        .expect("Should find name property");
    assert_eq!(name_prop.return_type, Some("String".into()));

    let count_prop = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "count")
        .expect("Should find count property");
    assert_eq!(count_prop.return_type, Some("Int".into()));
}

#[test]
fn test_interface_members() {
    let source = r"
        package com.example

        interface Service {
            fun execute(): String
            val config: Config
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let interface_sym = result
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::INTERFACE)
        .expect("Should find Service interface");
    assert_eq!(interface_sym.fqn, "com.example.Service");

    let execute_func = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "execute" && s.kind == SymbolKind::FUNCTION)
        .expect("Should find execute function");
    assert_eq!(execute_func.container, Some("com.example.Service".into()));

    let config_prop = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "config" && s.kind == SymbolKind::PROPERTY)
        .expect("Should find config property");
    assert_eq!(config_prop.container, Some("com.example.Service".into()));
}

#[test]
fn test_object_members() {
    let source = r"
        package com.example

        object Config {
            val timeout: Int = 30

            fun reset() {
            }
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let object_sym = result
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::OBJECT)
        .expect("Should find Config object");
    assert_eq!(object_sym.fqn, "com.example.Config");

    let timeout_prop = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "timeout")
        .expect("Should find timeout property");
    assert_eq!(timeout_prop.fqn, "com.example.Config.timeout");

    let reset_func = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "reset")
        .expect("Should find reset function");
    assert_eq!(reset_func.fqn, "com.example.Config.reset");
}

// === Property Accessor Tests ===

#[test]
fn test_property_with_getter() {
    let source = r#"
        package com.example

        class User {
            val fullName: String
                get() = "$firstName $lastName"
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    let property = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "fullName")
        .expect("Should find fullName property");

    assert!(
        property.custom_getter,
        "Should mark as having custom getter"
    );
    assert!(!property.custom_setter, "Should not have custom setter");
    assert_eq!(property.return_type, Some("String".into()));
    assert_eq!(
        property.getter_visibility, None,
        "Getter should have default visibility"
    );
}

#[test]
fn test_property_with_private_setter() {
    let source = r"
        package com.example

        class Counter {
            var count: Int = 0
                private set
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let property = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "count")
        .expect("Should find count property");

    assert!(!property.custom_getter, "Should not have custom getter");
    assert!(
        property.custom_setter,
        "Should mark as having custom setter"
    );
    assert_eq!(property.setter_visibility, Some("private".into()));
}

#[test]
fn test_property_with_both_accessors() {
    let source = r"
        package com.example

        class Storage {
            var data: Int = 0
                get() = field
                set(value) { field = value }
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let property = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "data")
        .expect("Should find data property");

    assert!(property.custom_getter, "Should have custom getter");
    assert!(property.custom_setter, "Should have custom setter");
    assert_eq!(
        property.getter_visibility, None,
        "Getter should have default visibility"
    );
    assert_eq!(
        property.setter_visibility, None,
        "Setter should have default visibility"
    );
}

#[test]
fn test_top_level_property_with_getter() {
    let source = r"
        package com.example

        val PI: Double
            get() = 3.14159
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let property = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "PI")
        .expect("Should find PI property");

    assert!(
        property.custom_getter,
        "Top-level property should have custom getter"
    );
    assert!(!property.custom_setter, "Should not have custom setter");
    assert_eq!(property.return_type, Some("Double".into()));
}

#[test]
fn test_property_without_accessors() {
    let source = r#"
        package com.example

        class User {
            val name: String = "John"
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    let property = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "name")
        .expect("Should find name property");

    assert!(!property.custom_getter, "Should not have custom getter");
    assert!(!property.custom_setter, "Should not have custom setter");
    assert_eq!(property.getter_visibility, None);
    assert_eq!(property.setter_visibility, None);
}

#[test]
fn test_soft_keyword_property_name() {
    let source = r#"
        package com.example

        class User {
            val data: String = "test"
            var sealed: Int = 0
                get() = field
                set(value) { field = value }
        }
    "#;

    let result = SymbolExtractor::extract(source).unwrap();

    // 'data' is a soft keyword (DATA_KW) but valid as property name
    let data_prop = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "data")
        .expect("Should find 'data' property with soft keyword name");
    assert_eq!(data_prop.return_type, Some("String".into()));

    // 'sealed' is a soft keyword (SEALED_KW) with accessors
    let sealed_prop = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "sealed")
        .expect("Should find 'sealed' property with soft keyword name");
    assert_eq!(sealed_prop.return_type, Some("Int".into()));
    assert!(sealed_prop.custom_getter, "Should have custom getter");
    assert!(sealed_prop.custom_setter, "Should have custom setter");
}

#[test]
fn test_property_with_protected_getter() {
    let source = r"
        package com.example

        class Data {
            var value: Int = 0
                protected get() = field
                protected set
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    let property = result
        .symbols
        .iter()
        .find(|s| s.simple_name == "value")
        .expect("Should find value property");

    assert!(property.custom_getter, "Should have custom getter");
    assert!(property.custom_setter, "Should have custom setter");
    assert_eq!(property.getter_visibility, Some("protected".into()));
    assert_eq!(property.setter_visibility, Some("protected".into()));
}

#[test]
fn test_class_with_primary_constructor() {
    let source = r"
        package com.example

        class Wrapper<T>(private val value: T) {
            fun get(): T = value
            fun getOrNull(): T? = value
        }
    ";

    let result = SymbolExtractor::extract(source).unwrap();

    println!("Extracted symbols:");
    for sym in &result.symbols {
        println!(
            "  - {} ({:?}) container={:?}",
            sym.fqn, sym.kind, sym.container
        );
    }

    // Should have: class + 2 methods = 3 symbols (value property is in constructor)
    let methods: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::FUNCTION)
        .collect();

    assert_eq!(
        methods.len(),
        2,
        "Expected 2 methods (get and getOrNull), got {}: {:?}",
        methods.len(),
        methods.iter().map(|m| &m.fqn).collect::<Vec<_>>()
    );
}

#[test]
fn extension_function_return_type_is_not_its_receiver() {
    use hyperion_kotlin::SymbolExtractor;
    let extraction = SymbolExtractor::extract(
        "package p\n\nfun Iterable<Money>.total(): Money = TODO()\nfun String?.orEmpty2(): String = \"\"\nfun List<String>.names() = 1\n",
    )
    .unwrap();
    let find = |name: &str| {
        extraction
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name == name)
            .unwrap_or_else(|| panic!("no symbol {name}"))
    };
    assert_eq!(find("total").return_type.as_deref(), Some("Money"));
    assert_eq!(
        find("total").receiver_type.as_deref(),
        Some("Iterable<Money>")
    );
    assert_eq!(find("orEmpty2").return_type.as_deref(), Some("String"));
    assert_eq!(find("orEmpty2").receiver_type.as_deref(), Some("String?"));
    assert_eq!(
        find("names").return_type,
        None,
        "an expression-bodied function without a declared type has no return type"
    );
}

#[test]
fn classes_after_an_enum_are_not_enums() {
    use hyperion_kotlin::SymbolExtractor;
    let extraction = SymbolExtractor::extract(
        "package p\n\nenum class Kind { A, B }\n\nclass Invoice(val id: String)\n\nannotation class Marker\n\nclass After\n",
    )
    .unwrap();
    let kind = |name: &str| {
        extraction
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name == name)
            .map_or_else(|| panic!("no symbol {name}"), |symbol| symbol.kind)
    };
    assert_eq!(kind("Kind"), lsp_types::SymbolKind::ENUM);
    assert_eq!(kind("Invoice"), lsp_types::SymbolKind::CLASS);
    assert_eq!(kind("Marker"), lsp_types::SymbolKind::INTERFACE);
    assert_eq!(kind("After"), lsp_types::SymbolKind::CLASS);
}

/// The declared type of member `name` in `source`.
fn declared_type(source: &str, name: &str) -> Option<String> {
    SymbolExtractor::extract(source)
        .unwrap()
        .symbols
        .into_iter()
        .find(|symbol| symbol.simple_name == name)
        .unwrap_or_else(|| panic!("no symbol {name}"))
        .return_type
}

/// A qualified name inside a type argument keeps its dots, at any
/// depth, so the nested argument names its class (`java.util.ArrayList<Int>`,
/// not `javautilArrayList<Int>`) and keeps its own arguments.
#[test]
fn qualified_type_argument_keeps_its_dots() {
    let source = "package p\n\nclass Table<K, V> {\n    fun pairs(): java.util.HashMap<String, java.util.ArrayList<Int>> = TODO()\n    fun rows(): java.util.HashMap<K, java.util.ArrayList<V>> = TODO()\n    val entries: List<java.util.Map.Entry<kotlin.String, V>>? = null\n}\n";
    assert_eq!(
        declared_type(source, "pairs").as_deref(),
        Some("java.util.HashMap<String, java.util.ArrayList<Int>>")
    );
    assert_eq!(
        declared_type(source, "rows").as_deref(),
        Some("java.util.HashMap<K, java.util.ArrayList<V>>")
    );
    assert_eq!(
        declared_type(source, "entries").as_deref(),
        Some("List<java.util.Map.Entry<kotlin.String, V>>?")
    );
}

/// A function type inside a type argument keeps its parameters and arrow.
#[test]
fn function_type_argument_keeps_its_parentheses_and_arrow() {
    let source = "package p\n\nclass Box {\n    fun handlers(): List<(String) -> Int> = TODO()\n    fun run(): Map<String, () -> Unit> = TODO()\n}\n";
    assert_eq!(
        declared_type(source, "handlers").as_deref(),
        Some("List<(String) -> Int>")
    );
    assert_eq!(
        declared_type(source, "run").as_deref(),
        Some("Map<String, () -> Unit>")
    );
}

/// A star projection is kept, so `List<*>` is not stored as `List<>`.
#[test]
fn star_projection_type_argument_is_kept() {
    let source = "package p\n\nclass Box {\n    fun any(): List<*> = TODO()\n    fun nested(): Map<String, MutableList<*>> = TODO()\n}\n";
    assert_eq!(declared_type(source, "any").as_deref(), Some("List<*>"));
    assert_eq!(
        declared_type(source, "nested").as_deref(),
        Some("Map<String, MutableList<*>>")
    );
}

/// A use-site variance on a type argument is kept, so a projected
/// argument is not read as invariant.
#[test]
fn type_argument_variance_is_kept() {
    let source = "package p\n\nclass Box<T> {\n    fun items(): List<out T> = TODO()\n    fun sink(): java.util.Comparator<in T> = TODO()\n    val nested: Map<String, MutableList<out java.lang.CharSequence>> = TODO()\n}\n";
    assert_eq!(
        declared_type(source, "items").as_deref(),
        Some("List<out T>")
    );
    assert_eq!(
        declared_type(source, "sink").as_deref(),
        Some("java.util.Comparator<in T>")
    );
    assert_eq!(
        declared_type(source, "nested").as_deref(),
        Some("Map<String, MutableList<out java.lang.CharSequence>>")
    );
}

/// A simple type argument and a qualified top-level type are read
/// as before.
#[test]
fn simple_type_argument_and_qualified_top_level_type_are_unchanged() {
    let source = "package p\n\nclass Box {\n    fun names(): List<String> = TODO()\n    fun table(): Map<String, List<Int?>>? = TODO()\n    fun date(): java.util.Date? = null\n    val map: java.util.HashMap<String, Int> = TODO()\n}\n";
    assert_eq!(
        declared_type(source, "names").as_deref(),
        Some("List<String>")
    );
    assert_eq!(
        declared_type(source, "table").as_deref(),
        Some("Map<String, List<Int?>>?")
    );
    assert_eq!(
        declared_type(source, "date").as_deref(),
        Some("java.util.Date?")
    );
    assert_eq!(
        declared_type(source, "map").as_deref(),
        Some("java.util.HashMap<String, Int>")
    );
}
