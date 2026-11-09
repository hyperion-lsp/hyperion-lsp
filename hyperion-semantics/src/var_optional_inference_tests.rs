// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#[cfg(test)]
mod tests {
    use crate::resolver::ResolverBuilder;
    use crate::symbol::Type;
    use hyperion_parser::{
        lexer::Lexer,
        parser::Parser,
        syntax::{AstNode, SourceFile, SyntaxKind, SyntaxNode},
    };
    use hyperion_workspace::WorkspaceIndex;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn parse_root(source: &str) -> SyntaxNode {
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(
            errors.is_empty(),
            "Parse errors in test source: {:?}",
            errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
        SourceFile::cast(SyntaxNode::new_root(green))
            .expect("Should parse source file")
            .syntax()
            .clone()
    }

    /// `process_local_var`/`process_field` defined the declared symbol and
    /// stopped, so nothing inside an initializer reached the symbol table. A
    /// lambda parameter declared there resolved to nothing everywhere it was
    /// used, which surfaced as "cannot find symbol" on the parameter.
    #[test]
    fn resolver_registers_lambda_parameters_declared_in_initializers() {
        let temp = TempDir::new().expect("temp dir");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));

        let source = r"
            package com.example;

            public class Main {
                java.util.function.Function<String, String> fieldTrim = fieldValue -> fieldValue.trim();

                public void test() {
                    java.util.function.Function<String, String> localTrim = localValue -> localValue.trim();
                }
            }
        ";

        let root = parse_root(source);
        let resolver =
            ResolverBuilder::with_workspace(crate::db::FileId(0), index).build_from_syntax(&root);

        for name in ["fieldValue", "localValue"] {
            assert!(
                resolver.symbol_table().find_symbol_anywhere(name).is_some(),
                "lambda parameter '{name}' declared in an initializer must be in the symbol table"
            );
        }
    }

    #[test]
    fn resolver_infers_optional_generic_through_var_chain() {
        let temp = TempDir::new().expect("temp dir");
        let root = temp.path().to_path_buf();

        let main_path = root.join("com/example/Main.java");
        let user_path = root.join("com/example/User.java");
        let optional_path = root.join("java/util/Optional.java");

        std::fs::create_dir_all(main_path.parent().expect("parent")).expect("mkdir main");
        std::fs::create_dir_all(user_path.parent().expect("parent")).expect("mkdir user");
        std::fs::create_dir_all(optional_path.parent().expect("parent")).expect("mkdir optional");

        let main_source = r#"
            package com.example;
            import java.util.*;

            public class Main {
                public void test() {
                    var optional = Optional.of(new User());
                    var user = optional.get();
                    user.getName();
                }
            }
        "#;

        let user_source = r#"
            package com.example;
            public class User {
                public String getName() { return ""; }
            }
        "#;

        let optional_source = r#"
            package java.util;
            public class Optional<T> {
                public static <T> Optional<T> of(T value) { return null; }
                public T get() { return null; }
            }
        "#;

        std::fs::write(&main_path, main_source).expect("write main");
        std::fs::write(&user_path, user_source).expect("write user");
        std::fs::write(&optional_path, optional_source).expect("write optional");

        let index = Arc::new(WorkspaceIndex::new(root.clone()));
        index
            .index_file_with_source(&main_path, main_source)
            .expect("index main");
        index
            .index_file_with_source(&user_path, user_source)
            .expect("index user");
        index
            .index_file_with_source(&optional_path, optional_source)
            .expect("index optional");

        let main_file_id = index
            .get_file_id(&main_path)
            .expect("main file id should exist");

        let root_node = parse_root(main_source);
        let builder = ResolverBuilder::with_workspace(main_file_id, index);
        let mut resolver = builder.build_from_syntax(&root_node);

        let optional_offset = main_source
            .find("optional.get")
            .expect("optional.get in source") as u32;
        let user_offset = main_source
            .find("user.getName")
            .expect("user.getName in source") as u32;

        let optional_scope = resolver
            .find_scope_at_offset(optional_offset)
            .expect("scope for optional");
        let user_scope = resolver
            .find_scope_at_offset(user_offset)
            .expect("scope for user");

        let optional_symbol = resolver
            .resolve_in_scope("optional", optional_scope)
            .expect("resolve optional");
        let user_symbol = resolver
            .resolve_in_scope("user", user_scope)
            .expect("resolve user");

        match optional_symbol.ty {
            Type::Class(ref ct) => {
                assert_eq!(ct.name, "Optional");
                assert_eq!(ct.type_arguments.len(), 1);
            }
            other => panic!("Expected optional to be Optional<...>, got {:?}", other),
        }

        match user_symbol.ty {
            Type::Class(ref ct) => {
                assert_eq!(ct.name, "User");
            }
            other => panic!("Expected user to be User, got {:?}", other),
        }
    }

    #[test]
    fn local_var_decls_are_visited_in_source_order() {
        let source = r#"
            class T {
                void m() {
                    var a = 1;
                    var b = a;
                }
            }
        "#;
        let root = parse_root(source);

        let mut names = Vec::new();
        for node in root.descendants() {
            if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                if let Some(name) = node.children_with_tokens().find_map(|item| {
                    item.into_token()
                        .filter(|t| t.kind() == SyntaxKind::IDENT)
                        .map(|t| t.text().to_string())
                }) {
                    names.push(name);
                }
            }
        }

        assert_eq!(names, vec!["a", "b"]);
    }
}
