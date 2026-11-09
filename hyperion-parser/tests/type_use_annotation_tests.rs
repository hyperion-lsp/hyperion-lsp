// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! User-visible contracts for JSR 308 type-use annotation positions.

use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};

#[test]
fn type_use_annotations_parse_in_every_declaration_position() {
    let source = r#"
        import org.jspecify.annotations.Nullable;
        import java.util.Map;
        import java.util.function.Function;

        class Test {
            public @Nullable String returnType() { return null; }
            public void parameter(@Nullable String value) {}
            private @Nullable String field;
            private Function<String, @Nullable String> genericArgument;
            private Map.@Nullable Entry<String, String> qualifiedType;
        }
    "#;

    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    assert!(
        errors.is_empty(),
        "type-use annotation forms must parse without recovery: {errors:?}"
    );

    let root = SyntaxNode::new_root(green);
    let annotations = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::ANNOTATION)
        .count();
    assert_eq!(
        annotations, 5,
        "every @Nullable type use must remain represented in the syntax tree"
    );
}
