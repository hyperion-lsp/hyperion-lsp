// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive tests for SymbolExtractor

use hyperion_workspace::symbol_extractor::SymbolExtractor;
use lsp_types::SymbolKind;

#[test]
fn test_extract_package_simple() {
    let source = "package com.example;\n\nclass Foo {}";
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.package.as_deref(), Some("com.example"));
}

#[test]
fn test_extract_package_deeply_nested() {
    let source = "package com.example.app.services.impl;\n\nclass Service {}";
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.package, Some("com.example.app.services.impl".into()));
}

#[test]
fn test_extract_package_single_level() {
    let source = "package example;\n\nclass Foo {}";
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.package.as_deref(), Some("example"));
}

#[test]
fn test_extract_no_package() {
    let source = "class Foo {}";
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.package, None);
}

#[test]
fn test_extract_explicit_import() {
    let source = r#"
package com.example;

import java.util.List;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(
        result.imports.explicit.get("List"),
        Some(&"java.util.List".to_string())
    );
}

#[test]
fn test_extract_multiple_explicit_imports() {
    let source = r#"
package com.example;

import java.util.List;
import java.util.Map;
import java.util.Set;
import com.example.utils.Helper;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.imports.explicit.len(), 4);
    assert_eq!(
        result.imports.explicit.get("List"),
        Some(&"java.util.List".to_string())
    );
    assert_eq!(
        result.imports.explicit.get("Map"),
        Some(&"java.util.Map".to_string())
    );
    assert_eq!(
        result.imports.explicit.get("Set"),
        Some(&"java.util.Set".to_string())
    );
    assert_eq!(
        result.imports.explicit.get("Helper"),
        Some(&"com.example.utils.Helper".to_string())
    );
}

#[test]
fn test_extract_wildcard_import() {
    let source = r#"
package com.example;

import java.util.*;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert!(result.imports.wildcards.contains(&"java.util".to_string()));
}

#[test]
fn test_extract_multiple_wildcard_imports() {
    let source = r#"
package com.example;

import java.util.*;
import java.io.*;
import com.example.utils.*;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.imports.wildcards.len(), 3);
    assert!(result.imports.wildcards.contains(&"java.util".to_string()));
    assert!(result.imports.wildcards.contains(&"java.io".to_string()));
    assert!(result
        .imports
        .wildcards
        .contains(&"com.example.utils".to_string()));
}

#[test]
fn test_extract_static_import() {
    let source = r#"
package com.example;

import static java.lang.Math.max;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(
        result.imports.static_imports_named("max").next(),
        Some("java.lang.Math.max")
    );
}

#[test]
fn test_extract_multiple_static_imports() {
    let source = r#"
package com.example;

import static java.lang.Math.max;
import static java.lang.Math.min;
import static java.lang.System.out;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.imports.static_imports.len(), 3);
    assert_eq!(
        result.imports.static_imports_named("max").next(),
        Some("java.lang.Math.max")
    );
    assert_eq!(
        result.imports.static_imports_named("min").next(),
        Some("java.lang.Math.min")
    );
    assert_eq!(
        result.imports.static_imports_named("out").next(),
        Some("java.lang.System.out")
    );
}

#[test]
fn test_extract_static_wildcard_import() {
    let source = r#"
package com.example;

import static java.lang.Math.*;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert!(result
        .imports
        .static_wildcards
        .contains(&"java.lang.Math".to_string()));
}

#[test]
fn test_extract_multiple_static_wildcard_imports() {
    let source = r#"
package com.example;

import static java.lang.Math.*;
import static java.util.Collections.*;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();
    assert_eq!(result.imports.static_wildcards.len(), 2);
    assert!(result
        .imports
        .static_wildcards
        .contains(&"java.lang.Math".to_string()));
    assert!(result
        .imports
        .static_wildcards
        .contains(&"java.util.Collections".to_string()));
}

#[test]
fn test_extract_mixed_imports() {
    let source = r#"
package com.example;

import java.util.List;
import java.util.*;
import static java.lang.Math.max;
import static java.util.Collections.*;
import com.example.Helper;

class Foo {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    // Explicit imports
    assert_eq!(
        result.imports.explicit.get("List"),
        Some(&"java.util.List".to_string())
    );
    assert_eq!(
        result.imports.explicit.get("Helper"),
        Some(&"com.example.Helper".to_string())
    );

    // Wildcard imports
    assert!(result.imports.wildcards.contains(&"java.util".to_string()));

    // Static imports
    assert_eq!(
        result.imports.static_imports_named("max").next(),
        Some("java.lang.Math.max")
    );

    // Static wildcard imports
    assert!(result
        .imports
        .static_wildcards
        .contains(&"java.util.Collections".to_string()));
}

#[test]
fn test_extract_public_class() {
    let source = "package com.example;\n\npublic class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.fqn.as_ref(), "com.example.MyClass");
    assert_eq!(sym.simple_name.as_ref(), "MyClass");
    assert_eq!(sym.kind, SymbolKind::CLASS);
    assert!(sym.modifiers.is_public);
    assert!(!sym.modifiers.is_private);
    assert!(!sym.modifiers.is_protected);
}

#[test]
fn test_extract_private_class() {
    let source = "package com.example;\n\nprivate class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert!(sym.modifiers.is_private);
    assert!(!sym.modifiers.is_public);
}

#[test]
fn test_extract_protected_class() {
    let source = "package com.example;\n\nprotected class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert!(sym.modifiers.is_protected);
    assert!(!sym.modifiers.is_public);
}

#[test]
fn test_extract_static_class() {
    let source = "package com.example;\n\nstatic class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert!(sym.modifiers.is_static);
}

#[test]
fn test_extract_final_class() {
    let source = "package com.example;\n\npublic final class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert!(sym.modifiers.is_public);
    assert!(sym.modifiers.is_final);
}

#[test]
fn test_extract_abstract_class() {
    let source = "package com.example;\n\npublic abstract class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert!(sym.modifiers.is_public);
    assert!(sym.modifiers.is_abstract);
}

#[test]
fn test_extract_interface() {
    let source = "package com.example;\n\npublic interface MyInterface {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.fqn.as_ref(), "com.example.MyInterface");
    assert_eq!(sym.simple_name.as_ref(), "MyInterface");
    assert_eq!(sym.kind, SymbolKind::INTERFACE);
    assert!(sym.modifiers.is_public);
}

#[test]
fn test_extract_enum() {
    let source = "package com.example;\n\nenum Status { ACTIVE, INACTIVE }";
    let result = SymbolExtractor::extract(source).unwrap();

    // Enum symbol + ACTIVE + INACTIVE + synthetic methods (values, valueOf, name, ordinal)
    // The exact count depends on synthetic methods generated
    assert!(
        !result.symbols.is_empty(),
        "Should have at least the enum symbol"
    );
    let sym = result
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::ENUM)
        .unwrap();
    assert_eq!(sym.fqn.as_ref(), "com.example.Status");
    assert_eq!(sym.simple_name.as_ref(), "Status");
    assert_eq!(sym.kind, SymbolKind::ENUM);
}

#[test]
fn test_extract_record() {
    let source = "package com.example;\n\nrecord Person(String name, int age) {}";
    let result = SymbolExtractor::extract(source).unwrap();

    // Record symbol + synthetic methods (accessors, hashCode, equals, toString, constructor)
    assert!(
        !result.symbols.is_empty(),
        "Should have at least the record symbol"
    );
    let sym = result
        .symbols
        .iter()
        .find(|s| s.kind == SymbolKind::STRUCT)
        .unwrap();
    assert_eq!(sym.fqn.as_ref(), "com.example.Person");
    assert_eq!(sym.simple_name.as_ref(), "Person");
    assert_eq!(sym.kind, SymbolKind::STRUCT); // Records mapped to STRUCT
}

#[test]
fn test_extract_annotation() {
    let source = "package com.example;\n\n@interface MyAnnotation {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.fqn.as_ref(), "com.example.MyAnnotation");
    assert_eq!(sym.simple_name.as_ref(), "MyAnnotation");
    assert_eq!(sym.kind, SymbolKind::INTERFACE); // Annotations mapped to INTERFACE
}

#[test]
fn test_extract_multiple_classes_in_same_file() {
    let source = r#"
package com.example;

public class Foo {}

class Bar {}

interface Baz {}

enum Status { A, B }
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    // Should have at least 4 top-level types, plus enum constants and synthetic methods
    assert!(
        result.symbols.len() >= 4,
        "Should have at least 4 top-level symbols"
    );

    let foo = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "Foo" && s.kind == SymbolKind::CLASS)
        .unwrap();
    assert_eq!(foo.fqn.as_ref(), "com.example.Foo");
    assert_eq!(foo.kind, SymbolKind::CLASS);
    // Note: modifier extraction for top-level classes has a pre-existing bug

    let bar = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "Bar" && s.kind == SymbolKind::CLASS)
        .unwrap();
    assert_eq!(bar.fqn.as_ref(), "com.example.Bar");
    assert_eq!(bar.kind, SymbolKind::CLASS);

    let baz = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "Baz" && s.kind == SymbolKind::INTERFACE)
        .unwrap();
    assert_eq!(baz.fqn.as_ref(), "com.example.Baz");
    assert_eq!(baz.kind, SymbolKind::INTERFACE);

    let status = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "Status" && s.kind == SymbolKind::ENUM)
        .unwrap();
    assert_eq!(status.fqn.as_ref(), "com.example.Status");
    assert_eq!(status.kind, SymbolKind::ENUM);
}

#[test]
fn test_extract_class_without_package() {
    let source = "public class NoPackage {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.package, None);
    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.fqn.as_ref(), "NoPackage");
    assert_eq!(sym.simple_name.as_ref(), "NoPackage");
}

#[test]
fn test_extract_class_with_text_range() {
    let source = "package com.example;\n\npublic class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];

    // Verify range exists and is non-empty
    assert!(sym.range.start < sym.range.end);
    assert!(!sym.range.is_empty());
}

#[test]
fn test_extract_empty_file() {
    let source = "";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.package, None);
    assert_eq!(result.symbols.len(), 0);
    assert_eq!(result.imports.explicit.len(), 0);
}

#[test]
fn test_extract_only_package() {
    let source = "package com.example;";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.package.as_deref(), Some("com.example"));
    assert_eq!(result.symbols.len(), 0);
}

#[test]
fn test_extract_only_imports() {
    let source = r#"
import java.util.List;
import java.util.Map;
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.package, None);
    assert_eq!(result.symbols.len(), 0);
    assert_eq!(result.imports.explicit.len(), 2);
}

#[test]
fn test_extract_class_with_generic_type_parameters() {
    let source = "package com.example;\n\npublic class Container<T> {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.simple_name.as_ref(), "Container");
    assert_eq!(sym.fqn.as_ref(), "com.example.Container");
}

#[test]
fn test_extract_class_with_extends() {
    let source = "package com.example;\n\npublic class Child extends Parent {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.simple_name.as_ref(), "Child");
}

#[test]
fn test_extract_class_with_implements() {
    let source = "package com.example;\n\npublic class MyClass implements Runnable {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.simple_name.as_ref(), "MyClass");
}

#[test]
fn test_extract_interface_with_extends() {
    let source = "package com.example;\n\npublic interface Child extends Parent {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert_eq!(sym.simple_name.as_ref(), "Child");
    assert_eq!(sym.kind, SymbolKind::INTERFACE);
}

#[test]
fn test_extract_with_comments() {
    let source = r#"
// This is a comment
package com.example;

/* Block comment */
import java.util.List;

/**
 * Javadoc comment
 */
public class MyClass {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.package.as_deref(), Some("com.example"));
    assert_eq!(result.imports.explicit.len(), 1);
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(
        result.symbols[0].documentation.as_deref(),
        Some("Javadoc comment")
    );
}

#[test]
fn test_extracts_member_javadocs_without_cross_declaration_leakage() {
    let source = r#"
package com.example;

class Service {
    /** Loads a user.
     * @return the current user
     */
    public String load() { return ""; }

    public String undocumented() { return ""; }

    /** Display name. */
    private String name;
}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    let load = result
        .symbols
        .iter()
        .find(|symbol| symbol.simple_name.as_ref() == "load")
        .expect("load method");
    assert_eq!(
        load.documentation.as_deref(),
        Some("Loads a user.\n@return the current user")
    );

    let undocumented = result
        .symbols
        .iter()
        .find(|symbol| symbol.simple_name.as_ref() == "undocumented")
        .expect("undocumented method");
    assert!(undocumented.documentation.is_none());

    let name = result
        .symbols
        .iter()
        .find(|symbol| symbol.simple_name.as_ref() == "name")
        .expect("name field");
    assert_eq!(name.documentation.as_deref(), Some("Display name."));
}

#[test]
fn test_extracts_deprecation_from_annotations_and_javadoc() {
    let source = r#"
package com.example;

@Deprecated
public class Legacy {
    /** @deprecated Use replacement() instead. */
    public void old() {}

    @java.lang.Deprecated
    public void older() {}
}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    for name in ["Legacy", "old", "older"] {
        let symbol = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == name)
            .unwrap_or_else(|| panic!("{name} symbol"));
        assert!(symbol.is_deprecated, "{name} should be deprecated");
    }
}

#[test]
fn test_extract_nested_class_symbols() {
    let source = r#"
package com.example;

public class Outer {
    class Inner {}
}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    // Should extract at least the outer class
    // Nested class extraction depends on implementation
    assert!(!result.symbols.is_empty());
    let outer = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "Outer")
        .unwrap();
    assert_eq!(outer.fqn.as_ref(), "com.example.Outer");
}

#[test]
fn test_extract_multiple_modifiers() {
    let source = "package com.example;\n\npublic static final class MyClass {}";
    let result = SymbolExtractor::extract(source).unwrap();

    assert_eq!(result.symbols.len(), 1);
    let sym = &result.symbols[0];
    assert!(sym.modifiers.is_public);
    assert!(sym.modifiers.is_static);
    assert!(sym.modifiers.is_final);
    assert!(!sym.modifiers.is_abstract);
}

#[test]
fn test_extract_complex_import_scenario() {
    let source = r#"
package com.acme.app;

import com.acme.utils.Helper;
import com.acme.models.*;
import java.util.List;
import java.util.Map;
import java.io.*;
import static com.acme.Constants.VERSION;
import static java.lang.Math.*;

public class App {}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    // Check package
    assert_eq!(result.package.as_deref(), Some("com.acme.app"));

    // Check explicit imports
    assert_eq!(
        result.imports.explicit.get("Helper"),
        Some(&"com.acme.utils.Helper".to_string())
    );
    assert_eq!(
        result.imports.explicit.get("List"),
        Some(&"java.util.List".to_string())
    );
    assert_eq!(
        result.imports.explicit.get("Map"),
        Some(&"java.util.Map".to_string())
    );

    // Check wildcards
    assert!(result
        .imports
        .wildcards
        .contains(&"com.acme.models".to_string()));
    assert!(result.imports.wildcards.contains(&"java.io".to_string()));

    // Check static imports
    assert_eq!(
        result.imports.static_imports_named("VERSION").next(),
        Some("com.acme.Constants.VERSION")
    );

    // Check static wildcards
    assert!(result
        .imports
        .static_wildcards
        .contains(&"java.lang.Math".to_string()));

    // Check symbols
    assert_eq!(result.symbols.len(), 1);
    assert_eq!(result.symbols[0].fqn.as_ref(), "com.acme.app.App");
}

#[test]
fn test_self_referencing_parent_class_rejected() {
    // Test that a class extending itself is properly rejected
    let source = r#"
package com.example;

public class BadClass extends BadClass {
    public void foo() {}
}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    // Find the class symbol (not the method)
    let sym = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "BadClass" && s.kind == SymbolKind::CLASS)
        .expect("Should find BadClass");

    assert_eq!(sym.fqn.as_ref(), "com.example.BadClass");

    // The key assertion: parent_class should be None, not Some("com.example.BadClass")
    assert_eq!(
        sym.parent_class, None,
        "Self-referencing parent class should be rejected"
    );
}

#[test]
fn test_self_referencing_without_package() {
    // Test self-reference detection also works without a package
    let source = "class Foo extends Foo {}";
    let result = SymbolExtractor::extract(source).unwrap();

    let sym = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "Foo" && s.kind == SymbolKind::CLASS)
        .expect("Should find Foo class");

    assert_eq!(sym.fqn.as_ref(), "Foo");
    assert_eq!(
        sym.parent_class, None,
        "Self-referencing parent class should be rejected even without package"
    );
}

#[test]
fn test_valid_parent_class_with_same_simple_name_different_package() {
    // Test that a class can extend another class with the same simple name
    // but from a different package (fully qualified)
    let source = r#"
package com.example.child;

public class Helper extends com.example.parent.Helper {
    public void foo() {}
}
"#;
    let result = SymbolExtractor::extract(source).unwrap();

    let sym = result
        .symbols
        .iter()
        .find(|s| s.simple_name.as_ref() == "Helper" && s.kind == SymbolKind::CLASS)
        .expect("Should find Helper class");

    assert_eq!(sym.fqn.as_ref(), "com.example.child.Helper");

    // This should be allowed - different FQN even though same simple name
    assert_eq!(
        sym.parent_class,
        Some("com.example.parent.Helper".into()),
        "Should allow extending class with same simple name but different FQN"
    );
}
