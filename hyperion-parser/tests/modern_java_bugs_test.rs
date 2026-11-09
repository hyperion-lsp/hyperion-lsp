// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#[cfg(test)]
mod bug_tests {
    use hyperion_parser::lexer::Lexer;
    use hyperion_parser::parser::Parser;

    #[test]
    fn bug1_record_with_compact_constructor() {
        // Bug: Compact constructors have no parameter list
        let source = r#"
public record Point(int x, int y) {
    public Point {
        if (x < 0) throw new IllegalArgumentException();
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);

        if !errors.is_empty() {
            eprintln!("BUG FOUND: Compact constructor parsing failed");
            eprintln!("Errors: {:?}", errors);
        }
        assert!(errors.is_empty());
    }

    #[test]
    fn bug2_switch_with_pattern_matching() {
        // Bug: Pattern matching in switch (Java 21)
        let source = r#"
class Test {
    String format(Object obj) {
        return switch (obj) {
            case Integer i -> "int: " + i;
            case String s -> "string: " + s;
            case null -> "null";
            default -> "unknown";
        };
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);

        if !errors.is_empty() {
            eprintln!("BUG FOUND: Switch with pattern matching failed");
            eprintln!("Errors: {:?}", errors);
        }
        assert!(errors.is_empty());
    }

    #[test]
    fn bug3_unnamed_patterns() {
        // Bug: Unnamed patterns with underscore (Java 21)
        let source = r#"
class Test {
    void process(Object obj) {
        if (obj instanceof String _) {
            System.out.println("It's a string");
        }
        
        switch (obj) {
            case Integer _ -> System.out.println("int");
            case String _ -> System.out.println("string");
            default -> {}
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);

        if !errors.is_empty() {
            eprintln!("BUG FOUND: Unnamed patterns (_) failed");
            eprintln!("Errors: {:?}", errors);
        }
        assert!(errors.is_empty());
    }

    #[test]
    fn bug4_record_patterns_in_switch() {
        // Bug: Record patterns in switch (Java 21)
        let source = r#"
record Point(int x, int y) {}

class Test {
    void process(Object obj) {
        switch (obj) {
            case Point(int x, int y) -> System.out.println(x + y);
            case null -> System.out.println("null");
            default -> {}
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);

        if !errors.is_empty() {
            eprintln!("BUG FOUND: Record patterns in switch failed");
            eprintln!("Errors: {:?}", errors);
        }
        assert!(errors.is_empty());
    }

    #[test]
    fn bug5_when_clause_in_switch() {
        // Bug: when clause in pattern matching (Java 21)
        let source = r#"
class Test {
    String classify(Object obj) {
        return switch (obj) {
            case String s when s.length() > 5 -> "long string";
            case String s -> "short string";
            case Integer i when i > 0 -> "positive";
            default -> "other";
        };
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);

        if !errors.is_empty() {
            eprintln!("BUG FOUND: when clause in switch failed");
            eprintln!("Errors: {:?}", errors);
        }
        assert!(errors.is_empty());
    }

    #[test]
    fn bug6_nested_record_patterns() {
        // Bug: Nested record patterns (Java 21)
        let source = r#"
record Point(int x, int y) {}
record Circle(Point center, int radius) {}

class Test {
    void process(Object obj) {
        if (obj instanceof Circle(Point(var x, var y), var r)) {
            System.out.println(x + y + r);
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);

        if !errors.is_empty() {
            eprintln!("BUG FOUND: Nested record patterns failed");
            eprintln!("Errors: {:?}", errors);
        }
        assert!(errors.is_empty());
    }
}
