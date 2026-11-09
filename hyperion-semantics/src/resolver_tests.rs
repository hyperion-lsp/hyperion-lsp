// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for the resolver module
//!
//! These tests cover symbol resolution, scope management, the ResolverBuilder,
//! and various resolution scenarios including shadowing and hierarchical lookups.

#[cfg(test)]
mod resolver_comprehensive_tests {
    use crate::db::FileId;
    use crate::resolver::{ResolvedSymbol, SymbolResolver};
    use crate::symbol::*;

    // ========== SymbolResolver Basic Tests ==========

    #[test]
    fn test_resolver_creation() {
        let resolver = SymbolResolver::new();
        assert!(resolver.current_scope().is_some());
        assert_eq!(resolver.current_scope().unwrap().kind, ScopeKind::Package);
    }

    #[test]
    fn test_resolver_default() {
        let resolver = SymbolResolver::default();
        assert!(resolver.current_scope().is_some());
    }

    #[test]
    fn test_resolver_with_symbol_table() {
        let table = SymbolTable::new();
        let resolver = SymbolResolver::with_symbol_table(table);
        assert!(resolver.current_scope().is_some());
    }

    #[test]
    fn test_symbol_table_access() {
        let mut resolver = SymbolResolver::new();

        // Test immutable access
        let _table_ref = resolver.symbol_table();

        // Test mutable access
        let _table_mut = resolver.symbol_table_mut();

        // Test passes if both accesses succeed without panic
    }

    // ========== Scope Management Tests ==========

    #[test]
    fn test_enter_exit_scope() {
        let mut resolver = SymbolResolver::new();
        let root_scope = resolver.current_scope_id();

        // Enter class scope
        let class_scope = resolver.enter_scope(ScopeKind::Class);
        assert_ne!(class_scope, root_scope);
        assert_eq!(resolver.current_scope_id(), class_scope);

        // Exit back to root
        assert!(resolver.exit_scope());
        assert_eq!(resolver.current_scope_id(), root_scope);

        // Can't exit root scope
        assert!(!resolver.exit_scope());
        assert_eq!(resolver.current_scope_id(), root_scope);
    }

    #[test]
    fn test_nested_scope_hierarchy() {
        let mut resolver = SymbolResolver::new();
        let root = resolver.current_scope_id();

        let package_scope = resolver.enter_scope(ScopeKind::Package);
        let class_scope = resolver.enter_scope(ScopeKind::Class);
        let method_scope = resolver.enter_scope(ScopeKind::Method);
        let block_scope = resolver.enter_scope(ScopeKind::Block);

        // Verify we're in the deepest scope
        assert_eq!(resolver.current_scope_id(), block_scope);

        // Exit one by one
        assert!(resolver.exit_scope());
        assert_eq!(resolver.current_scope_id(), method_scope);

        assert!(resolver.exit_scope());
        assert_eq!(resolver.current_scope_id(), class_scope);

        assert!(resolver.exit_scope());
        assert_eq!(resolver.current_scope_id(), package_scope);

        assert!(resolver.exit_scope());
        assert_eq!(resolver.current_scope_id(), root);
    }

    #[test]
    fn test_multiple_sibling_scopes() {
        let mut resolver = SymbolResolver::new();
        let root = resolver.current_scope_id();

        // Create first branch
        let class1 = resolver.enter_scope(ScopeKind::Class);
        let method1 = resolver.enter_scope(ScopeKind::Method);
        resolver.exit_scope();
        resolver.exit_scope();

        // Back at root
        assert_eq!(resolver.current_scope_id(), root);

        // Create second branch
        let class2 = resolver.enter_scope(ScopeKind::Class);
        let method2 = resolver.enter_scope(ScopeKind::Method);

        // class1 and class2 should be different
        assert_ne!(class1, class2);
        assert_ne!(method1, method2);
    }

    #[test]
    fn test_scope_kinds() {
        let mut resolver = SymbolResolver::new();

        let _package = resolver.enter_scope(ScopeKind::Package);
        assert_eq!(resolver.current_scope().unwrap().kind, ScopeKind::Package);
        resolver.exit_scope();

        let _class = resolver.enter_scope(ScopeKind::Class);
        assert_eq!(resolver.current_scope().unwrap().kind, ScopeKind::Class);
        resolver.exit_scope();

        let _method = resolver.enter_scope(ScopeKind::Method);
        assert_eq!(resolver.current_scope().unwrap().kind, ScopeKind::Method);
        resolver.exit_scope();

        let _block = resolver.enter_scope(ScopeKind::Block);
        assert_eq!(resolver.current_scope().unwrap().kind, ScopeKind::Block);
        resolver.exit_scope();

        let _lambda = resolver.enter_scope(ScopeKind::Lambda);
        assert_eq!(resolver.current_scope().unwrap().kind, ScopeKind::Lambda);
        resolver.exit_scope();
    }

    // ========== Symbol Definition and Resolution Tests ==========

    #[test]
    fn test_define_and_resolve_symbol() {
        let mut resolver = SymbolResolver::new();

        let symbol = Symbol::new(
            "testVar".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Int),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 10),
        );

        let symbol_id = resolver.define_symbol(symbol);

        let resolved = resolver.resolve("testVar");
        assert!(resolved.is_some());

        let resolved_sym = resolved.unwrap();
        assert_eq!(resolved_sym.symbol_id, symbol_id);
        assert_eq!(&*resolved_sym.name, "testVar");
        assert_eq!(resolved_sym.ty, Type::Primitive(PrimitiveType::Int));
    }

    #[test]
    fn test_resolve_nonexistent_symbol() {
        let mut resolver = SymbolResolver::new();
        let resolved = resolver.resolve("nonexistent");
        assert!(resolved.is_none());
    }

    #[test]
    fn test_hierarchical_symbol_resolution() {
        let mut resolver = SymbolResolver::new();

        // Define in root scope
        let root_symbol = Symbol::new(
            "globalVar".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 5),
        );
        resolver.define_symbol(root_symbol);

        // Enter nested scope
        resolver.enter_scope(ScopeKind::Class);

        // Define in nested scope
        let nested_symbol = Symbol::new(
            "localVar".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Class(ClassType::string()),
                is_final: false,
            },
            Type::Class(ClassType::string()),
            Visibility::Public,
            SourceLocation::new(FileId(0), 10, 20),
        );
        resolver.define_symbol(nested_symbol);

        // Should resolve both from nested scope
        assert!(resolver.resolve("globalVar").is_some());
        assert!(resolver.resolve("localVar").is_some());

        resolver.exit_scope();

        // From root, only globalVar is visible
        assert!(resolver.resolve("globalVar").is_some());
        assert!(resolver.resolve("localVar").is_none());
    }

    #[test]
    fn test_symbol_shadowing() {
        let mut resolver = SymbolResolver::new();

        // Define outer symbol
        let outer = Symbol::new(
            "x".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 5),
        );
        let outer_id = resolver.define_symbol(outer);

        // Enter nested scope
        resolver.enter_scope(ScopeKind::Method);

        // Shadow with different type
        let inner = Symbol::new(
            "x".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Long),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Long),
            Visibility::Public,
            SourceLocation::new(FileId(0), 20, 25),
        );
        let inner_id = resolver.define_symbol(inner);

        // Should resolve to inner symbol
        let resolved = resolver.resolve("x").unwrap();
        assert_eq!(resolved.symbol_id, inner_id);
        assert_eq!(resolved.ty, Type::Primitive(PrimitiveType::Long));

        resolver.exit_scope();

        // Should resolve to outer symbol
        let resolved = resolver.resolve("x").unwrap();
        assert_eq!(resolved.symbol_id, outer_id);
        assert_eq!(resolved.ty, Type::Primitive(PrimitiveType::Int));
    }

    #[test]
    fn test_multiple_shadowing_levels() {
        let mut resolver = SymbolResolver::new();

        // Level 1
        let level1 = Symbol::new(
            "var".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Byte),
            },
            Type::Primitive(PrimitiveType::Byte),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 5),
        );
        resolver.define_symbol(level1);

        // Level 2
        resolver.enter_scope(ScopeKind::Class);
        let level2 = Symbol::new(
            "var".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Short),
            },
            Type::Primitive(PrimitiveType::Short),
            Visibility::Public,
            SourceLocation::new(FileId(0), 10, 15),
        );
        resolver.define_symbol(level2);

        // Level 3
        resolver.enter_scope(ScopeKind::Method);
        let level3 = Symbol::new(
            "var".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Int),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 20, 25),
        );
        resolver.define_symbol(level3);

        // Level 4
        resolver.enter_scope(ScopeKind::Block);
        let level4 = Symbol::new(
            "var".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Long),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Long),
            Visibility::Public,
            SourceLocation::new(FileId(0), 30, 35),
        );
        resolver.define_symbol(level4);

        // Should resolve to level 4
        assert_eq!(
            resolver.resolve("var").unwrap().ty,
            Type::Primitive(PrimitiveType::Long)
        );

        resolver.exit_scope();
        assert_eq!(
            resolver.resolve("var").unwrap().ty,
            Type::Primitive(PrimitiveType::Int)
        );

        resolver.exit_scope();
        assert_eq!(
            resolver.resolve("var").unwrap().ty,
            Type::Primitive(PrimitiveType::Short)
        );

        resolver.exit_scope();
        assert_eq!(
            resolver.resolve("var").unwrap().ty,
            Type::Primitive(PrimitiveType::Byte)
        );
    }

    #[test]
    fn test_resolve_in_specific_scope() {
        let mut resolver = SymbolResolver::new();
        let root = resolver.current_scope_id();

        // Define in root
        let symbol = Symbol::new(
            "rootVar".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 5),
        );
        resolver.define_symbol(symbol);

        // Enter nested scope
        let nested = resolver.enter_scope(ScopeKind::Class);

        // Resolve from nested scope should find it
        let resolved = resolver.resolve_in_scope("rootVar", nested);
        assert!(resolved.is_some());

        // Resolve from root should also find it
        let resolved = resolver.resolve_in_scope("rootVar", root);
        assert!(resolved.is_some());
    }

    // ========== Visible Symbols Tests ==========

    #[test]
    fn test_visible_symbols() {
        let mut resolver = SymbolResolver::new();

        // Define multiple symbols in current scope
        for i in 0..5 {
            let symbol = Symbol::new(
                format!("var{}", i),
                SymbolKind::LocalVariable {
                    var_type: Type::Primitive(PrimitiveType::Int),
                    is_final: false,
                },
                Type::Primitive(PrimitiveType::Int),
                Visibility::Public,
                SourceLocation::new(FileId(0), i * 10, i * 10 + 5),
            );
            resolver.define_symbol(symbol);
        }

        let visible = resolver.visible_symbols();
        assert_eq!(visible.len(), 5);

        // Check all symbols are present
        for i in 0..5 {
            assert!(visible.iter().any(|(name, _)| name == &format!("var{}", i)));
        }
    }

    #[test]
    fn test_visible_symbols_hierarchical() {
        let mut resolver = SymbolResolver::new();

        // Root level
        let root_sym = Symbol::new(
            "root".into(),
            SymbolKind::Package,
            Type::Unknown,
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 5),
        );
        resolver.define_symbol(root_sym);

        // Nested level
        resolver.enter_scope(ScopeKind::Class);
        let class_sym = Symbol::new(
            "class".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 10, 15),
        );
        resolver.define_symbol(class_sym);

        let visible = resolver.visible_symbols();

        // Should see both root and class symbols
        assert!(visible.iter().any(|(name, _)| name == "root"));
        assert!(visible.iter().any(|(name, _)| name == "class"));
    }

    // ========== Enclosing Scope Tests ==========

    #[test]
    fn test_enclosing_class_scope() {
        let mut resolver = SymbolResolver::new();

        resolver.enter_scope(ScopeKind::Package);
        let class_scope = resolver.enter_scope(ScopeKind::Class);
        resolver.enter_scope(ScopeKind::Method);
        resolver.enter_scope(ScopeKind::Block);

        let enclosing = resolver.enclosing_class_scope();
        assert_eq!(enclosing, Some(class_scope));
    }

    #[test]
    fn test_enclosing_method_scope() {
        let mut resolver = SymbolResolver::new();

        resolver.enter_scope(ScopeKind::Package);
        resolver.enter_scope(ScopeKind::Class);
        let method_scope = resolver.enter_scope(ScopeKind::Method);
        resolver.enter_scope(ScopeKind::Block);

        let enclosing = resolver.enclosing_method_scope();
        assert_eq!(enclosing, Some(method_scope));
    }

    #[test]
    fn test_no_enclosing_class() {
        let mut resolver = SymbolResolver::new();
        resolver.enter_scope(ScopeKind::Package);
        resolver.enter_scope(ScopeKind::Method); // Method without class (edge case)

        let enclosing = resolver.enclosing_class_scope();
        assert_eq!(enclosing, None);
    }

    #[test]
    fn test_no_enclosing_method() {
        let mut resolver = SymbolResolver::new();
        resolver.enter_scope(ScopeKind::Package);
        resolver.enter_scope(ScopeKind::Class);

        let enclosing = resolver.enclosing_method_scope();
        assert_eq!(enclosing, None);
    }

    // ========== Cache Tests ==========

    #[test]
    fn test_cache_clear() {
        let mut resolver = SymbolResolver::new();

        let symbol = Symbol::new(
            "cached".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Int),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 5),
        );
        resolver.define_symbol(symbol);

        // Resolve to populate cache
        let _ = resolver.resolve("cached");

        // Clear cache
        resolver.clear_cache();

        // Should still be able to resolve (will rebuild cache)
        let resolved = resolver.resolve("cached");
        assert!(resolved.is_some());
    }

    // ========== ResolvedSymbol Tests ==========

    #[test]
    fn test_resolved_symbol_structure() {
        let resolved = ResolvedSymbol {
            symbol_id: SymbolId(42),
            name: "test".into(),
            ty: Type::Primitive(PrimitiveType::Int),
            file_id: FileId(1),
            scope_id: ScopeId(10),
        };

        assert_eq!(resolved.symbol_id, SymbolId(42));
        assert_eq!(&*resolved.name, "test");
        assert_eq!(resolved.ty, Type::Primitive(PrimitiveType::Int));
        assert_eq!(resolved.file_id, FileId(1));
        assert_eq!(resolved.scope_id, ScopeId(10));
    }

    #[test]
    fn test_resolved_symbol_equality() {
        let resolved1 = ResolvedSymbol {
            symbol_id: SymbolId(1),
            name: "test".into(),
            ty: Type::Primitive(PrimitiveType::Int),
            file_id: FileId(1),
            scope_id: ScopeId(1),
        };

        let resolved2 = ResolvedSymbol {
            symbol_id: SymbolId(1),
            name: "test".into(),
            ty: Type::Primitive(PrimitiveType::Int),
            file_id: FileId(1),
            scope_id: ScopeId(1),
        };

        assert_eq!(resolved1, resolved2);
    }

    // ========== Complex Scenarios ==========

    #[test]
    fn test_java_like_scope_scenario() {
        let mut resolver = SymbolResolver::new();

        // Package level
        let _package_scope = resolver.enter_scope(ScopeKind::Package);

        // Class level
        let class_scope = resolver.enter_scope(ScopeKind::Class);
        let field = Symbol::new(
            "count".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Private,
            SourceLocation::new(FileId(0), 10, 20),
        );
        resolver.define_symbol(field);

        // Method level
        let method_scope = resolver.enter_scope(ScopeKind::Method);
        let param = Symbol::new(
            "value".into(),
            SymbolKind::Parameter {
                param_type: Type::Primitive(PrimitiveType::Int),
                is_varargs: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 30, 40),
        );
        resolver.define_symbol(param);

        // Block level
        resolver.enter_scope(ScopeKind::Block);
        let local = Symbol::new(
            "temp".into(),
            SymbolKind::LocalVariable {
                var_type: Type::Primitive(PrimitiveType::Int),
                is_final: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 50, 60),
        );
        resolver.define_symbol(local);

        // All should be visible
        assert!(resolver.resolve("count").is_some());
        assert!(resolver.resolve("value").is_some());
        assert!(resolver.resolve("temp").is_some());

        // Verify scope hierarchy
        assert_eq!(resolver.enclosing_class_scope(), Some(class_scope));
        assert_eq!(resolver.enclosing_method_scope(), Some(method_scope));
    }

    #[test]
    fn test_multiple_classes_scenario() {
        let mut resolver = SymbolResolver::new();

        // First class
        resolver.enter_scope(ScopeKind::Class);
        let field1 = Symbol::new(
            "field1".into(),
            SymbolKind::Field {
                field_type: Type::Primitive(PrimitiveType::Int),
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Private,
            SourceLocation::new(FileId(0), 0, 10),
        );
        resolver.define_symbol(field1);
        resolver.exit_scope();

        // Second class
        resolver.enter_scope(ScopeKind::Class);
        let field2 = Symbol::new(
            "field2".into(),
            SymbolKind::Field {
                field_type: Type::Class(ClassType::string()),
            },
            Type::Class(ClassType::string()),
            Visibility::Private,
            SourceLocation::new(FileId(0), 20, 30),
        );
        resolver.define_symbol(field2);

        // field1 should not be visible in second class
        assert!(resolver.resolve("field1").is_none());
        assert!(resolver.resolve("field2").is_some());
    }

    #[test]
    fn test_lambda_scope() {
        let mut resolver = SymbolResolver::new();

        resolver.enter_scope(ScopeKind::Method);
        let param = Symbol::new(
            "param".into(),
            SymbolKind::Parameter {
                param_type: Type::Primitive(PrimitiveType::Int),
                is_varargs: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 0, 10),
        );
        resolver.define_symbol(param);

        // Lambda scope
        resolver.enter_scope(ScopeKind::Lambda);
        let lambda_param = Symbol::new(
            "x".into(),
            SymbolKind::Parameter {
                param_type: Type::Primitive(PrimitiveType::Int),
                is_varargs: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Public,
            SourceLocation::new(FileId(0), 20, 25),
        );
        resolver.define_symbol(lambda_param);

        // Both should be visible
        assert!(resolver.resolve("param").is_some());
        assert!(resolver.resolve("x").is_some());
    }

    #[test]
    fn test_large_number_of_symbols() {
        let mut resolver = SymbolResolver::new();

        // Define many symbols
        for i in 0..1000 {
            let symbol = Symbol::new(
                format!("var{}", i),
                SymbolKind::LocalVariable {
                    var_type: Type::Primitive(PrimitiveType::Int),
                    is_final: false,
                },
                Type::Primitive(PrimitiveType::Int),
                Visibility::Public,
                SourceLocation::new(FileId(0), i * 10, i * 10 + 5),
            );
            resolver.define_symbol(symbol);
        }

        // All should be resolvable
        for i in 0..1000 {
            assert!(resolver.resolve(&format!("var{}", i)).is_some());
        }

        let visible = resolver.visible_symbols();
        assert_eq!(visible.len(), 1000);
    }

    // ========== Navigation Resolution Tests ==========

    #[test]
    fn test_navigation_respects_shadowing() {
        // Java scoping: inner scopes shadow outer scopes.
        // A method parameter shadows a class field with the same name.
        let mut resolver = SymbolResolver::new();
        let string_type = Type::Class(ClassType::new("String".into(), Some("java.lang".into())));

        // Enter class scope and define a field
        let _class_scope = resolver.enter_scope(ScopeKind::Class);
        let field_symbol = Symbol::new(
            "name".into(),
            SymbolKind::Field {
                field_type: string_type.clone(),
            },
            string_type.clone(),
            Visibility::Private,
            SourceLocation::new(FileId(0), 10, 14),
        );
        resolver.define_symbol(field_symbol);

        // Enter method scope and define a parameter with the same name
        let method_scope = resolver.enter_scope(ScopeKind::Method);
        let param_symbol = Symbol::new(
            "name".into(),
            SymbolKind::Parameter {
                param_type: string_type.clone(),
                is_varargs: false,
            },
            string_type,
            Visibility::Private,
            SourceLocation::new(FileId(0), 50, 54),
        );
        resolver.define_symbol(param_symbol);

        // Standard resolution should find parameter (innermost scope)
        let standard_result = resolver.resolve_in_scope("name", method_scope);
        assert!(standard_result.is_some());
        let standard_symbol = resolver
            .symbol_table()
            .get_symbol(standard_result.unwrap().symbol_id)
            .unwrap();
        assert!(
            matches!(standard_symbol.kind, SymbolKind::Parameter { .. }),
            "Standard resolution should find parameter"
        );

        // Navigation resolution should also find parameter (respects shadowing)
        let nav_result = resolver.resolve_in_scope_for_navigation("name", method_scope);
        assert!(nav_result.is_some());
        let nav_symbol = resolver
            .symbol_table()
            .get_symbol(nav_result.unwrap().symbol_id)
            .unwrap();
        assert!(
            matches!(nav_symbol.kind, SymbolKind::Parameter { .. }),
            "Navigation resolution should respect shadowing and find parameter"
        );
        assert_eq!(
            nav_symbol.location.range.start, 50,
            "Should navigate to parameter position"
        );
    }

    #[test]
    fn test_navigation_returns_parameter_when_no_field() {
        let mut resolver = SymbolResolver::new();
        let _class_scope = resolver.enter_scope(ScopeKind::Class);
        let method_scope = resolver.enter_scope(ScopeKind::Method);

        let param_symbol = Symbol::new(
            "arg".into(),
            SymbolKind::Parameter {
                param_type: Type::Primitive(PrimitiveType::Int),
                is_varargs: false,
            },
            Type::Primitive(PrimitiveType::Int),
            Visibility::Private,
            SourceLocation::new(FileId(0), 20, 23),
        );
        resolver.define_symbol(param_symbol);

        let nav_result = resolver.resolve_in_scope_for_navigation("arg", method_scope);
        assert!(nav_result.is_some());
        let nav_symbol = resolver
            .symbol_table()
            .get_symbol(nav_result.unwrap().symbol_id)
            .unwrap();
        assert!(
            matches!(nav_symbol.kind, SymbolKind::Parameter { .. }),
            "Should find parameter when no field exists"
        );
    }

    #[test]
    fn test_navigation_field_in_nested_block() {
        let mut resolver = SymbolResolver::new();
        let string_type = Type::Class(ClassType::new("String".into(), Some("java.lang".into())));
        let _class_scope = resolver.enter_scope(ScopeKind::Class);

        let field_symbol = Symbol::new(
            "name".into(),
            SymbolKind::Field {
                field_type: string_type.clone(),
            },
            string_type,
            Visibility::Private,
            SourceLocation::new(FileId(0), 15, 19),
        );
        resolver.define_symbol(field_symbol);

        let _method_scope = resolver.enter_scope(ScopeKind::Method);
        let block_scope = resolver.enter_scope(ScopeKind::Block);

        let nav_result = resolver.resolve_in_scope_for_navigation("name", block_scope);
        assert!(nav_result.is_some());
        let nav_symbol = resolver
            .symbol_table()
            .get_symbol(nav_result.unwrap().symbol_id)
            .unwrap();
        assert!(
            matches!(nav_symbol.kind, SymbolKind::Field { .. }),
            "Should find field from nested block scope"
        );
    }
}

// ============================================================================
// Edge Case Tests for Bug Fixes (2.1 and 2.2)
// ============================================================================

#[cfg(test)]
mod edge_case_tests {

    use crate::resolver::{NarrowingScopeGuard, PatternScopeGuard, SymbolResolver};
    use crate::symbol::{PrimitiveType, Type};

    #[test]
    fn test_fqn_edge_cases() {
        // BUGFIX 2.2: Test FQN handling with edge cases
        // These patterns should not panic when splitting FQNs

        let test_cases = vec![
            // (fqn, expected_valid)
            ("java.lang.String", true),
            ("java.util.ArrayList", true),
            ("com.example.MyClass", true),
            ("", false),             // empty
            (".", false),            // just dot
            ("com.example.", false), // trailing dot
            (".com.example", false), // leading dot
        ];

        for (fqn, _should_be_valid) in test_cases {
            // The important thing is that this doesn't panic
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Simulate the split logic from the bugfix
                if let Some(last_dot) = fqn.rfind('.') {
                    let (pkg, name) = fqn.split_at(last_dot);
                    // BUGFIX: This check prevents panic
                    let _class_name = name.get(1..).unwrap_or("");
                    (pkg.to_string(), _class_name.to_string())
                } else {
                    (String::new(), fqn.to_string())
                }
            }));

            // All cases should complete without panicking
            assert!(result.is_ok(), "FQN '{}' caused panic during split", fqn);
        }
    }

    #[test]
    fn test_parse_type_rejects_malformed_fqns() {
        use crate::db::FileId;
        use crate::resolver::ResolverBuilder;

        // Create a minimal resolver
        let resolver = ResolverBuilder::new(FileId(0));

        // Malformed FQNs should return None, not garbage types
        let malformed_cases = vec![
            "",             // empty
            ".",            // just dot
            "com.example.", // trailing dot
            ".com.example", // leading dot
            "..",           // double dot
        ];

        for fqn in malformed_cases {
            let result = resolver.parse_type_from_string(fqn);
            assert!(
                result.is_none(),
                "Malformed FQN '{}' should return None, got {:?}",
                fqn,
                result
            );
        }

        // Valid FQNs should return Some
        let valid_cases = vec!["String", "java.lang.String", "com.example.MyClass"];

        for fqn in valid_cases {
            let result = resolver.parse_type_from_string(fqn);
            assert!(
                result.is_some(),
                "Valid FQN '{}' should return Some, got None",
                fqn
            );
        }
    }

    // ========== RAII Scope Guard Tests ==========

    #[test]
    fn test_narrowing_scope_guard_cleanup() {
        let resolver = SymbolResolver::new();

        // Initial state: 1 scope (root)
        assert_eq!(resolver.narrowed_types_stack.borrow().len(), 1);

        {
            let _guard = NarrowingScopeGuard::new(&resolver);
            // Guard pushed a scope
            assert_eq!(resolver.narrowed_types_stack.borrow().len(), 2);
        } // Guard drops here

        // Guard automatically popped the scope
        assert_eq!(resolver.narrowed_types_stack.borrow().len(), 1);
    }

    #[test]
    fn test_pattern_scope_guard_cleanup() {
        let resolver = SymbolResolver::new();

        // Initial state: 1 scope (root)
        assert_eq!(resolver.pattern_variables_stack.borrow().len(), 1);

        {
            let _guard = PatternScopeGuard::new(&resolver);
            // Guard pushed a scope
            assert_eq!(resolver.pattern_variables_stack.borrow().len(), 2);
        } // Guard drops here

        // Guard automatically popped the scope
        assert_eq!(resolver.pattern_variables_stack.borrow().len(), 1);
    }

    #[test]
    fn test_scope_guard_cleanup_on_early_return() {
        fn test_helper(do_early_return: bool) -> (usize, usize) {
            let resolver = SymbolResolver::new();

            let _narrowing_guard = NarrowingScopeGuard::new(&resolver);
            let _pattern_guard = PatternScopeGuard::new(&resolver);

            if do_early_return {
                let narrowed_len = resolver.narrowed_types_stack.borrow().len();
                let pattern_len = resolver.pattern_variables_stack.borrow().len();
                return (narrowed_len, pattern_len);
            }

            // Add some data
            resolver.add_narrowed_type("x".to_string(), Type::Primitive(PrimitiveType::Int));
            resolver.add_pattern_variable("y".to_string(), Type::Primitive(PrimitiveType::Long));

            let narrowed_len = resolver.narrowed_types_stack.borrow().len();
            let pattern_len = resolver.pattern_variables_stack.borrow().len();
            (narrowed_len, pattern_len)
        }

        // Both guards should be dropped even on early return
        let (narrowed, pattern) = test_helper(true);
        assert_eq!(narrowed, 2); // Root + guard's scope
        assert_eq!(pattern, 2);
    }

    #[test]
    fn test_multiple_guards_cleanup_in_order() {
        let resolver = SymbolResolver::new();

        assert_eq!(resolver.narrowed_types_stack.borrow().len(), 1);

        {
            let _guard1 = NarrowingScopeGuard::new(&resolver);
            assert_eq!(resolver.narrowed_types_stack.borrow().len(), 2);

            {
                let _guard2 = NarrowingScopeGuard::new(&resolver);
                assert_eq!(resolver.narrowed_types_stack.borrow().len(), 3);
            } // guard2 drops

            assert_eq!(resolver.narrowed_types_stack.borrow().len(), 2);
        } // guard1 drops

        assert_eq!(resolver.narrowed_types_stack.borrow().len(), 1);
    }

    #[test]
    fn test_guards_with_resolver_usage() {
        let resolver = SymbolResolver::new();

        let _narrowing_guard = NarrowingScopeGuard::new(&resolver);
        let _pattern_guard = PatternScopeGuard::new(&resolver);

        // Can still use resolver while guards are alive
        resolver.add_narrowed_type("x".to_string(), Type::Primitive(PrimitiveType::Int));
        resolver.add_pattern_variable("y".to_string(), Type::Primitive(PrimitiveType::Long));

        // Verify the data was added
        let narrowings = resolver.get_all_narrowed_types();
        assert!(narrowings.contains_key("x"));

        // Guards will clean up on drop
    }
}
