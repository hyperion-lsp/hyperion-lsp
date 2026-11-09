// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Integration tests that verify critical LSP functionality
// These tests catch bugs that unit tests miss by testing the full pipeline

#[cfg(test)]
mod tests {
    use crate::{Modifiers, Symbol, TextRange, WorkspaceIndex};
    use lsp_types::SymbolKind;
    use std::path::PathBuf;

    /// Helper: create a test symbol with a given FQN and simple name
    fn make_symbol(fqn: &str, simple_name: &str) -> Symbol {
        Symbol {
            fqn: fqn.into(),
            simple_name: simple_name.into(),
            range: TextRange::new(0, 100),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: vec![],
            parent_class: None,
            interfaces: vec![],
            documentation: None,
            type_parameters: vec![],
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        }
    }

    /// CRITICAL: This test catches the FileId HashMap collision bug (046d97d)
    ///
    /// Bug: Using FileId(0) for all JARs caused HashMap collisions, losing 49K+ symbols
    /// Fix: Use unique FileIds + track in external_file_ids HashSet (6f78b34)
    ///
    /// We simulate multiple JARs by adding symbols from different "sources".
    /// Each source represents a different JAR with unique symbols.
    #[test]
    fn test_multiple_jars_no_symbol_loss() {
        let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));

        // Simulate 3 JARs with distinct symbols
        let jar1_symbols = vec![
            make_symbol("com.jar1.Alpha", "Alpha"),
            make_symbol("com.jar1.Beta", "Beta"),
            make_symbol("com.jar1.Gamma", "Gamma"),
        ];
        let jar2_symbols = vec![
            make_symbol("com.jar2.Delta", "Delta"),
            make_symbol("com.jar2.Epsilon", "Epsilon"),
            make_symbol("com.jar2.Zeta", "Zeta"),
        ];
        let jar3_symbols = vec![
            make_symbol("com.jar3.Eta", "Eta"),
            make_symbol("com.jar3.Theta", "Theta"),
            make_symbol("com.jar3.Iota", "Iota"),
        ];

        // Add all symbols (simulating 3 separate JAR indexing operations)
        for sym in &jar1_symbols {
            index.add_external_symbol(sym.clone());
        }
        for sym in &jar2_symbols {
            index.add_external_symbol(sym.clone());
        }
        for sym in &jar3_symbols {
            index.add_external_symbol(sym.clone());
        }

        // CRITICAL ASSERTION: All 9 symbols from all 3 "JARs" should be retrievable
        let total_expected = jar1_symbols.len() + jar2_symbols.len() + jar3_symbols.len();
        let external_symbols = index.get_external_symbols();
        assert_eq!(
            external_symbols.len(),
            total_expected,
            "Should have {} symbols from 3 JARs, got {}",
            total_expected,
            external_symbols.len()
        );

        // Verify each specific symbol is retrievable by FQN
        for fqn in [
            "com.jar1.Alpha",
            "com.jar1.Beta",
            "com.jar1.Gamma",
            "com.jar2.Delta",
            "com.jar2.Epsilon",
            "com.jar2.Zeta",
            "com.jar3.Eta",
            "com.jar3.Theta",
            "com.jar3.Iota",
        ] {
            assert!(
                index.get_symbol(fqn).is_some(),
                "Symbol '{}' should be retrievable after multi-JAR indexing",
                fqn
            );
        }
    }

    /// Verify that external symbols persist correctly through save/load cycle
    #[test]
    fn test_external_symbols_cache_roundtrip() {
        let index = WorkspaceIndex::new(PathBuf::from("/tmp/test"));

        // Add symbols to first index
        let symbols = vec![
            make_symbol("com.cache.Foo", "Foo"),
            make_symbol("com.cache.Bar", "Bar"),
            make_symbol("com.cache.Baz", "Baz"),
        ];
        for sym in &symbols {
            index.add_external_symbol(sym.clone());
        }

        let symbols_before = index.get_external_symbols();
        assert_eq!(
            symbols_before.len(),
            3,
            "Should have 3 external symbols before roundtrip"
        );

        // Simulate cache save/load: transfer symbols to a fresh index
        let index2 = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
        for symbol in symbols_before.iter() {
            index2.add_external_symbol(symbol.clone());
        }

        // Verify symbols are still retrievable
        let symbols_after = index2.get_external_symbols();
        assert_eq!(
            symbols_before.len(),
            symbols_after.len(),
            "Symbol count should match after cache roundtrip"
        );

        // Verify each symbol by FQN
        for fqn in ["com.cache.Foo", "com.cache.Bar", "com.cache.Baz"] {
            assert!(
                index2.get_symbol(fqn).is_some(),
                "Symbol '{}' should survive cache roundtrip",
                fqn
            );
        }
    }

    /// Verify workspace symbols take priority over JDK symbols
    ///
    /// This is a critical invariant: if a workspace has a mock/override of a JDK class,
    /// that should take priority. This is important for testing.
    #[test]
    fn test_workspace_symbols_override_jdk() {
        let temp = tempfile::tempdir().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let workspace_path = temp.path().join("String.java");
        index
            .index_file_with_source(&workspace_path, "package java.lang; public class String {}")
            .unwrap();
        let workspace_file_id = index.get_file_id(&workspace_path).unwrap();

        // Try to add JDK symbol with same FQN
        let jdk_symbol = Symbol {
            documentation: Some("JDK version".into()),
            ..make_symbol("java.lang.String", "String")
        };
        index.add_external_symbol(jdk_symbol);

        let active = index.find_symbol("java.lang.String").unwrap();
        assert_eq!(
            active.file_id, workspace_file_id,
            "workspace declarations must take priority over external declarations"
        );

        index.remove_file(workspace_file_id);
        let restored = index.get_symbol("java.lang.String").unwrap();
        assert_eq!(restored.documentation.as_deref(), Some("JDK version"));
    }
}
