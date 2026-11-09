// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::lexer::Lexer;
use hyperion_parser::parser::Parser;
use hyperion_parser::syntax::SyntaxNode;

/// Helper function to parse source code and return the root node
fn parse(source: &str) -> (SyntaxNode, Vec<hyperion_parser::parser::ParseError>) {
    let tokens = Lexer::new(source).collect();
    let (green_node, errors) = Parser::parse(tokens);
    (SyntaxNode::new_root(green_node), errors)
}

#[test]
fn test_super_constructor_call_no_args() {
    let input = r"
class Child extends Parent {
    Child() {
        super();
    }
}
";
    let (root, errors) = parse(input);
    let tree = format!("{root:#?}");
    println!("{tree}");

    if !errors.is_empty() {
        println!("Parse errors:");
        for error in &errors {
            println!("  {error:?}");
        }
    }

    // Should not have errors for super()
    assert_eq!(
        errors.len(),
        0,
        "Expected no parse errors for super(), got: {errors:?}"
    );

    // Check that we have a super constructor call
    let has_super = tree.contains("SUPER");
    assert!(has_super, "Expected to find SUPER in parse tree");
}

#[test]
fn test_super_constructor_call_with_args() {
    let input = r"
class Child extends Parent {
    Child(String name) {
        super(name);
    }
}
";
    let (root, errors) = parse(input);
    let tree = format!("{root:#?}");
    println!("{tree}");

    if !errors.is_empty() {
        println!("Parse errors:");
        for error in &errors {
            println!("  {error:?}");
        }
    }

    // Should not have errors for super(arg)
    assert_eq!(
        errors.len(),
        0,
        "Expected no parse errors for super(name), got: {errors:?}"
    );
}

#[test]
fn test_super_method_call() {
    let input = r"
class Child extends Parent {
    void method() {
        super.toString();
    }
}
";
    let (root, errors) = parse(input);
    let tree = format!("{root:#?}");
    println!("{tree}");

    if !errors.is_empty() {
        println!("Parse errors:");
        for error in &errors {
            println!("  {error:?}");
        }
    }

    // Should not have errors for super.method()
    assert_eq!(
        errors.len(),
        0,
        "Expected no parse errors for super.toString(), got: {errors:?}"
    );
}

#[test]
fn test_super_field_access() {
    let input = r"
class Child extends Parent {
    void method() {
        String x = super.name;
    }
}
";
    let (root, errors) = parse(input);
    let tree = format!("{root:#?}");
    println!("{tree}");

    if !errors.is_empty() {
        println!("Parse errors:");
        for error in &errors {
            println!("  {error:?}");
        }
    }

    // Should not have errors for super.field
    assert_eq!(
        errors.len(),
        0,
        "Expected no parse errors for super.name, got: {errors:?}"
    );
}

#[test]
fn test_super_in_return_statement() {
    let input = r"
class Child extends Parent {
    Parent getParent() {
        return super;
    }
}
";
    let (root, errors) = parse(input);
    let tree = format!("{root:#?}");
    println!("{tree}");

    // Just using super as an expression (uncommon but valid in some contexts)
    // This test documents current behavior
    println!("Super in return - errors: {errors:?}");
}

#[test]
fn test_complex_super_usage() {
    let input = r"
class Child extends Parent {
    Child(int x, String name) {
        super(x, name);
        super.init();
        int y = super.value + 1;
    }
}
";
    let (root, errors) = parse(input);
    let tree = format!("{root:#?}");
    println!("{tree}");

    if !errors.is_empty() {
        println!("Parse errors:");
        for error in &errors {
            println!("  {error:?}");
        }
    }

    // Should handle all super usages correctly
    assert_eq!(
        errors.len(),
        0,
        "Expected no parse errors for complex super usage, got: {errors:?}"
    );
}

#[test]
fn test_qualified_super_constructor_call() {
    let input = r"
class Outer {
    class Inner extends Parent {
        Inner() {
            Outer.this.super();
        }
    }
}
";
    let (root, errors) = parse(input);
    let tree = format!("{root:#?}");
    println!("{tree}");

    if !errors.is_empty() {
        println!("Parse errors:");
        for error in &errors {
            println!("  {error:?}");
        }
    }

    assert_eq!(
        errors.len(),
        0,
        "Expected no parse errors for qualified super(), got: {errors:?}"
    );

    let has_explicit = tree.contains("EXPLICIT_CONSTRUCTOR_INVOCATION");
    assert!(
        has_explicit,
        "Expected explicit constructor invocation for qualified super()"
    );
}
