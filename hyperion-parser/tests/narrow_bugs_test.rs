// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#[cfg(test)]
mod narrow {
    use hyperion_parser::lexer::Lexer;
    use hyperion_parser::parser::Parser;

    #[test]
    fn test_type_pattern_basic() {
        // This works
        let source = r#"
class Test {
    void test(Object obj) {
        switch (obj) {
            case String s -> System.out.println(s);
            default -> {}
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "Basic type pattern should work");
    }

    #[test]
    fn test_null_pattern() {
        let source = r#"
class Test {
    void test(Object obj) {
        switch (obj) {
            case null -> System.out.println("null");
            default -> {}
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);
        eprintln!("Null pattern errors: {:?}", errors);
        assert!(errors.is_empty(), "Null pattern should work");
    }

    #[test]
    fn test_underscore_pattern() {
        let source = r#"
class Test {
    void test(Object obj) {
        switch (obj) {
            case String _ -> System.out.println("string");
            default -> {}
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);
        eprintln!("Underscore pattern errors: {:?}", errors);
        // This will likely fail - underscore as variable name
    }

    #[test]
    fn test_record_pattern_simple() {
        let source = r#"
record Point(int x, int y) {}
class Test {
    void test(Object obj) {
        switch (obj) {
            case Point(int x, int y) -> System.out.println(x);
            default -> {}
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);
        eprintln!("Record pattern errors: {:?}", errors);
        // This will fail - record patterns not implemented
    }

    #[test]
    fn test_when_guard() {
        let source = r#"
class Test {
    void test(Object obj) {
        switch (obj) {
            case String s when s.length() > 5 -> System.out.println("long");
            default -> {}
        }
    }
}
"#;
        let tokens = Lexer::new(source).collect();
        let (_, errors) = Parser::parse(tokens);
        eprintln!("When guard errors: {:?}", errors);
        // This will fail - when keyword not recognized
    }
}
