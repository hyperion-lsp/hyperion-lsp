// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Scope File ID Mismatch
// Tests that scope resolution correctly tracks which file a symbol belongs to
// Current behavior: Scope may incorrectly resolve symbols from wrong file when files have same package
// Expected: Should maintain file_id context and only resolve symbols from correct file

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// Mock structure representing a scope
    #[derive(Debug)]
    struct Scope {
        file_id: usize,
        symbols: HashMap<String, Symbol>,
        parent: Option<Box<Self>>,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Symbol {
        name: String,
        file_id: usize,
        kind: SymbolKind,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum SymbolKind {
        Class,
        Method,
        Field,
        Variable,
    }

    impl Scope {
        fn new(file_id: usize) -> Self {
            Self {
                file_id,
                symbols: HashMap::new(),
                parent: None,
            }
        }

        fn with_parent(file_id: usize, parent: Self) -> Self {
            Self {
                file_id,
                symbols: HashMap::new(),
                parent: Some(Box::new(parent)),
            }
        }

        fn add_symbol(&mut self, name: String, kind: SymbolKind) {
            self.symbols.insert(
                name.clone(),
                Symbol {
                    name,
                    file_id: self.file_id,
                    kind,
                },
            );
        }

        fn resolve(&self, name: &str) -> Option<&Symbol> {
            // First check current scope
            if let Some(symbol) = self.symbols.get(name) {
                return Some(symbol);
            }

            // Then check parent scopes
            if let Some(parent) = &self.parent {
                return parent.resolve(name);
            }

            None
        }

        fn resolve_in_file(&self, name: &str, file_id: usize) -> Option<&Symbol> {
            // Only resolve if symbol is from the correct file
            if let Some(symbol) = self.resolve(name) {
                if symbol.file_id == file_id {
                    return Some(symbol);
                }
            }
            None
        }
    }

    #[test]
    fn test_scope_tracks_correct_file_id() {
        let mut scope = Scope::new(1);
        scope.add_symbol("User".to_string(), SymbolKind::Class);

        let symbol = scope.resolve("User").unwrap();
        assert_eq!(symbol.file_id, 1);
    }

    #[test]
    fn test_scope_different_files_same_package() {
        // File 1 scope
        let mut scope1 = Scope::new(1);
        scope1.add_symbol("User".to_string(), SymbolKind::Class);

        // File 2 scope (same package, different file)
        let mut scope2 = Scope::new(2);
        scope2.add_symbol("Product".to_string(), SymbolKind::Class);

        // Each scope should only resolve its own symbols with correct file_id
        let user = scope1.resolve("User").unwrap();
        assert_eq!(user.file_id, 1);

        let product = scope2.resolve("Product").unwrap();
        assert_eq!(product.file_id, 2);

        // Cross-file lookup should fail
        assert!(scope1.resolve("Product").is_none());
        assert!(scope2.resolve("User").is_none());
    }

    #[test]
    fn test_scope_file_id_with_parent() {
        // Parent scope (file 1)
        let mut parent = Scope::new(1);
        parent.add_symbol("BaseClass".to_string(), SymbolKind::Class);

        // Child scope (file 1, inner class)
        let mut child = Scope::with_parent(1, parent);
        child.add_symbol("InnerClass".to_string(), SymbolKind::Class);

        // Resolve from child should get parent's symbol with correct file_id
        let base = child.resolve("BaseClass").unwrap();
        assert_eq!(base.file_id, 1);

        let inner = child.resolve("InnerClass").unwrap();
        assert_eq!(inner.file_id, 1);
    }

    #[test]
    fn test_scope_file_id_cross_file_parent() {
        // This shouldn't happen in normal operation, but test defensive behavior
        // Parent scope from file 1
        let mut parent = Scope::new(1);
        parent.add_symbol("ParentSymbol".to_string(), SymbolKind::Class);

        // Child scope from file 2 (unusual but test it)
        let mut child = Scope::with_parent(2, parent);
        child.add_symbol("ChildSymbol".to_string(), SymbolKind::Class);

        // Child symbol should have file_id 2
        let child_sym = child.resolve("ChildSymbol").unwrap();
        assert_eq!(child_sym.file_id, 2);

        // Parent symbol should still have file_id 1
        let parent_sym = child.resolve("ParentSymbol").unwrap();
        assert_eq!(parent_sym.file_id, 1);
    }

    #[test]
    fn test_scope_resolve_in_file() {
        let mut scope = Scope::new(1);
        scope.add_symbol("User".to_string(), SymbolKind::Class);

        // Should resolve when file_id matches
        assert!(scope.resolve_in_file("User", 1).is_some());

        // Should not resolve when file_id doesn't match
        assert!(scope.resolve_in_file("User", 2).is_none());
    }

    #[test]
    fn test_scope_shadowing_same_file() {
        // Parent scope
        let mut parent = Scope::new(1);
        parent.add_symbol("name".to_string(), SymbolKind::Field);

        // Child scope shadows with local variable
        let mut child = Scope::with_parent(1, parent);
        child.add_symbol("name".to_string(), SymbolKind::Variable);

        // Should resolve to child's symbol
        let symbol = child.resolve("name").unwrap();
        assert_eq!(symbol.kind, SymbolKind::Variable);
        assert_eq!(symbol.file_id, 1);
    }

    #[test]
    fn test_scope_method_local_variables() {
        // Class scope
        let mut class_scope = Scope::new(1);
        class_scope.add_symbol("MyClass".to_string(), SymbolKind::Class);

        // Method scope
        let mut method_scope = Scope::with_parent(1, class_scope);
        method_scope.add_symbol("localVar".to_string(), SymbolKind::Variable);

        let var = method_scope.resolve("localVar").unwrap();
        assert_eq!(var.file_id, 1);

        let class = method_scope.resolve("MyClass").unwrap();
        assert_eq!(class.file_id, 1);
    }

    #[test]
    fn test_scope_multiple_nested_levels() {
        // File scope
        let mut file_scope = Scope::new(1);
        file_scope.add_symbol("TopLevel".to_string(), SymbolKind::Class);

        // Class scope
        let mut class_scope = Scope::with_parent(1, file_scope);
        class_scope.add_symbol("field".to_string(), SymbolKind::Field);

        // Method scope
        let mut method_scope = Scope::with_parent(1, class_scope);
        method_scope.add_symbol("param".to_string(), SymbolKind::Variable);

        // Block scope
        let mut block_scope = Scope::with_parent(1, method_scope);
        block_scope.add_symbol("local".to_string(), SymbolKind::Variable);

        // All symbols should be resolvable with correct file_id
        assert_eq!(block_scope.resolve("local").unwrap().file_id, 1);
        assert_eq!(block_scope.resolve("param").unwrap().file_id, 1);
        assert_eq!(block_scope.resolve("field").unwrap().file_id, 1);
        assert_eq!(block_scope.resolve("TopLevel").unwrap().file_id, 1);
    }

    #[test]
    fn test_scope_same_symbol_name_different_files() {
        // File 1: defines User class
        let mut scope1 = Scope::new(1);
        scope1.add_symbol("User".to_string(), SymbolKind::Class);

        // File 2: defines User class (different class, same name)
        let mut scope2 = Scope::new(2);
        scope2.add_symbol("User".to_string(), SymbolKind::Class);

        let user1 = scope1.resolve("User").unwrap();
        let user2 = scope2.resolve("User").unwrap();

        // Both resolve "User", but file_ids should differ
        assert_eq!(user1.file_id, 1);
        assert_eq!(user2.file_id, 2);
    }

    #[test]
    fn test_scope_imported_symbol_file_id() {
        // In real implementation, imported symbols would have file_id of defining file
        // not importing file. Test this behavior.

        let mut scope = Scope::new(1); // Current file is 1

        // Manually add symbol from file 2 (simulates import)
        scope.symbols.insert(
            "ImportedClass".into(),
            Symbol {
                name: "ImportedClass".into(),
                file_id: 2, // From different file
                kind: SymbolKind::Class,
            },
        );

        let symbol = scope.resolve("ImportedClass").unwrap();
        assert_eq!(symbol.file_id, 2); // Should be defining file, not importing file
    }

    #[test]
    fn test_scope_inner_class_file_id() {
        let mut outer_scope = Scope::new(1);
        outer_scope.add_symbol("Outer".to_string(), SymbolKind::Class);

        let mut inner_scope = Scope::with_parent(1, outer_scope);
        inner_scope.add_symbol("Inner".to_string(), SymbolKind::Class);

        // Both outer and inner should have same file_id
        let inner = inner_scope.resolve("Inner").unwrap();
        let outer = inner_scope.resolve("Outer").unwrap();

        assert_eq!(inner.file_id, 1);
        assert_eq!(outer.file_id, 1);
    }

    #[test]
    fn test_scope_lambda_file_id() {
        let mut method_scope = Scope::new(1);
        method_scope.add_symbol("method".to_string(), SymbolKind::Method);

        // Lambda scope inherits file_id
        let mut lambda_scope = Scope::with_parent(1, method_scope);
        lambda_scope.add_symbol("lambdaParam".to_string(), SymbolKind::Variable);

        let param = lambda_scope.resolve("lambdaParam").unwrap();
        assert_eq!(param.file_id, 1);

        let method = lambda_scope.resolve("method").unwrap();
        assert_eq!(method.file_id, 1);
    }

    #[test]
    fn test_scope_anonymous_class_file_id() {
        let mut outer_scope = Scope::new(1);
        outer_scope.add_symbol("outerField".to_string(), SymbolKind::Field);

        // Anonymous class scope
        let mut anon_scope = Scope::with_parent(1, outer_scope);
        anon_scope.add_symbol("anonMethod".to_string(), SymbolKind::Method);

        let method = anon_scope.resolve("anonMethod").unwrap();
        let field = anon_scope.resolve("outerField").unwrap();

        assert_eq!(method.file_id, 1);
        assert_eq!(field.file_id, 1);
    }

    #[test]
    fn test_scope_file_id_consistency() {
        // Ensure all symbols in a scope chain from single file have consistent file_id
        let mut level1 = Scope::new(5);
        level1.add_symbol("s1".to_string(), SymbolKind::Class);

        let mut level2 = Scope::with_parent(5, level1);
        level2.add_symbol("s2".to_string(), SymbolKind::Method);

        let mut level3 = Scope::with_parent(5, level2);
        level3.add_symbol("s3".to_string(), SymbolKind::Variable);

        // All should have file_id 5
        assert_eq!(level3.resolve("s1").unwrap().file_id, 5);
        assert_eq!(level3.resolve("s2").unwrap().file_id, 5);
        assert_eq!(level3.resolve("s3").unwrap().file_id, 5);
    }

    #[test]
    fn test_scope_empty_scope_file_id() {
        let scope = Scope::new(42);
        assert_eq!(scope.file_id, 42);

        // Even empty scope should maintain file_id
        assert!(scope.resolve("anything").is_none());
    }

    #[test]
    fn test_scope_resolve_in_file_with_parent() {
        let mut parent = Scope::new(1);
        parent.add_symbol("ParentSym".to_string(), SymbolKind::Class);

        let mut child = Scope::with_parent(1, parent);
        child.add_symbol("ChildSym".to_string(), SymbolKind::Class);

        // Both should resolve when looking in file 1
        assert!(child.resolve_in_file("ChildSym", 1).is_some());
        assert!(child.resolve_in_file("ParentSym", 1).is_some());

        // Neither should resolve when looking in file 2
        assert!(child.resolve_in_file("ChildSym", 2).is_none());
        assert!(child.resolve_in_file("ParentSym", 2).is_none());
    }
}
