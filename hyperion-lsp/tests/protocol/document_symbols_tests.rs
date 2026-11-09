// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for LSP document symbols (outline view)
//!
//! Tests the `textDocument/documentSymbol` LSP capability which provides
//! hierarchical document outline information for editors.

use hyperion_lsp::semantic_host::SemanticHost;
use hyperion_lsp::symbols::SymbolsProvider;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_workspace::Language;
use std::sync::Arc;
use tower_lsp::lsp_types::{DocumentSymbol, SymbolKind as LspSymbolKind};

/// Helper to get document symbols for a source string
fn get_document_symbols(source: &str) -> Vec<DocumentSymbol> {
    let host = SemanticHost::for_tests();
    let provider = SymbolsProvider::new();

    let file_id = FileId::new(0);

    // Set file text and language in database
    host.with_db(|db| {
        db.set_file_text(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);
    });

    provider
        .document_symbols(&host.read_now(), file_id, source)
        .unwrap()
}

#[test]
fn test_simple_class_outline() {
    let source = r"
package com.example;

public class User {
    private String name;
    private int age;

    public String getName() {
        return name;
    }

    public void setName(String name) {
        this.name = name;
    }
}
";

    let symbols = get_document_symbols(source);

    // Should have 1 top-level symbol (User class)
    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    assert_eq!(class_symbol.name.as_str(), "User");
    assert_eq!(class_symbol.kind, LspSymbolKind::CLASS);

    // Should have children (fields and methods)
    let children = class_symbol
        .children
        .as_ref()
        .expect("Class should have children");
    assert!(
        children.len() >= 2,
        "Should have at least 2 children (fields/methods)"
    );

    // Verify we have field symbols
    assert!(
        children.iter().any(|s| s.kind == LspSymbolKind::FIELD),
        "Should have at least one field"
    );

    // Verify we have method symbols
    assert!(
        children.iter().any(|s| s.kind == LspSymbolKind::METHOD),
        "Should have at least one method"
    );
}

#[test]
fn test_interface_outline() {
    let source = r"
package com.example;

public interface UserService {
    void saveUser(String name);
    String findUser(int id);
    void deleteUser(int id);
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let interface_symbol = &symbols[0];
    assert_eq!(interface_symbol.name.as_str(), "UserService");
    assert_eq!(interface_symbol.kind, LspSymbolKind::INTERFACE);

    // Interface methods should be in children
    let children = interface_symbol
        .children
        .as_ref()
        .expect("Interface should have children");
    assert!(children.len() >= 3, "Should have 3 methods");

    for child in children {
        assert_eq!(child.kind, LspSymbolKind::METHOD);
    }
}

#[test]
fn test_enum_outline() {
    let source = r"
package com.example;

public enum Status {
    ACTIVE,
    INACTIVE,
    PENDING,
    DELETED
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let enum_symbol = &symbols[0];
    assert_eq!(enum_symbol.name.as_str(), "Status");
    assert_eq!(enum_symbol.kind, LspSymbolKind::ENUM);

    // Enum constants should be in children
    let children = enum_symbol
        .children
        .as_ref()
        .expect("Enum should have children");
    assert!(children.len() >= 4, "Should have 4 enum constants");

    for child in children {
        assert_eq!(child.kind, LspSymbolKind::ENUM_MEMBER);
    }

    // Verify specific constants
    let constant_names: Vec<_> = children.iter().map(|c| c.name.as_str()).collect();
    assert!(constant_names.contains(&"ACTIVE"));
    assert!(constant_names.contains(&"INACTIVE"));
    assert!(constant_names.contains(&"PENDING"));
    assert!(constant_names.contains(&"DELETED"));
}

#[test]
fn test_multiple_classes_in_file() {
    let source = r"
package com.example;

class Helper {
    void help() {}
}

public class Main {
    private Helper helper;

    public void run() {}
}
";

    let symbols = get_document_symbols(source);

    // Should have 2 top-level symbols
    assert_eq!(symbols.len(), 2);

    let class_names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(class_names.contains(&"Helper"));
    assert!(class_names.contains(&"Main"));

    for symbol in &symbols {
        assert_eq!(symbol.kind, LspSymbolKind::CLASS);
    }
}

#[test]
fn test_nested_class_outline() {
    let source = r"
package com.example;

public class Outer {
    private int value;

    public class Inner {
        private String data;

        public void process() {}
    }

    public void outerMethod() {}
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let outer_symbol = &symbols[0];
    assert_eq!(outer_symbol.name.as_str(), "Outer");
    assert_eq!(outer_symbol.kind, LspSymbolKind::CLASS);

    let children = outer_symbol
        .children
        .as_ref()
        .expect("Outer class should have children");

    // Should have field, inner class, and method
    assert!(children.len() >= 2);

    // Find the inner class
    let inner_class = children
        .iter()
        .find(|s| s.kind == LspSymbolKind::CLASS && s.name == "Inner");

    if let Some(inner) = inner_class {
        assert_eq!(inner.name.as_str(), "Inner");

        // Inner class should have its own children
        let inner_children = inner
            .children
            .as_ref()
            .expect("Inner class should have children");
        assert!(
            !inner_children.is_empty(),
            "Inner class should have at least one member"
        );
    }
}

#[test]
fn test_abstract_class_outline() {
    let source = r"
package com.example;

public abstract class AbstractService {
    protected String name;

    public abstract void execute();

    public void initialize() {
        // implementation
    }
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    assert_eq!(class_symbol.name.as_str(), "AbstractService");
    assert_eq!(class_symbol.kind, LspSymbolKind::CLASS);

    let children = class_symbol
        .children
        .as_ref()
        .expect("Class should have children");

    // Should have field and methods
    assert!(
        children
            .iter()
            .filter(|s| s.kind == LspSymbolKind::METHOD)
            .count()
            >= 2,
        "Should have at least 2 methods"
    );
}

#[test]
fn test_class_with_constructor() {
    let source = r"
package com.example;

public class Person {
    private String name;

    public Person() {}

    public Person(String name) {
        this.name = name;
    }

    public String getName() {
        return name;
    }
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    assert_eq!(class_symbol.name.as_str(), "Person");

    let children = class_symbol
        .children
        .as_ref()
        .expect("Class should have children");

    // Should have methods (constructors may or may not be included depending on parser)
    assert!(
        children.iter().any(|s| s.kind == LspSymbolKind::METHOD),
        "Should have at least 1 method (getName)"
    );
}

#[test]
fn test_class_with_static_members() {
    let source = r#"
package com.example;

public class Config {
    private static final String VERSION = "1.0";
    private static int instanceCount = 0;

    public static void initialize() {}

    public void run() {}
}
"#;

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    assert_eq!(class_symbol.name.as_str(), "Config");

    let children = class_symbol
        .children
        .as_ref()
        .expect("Class should have children");

    // Should have static fields and methods
    assert!(
        children
            .iter()
            .filter(|s| s.kind == LspSymbolKind::FIELD)
            .count()
            >= 2,
        "Should have at least 2 fields"
    );

    assert!(
        children
            .iter()
            .filter(|s| s.kind == LspSymbolKind::METHOD)
            .count()
            >= 2,
        "Should have at least 2 methods"
    );
}

#[test]
fn test_enum_with_methods() {
    let source = r"
package com.example;

public enum Operation {
    ADD, SUBTRACT, MULTIPLY, DIVIDE;

    public void execute(int a, int b) {}

    public abstract double calculate(double x, double y);
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let enum_symbol = &symbols[0];
    assert_eq!(enum_symbol.name.as_str(), "Operation");
    assert_eq!(enum_symbol.kind, LspSymbolKind::ENUM);

    let children = enum_symbol
        .children
        .as_ref()
        .expect("Enum should have children");

    // Should have enum constants
    assert!(
        children
            .iter()
            .filter(|s| s.kind == LspSymbolKind::ENUM_MEMBER)
            .count()
            >= 4,
        "Should have 4 enum constants"
    );

    // Should also have methods
    assert!(
        children.iter().any(|s| s.kind == LspSymbolKind::METHOD),
        "Should have at least 1 method"
    );
}

#[test]
fn test_interface_with_default_methods() {
    let source = r#"
package com.example;

public interface Logger {
    void log(String message);

    default void info(String message) {
        log("INFO: " + message);
    }

    static void logError(String error) {
        System.err.println(error);
    }
}
"#;

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let interface_symbol = &symbols[0];
    assert_eq!(interface_symbol.name.as_str(), "Logger");
    assert_eq!(interface_symbol.kind, LspSymbolKind::INTERFACE);

    let children = interface_symbol
        .children
        .as_ref()
        .expect("Interface should have children");

    // Should have 3 methods (abstract, default, static)
    assert!(
        children
            .iter()
            .filter(|s| s.kind == LspSymbolKind::METHOD)
            .count()
            >= 3,
        "Should have at least 3 methods"
    );
}

#[test]
fn test_empty_class() {
    let source = r"
package com.example;

public class Empty {
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    assert_eq!(class_symbol.name.as_str(), "Empty");
    assert_eq!(class_symbol.kind, LspSymbolKind::CLASS);

    // Empty class may have no children or empty children vec
    if let Some(children) = &class_symbol.children {
        assert_eq!(children.len(), 0);
    }
}

#[test]
fn test_class_with_generics() {
    let source = r"
package com.example;

public class Container<T> {
    private T value;

    public T getValue() {
        return value;
    }

    public void setValue(T value) {
        this.value = value;
    }
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    assert_eq!(class_symbol.name.as_str(), "Container");
    assert_eq!(class_symbol.kind, LspSymbolKind::CLASS);

    let children = class_symbol
        .children
        .as_ref()
        .expect("Class should have children");

    // Should have field and methods
    assert!(
        children.len() >= 3,
        "Should have at least 1 field and 2 methods"
    );
}

#[test]
fn test_method_signatures_in_detail() {
    let source = r#"
package com.example;

public class Service {
    public void simpleMethod() {}

    public String methodWithReturn() {
        return "";
    }

    public void methodWithParams(String name, int age) {}

    private static final void complexMethod(String a, int b, boolean c) {}
}
"#;

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    let children = class_symbol
        .children
        .as_ref()
        .expect("Class should have children");

    // Find methods and check their detail field contains signature info
    let methods: Vec<_> = children
        .iter()
        .filter(|s| s.kind == LspSymbolKind::METHOD)
        .collect();

    assert!(methods.len() >= 4, "Should have 4 methods");

    // Methods should have detail containing signature
    for method in methods {
        assert!(
            method.detail.is_some(),
            "Method {} should have detail",
            method.name
        );
    }
}

#[test]
fn test_field_types_in_detail() {
    let source = r"
package com.example;

public class DataHolder {
    private String name;
    private int count;
    private boolean active;
    private String[] tags;
    private List<String> items;
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];
    let children = class_symbol
        .children
        .as_ref()
        .expect("Class should have children");

    // All children should be fields
    let fields: Vec<_> = children
        .iter()
        .filter(|s| s.kind == LspSymbolKind::FIELD)
        .collect();

    assert!(fields.len() >= 3, "Should have at least 3 fields");

    // Fields should have detail containing type info
    for field in fields {
        assert!(
            field.detail.is_some(),
            "Field {} should have detail",
            field.name
        );
    }
}

#[test]
fn test_record_class_outline() {
    let source = r"
package com.example;

public record Person(String name, int age) {
    public String displayName() {
        return name;
    }
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);
    let record_symbol = &symbols[0];
    assert_eq!(record_symbol.name.as_str(), "Person");
    assert_eq!(record_symbol.kind, LspSymbolKind::STRUCT);
    assert_eq!(record_symbol.detail.as_deref(), Some("record"));

    let children = record_symbol
        .children
        .as_ref()
        .expect("record components and methods must appear in the outline");
    let outline: Vec<_> = children
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.kind, symbol.detail.as_deref()))
        .collect();
    assert_eq!(
        outline,
        vec![
            ("name", LspSymbolKind::FIELD, Some("String")),
            ("age", LspSymbolKind::FIELD, Some("int")),
            (
                "displayName",
                LspSymbolKind::METHOD,
                Some("String displayName()")
            ),
        ]
    );
}

#[test]
fn test_annotation_outline() {
    let source = r#"
package com.example;

public @interface Audited {
    String value();
    int level() default 1;
}
"#;

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);
    let annotation = &symbols[0];
    assert_eq!(annotation.name, "Audited");
    assert_eq!(annotation.kind, LspSymbolKind::INTERFACE);
    assert_eq!(annotation.detail.as_deref(), Some("annotation"));
    let elements = annotation
        .children
        .as_ref()
        .expect("annotation elements must appear in the outline");
    let names: Vec<_> = elements
        .iter()
        .map(|element| element.name.as_str())
        .collect();
    assert_eq!(names, vec!["value", "level"]);
    assert!(elements
        .iter()
        .all(|element| element.kind == LspSymbolKind::METHOD));
}

#[test]
fn test_ranges_are_valid() {
    let source = r"
package com.example;

public class Test {
    private int value;

    public void method() {}
}
";

    let symbols = get_document_symbols(source);

    assert_eq!(symbols.len(), 1);

    let class_symbol = &symbols[0];

    // Verify range is valid
    assert!(class_symbol.range.start.line <= class_symbol.range.end.line);
    assert!(class_symbol.selection_range.start.line <= class_symbol.selection_range.end.line);

    // Verify children have valid ranges
    if let Some(children) = &class_symbol.children {
        for child in children {
            assert!(child.range.start.line <= child.range.end.line);
            assert!(child.selection_range.start.line <= child.selection_range.end.line);
        }
    }
}
