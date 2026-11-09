// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Usage extractor: finds symbol references in Java source code
//
// Extracts:
// - Qualified field access (e.g., ResourceType.UNKNOWN, System.out)
// - Method calls (e.g., user.getName())
// - Simple identifier references (variable/field usage)
//
// Each usage is resolved to an FQN using import information.

mod context;
mod field;
mod identifier;
pub mod import_ref;
mod method;
pub mod signature_inference;
mod type_ref;
mod type_spelling;

use crate::{
    import_resolver::{ImportResolver, RevisionSymbolOverlay},
    index::IndexInner,
    FileId, ImportMap, SymbolLocation, WorkspaceIndex,
};
use anyhow::Result;
use hyperion_parser::{
    lexer::Lexer,
    parser::Parser,
    syntax::{SyntaxKind, SyntaxNode},
};
use tracing::debug;

// Re-export for public API
pub use context::ExtractionContext;

/// Represents a single usage/reference to a symbol
#[derive(Debug, Clone)]
pub struct Usage {
    /// Fully qualified name of the referenced symbol
    pub fqn: String,
    /// Location of the usage
    pub location: SymbolLocation,
}

pub struct UsageExtractor;

impl UsageExtractor {
    /// Extract all symbol usages from Java source code
    ///
    /// # Arguments
    /// * `source` - Java source code
    /// * `file_id` - FileId for the source file
    /// * `imports` - Import map for this file (includes package info)
    /// * `workspace_index` - Workspace index for resolving types
    ///
    /// # Returns
    /// Vector of usages found in the source code
    pub fn extract(
        source: &str,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
    ) -> Result<Vec<Usage>> {
        // Parse the source
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        Self::extract_from_syntax(&root, file_id, imports, workspace_index)
    }

    /// Extract usages from an already parsed full Java syntax tree.
    ///
    /// Indexing retains this same parse product for incremental updates, so
    /// accepting it here keeps usage extraction and the parse cache on one
    /// canonical view of the file revision.
    pub fn extract_from_syntax(
        root: &SyntaxNode,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
    ) -> Result<Vec<Usage>> {
        let resolver =
            ImportResolver::for_language(imports, workspace_index, crate::Language::Java);
        Ok(Self::extract_with_resolver(
            root, file_id, imports, &resolver,
        ))
    }

    pub(crate) fn extract_from_syntax_for_revision(
        root: &SyntaxNode,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
        revision: &RevisionSymbolOverlay,
    ) -> Vec<Usage> {
        let resolver =
            ImportResolver::for_revision(imports, workspace_index, revision, crate::Language::Java);
        Self::extract_with_resolver(root, file_id, imports, &resolver)
    }

    pub(crate) fn extract_from_syntax_for_coherent_index(
        root: &SyntaxNode,
        file_id: FileId,
        imports: &ImportMap,
        inner: &IndexInner,
        revision: &RevisionSymbolOverlay,
    ) -> Vec<Usage> {
        let resolver =
            ImportResolver::for_coherent_index(imports, inner, revision, crate::Language::Java);
        Self::extract_with_resolver(root, file_id, imports, &resolver)
    }

    /// Type usages spelled in `import` declarations and Javadoc reference
    /// tags. Only refactorings that must rewrite every spelling of a type
    /// request these; they are deliberately absent from the usage cache.
    pub(crate) fn extract_import_and_doc_type_usages(
        root: &SyntaxNode,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
    ) -> Vec<Usage> {
        let resolver =
            ImportResolver::for_language(imports, workspace_index, crate::Language::Java);
        import_ref::extract_import_and_doc_type_usages(root, file_id, &resolver)
    }

    pub(crate) fn extract_kotlin_import_type_usages(
        root: &hyperion_kotlin::SyntaxNode,
        file_id: FileId,
        imports: &ImportMap,
        workspace_index: &WorkspaceIndex,
    ) -> Vec<Usage> {
        let resolver =
            ImportResolver::for_language(imports, workspace_index, crate::Language::Kotlin);
        import_ref::extract_kotlin_import_type_usages(root, file_id, &resolver)
    }

    fn extract_with_resolver(
        root: &SyntaxNode,
        file_id: FileId,
        imports: &ImportMap,
        resolver: &ImportResolver<'_>,
    ) -> Vec<Usage> {
        let mut usages = Vec::new();
        let ctx = ExtractionContext::for_root(root);

        // Walk the AST and find usages
        Self::extract_from_node(root, file_id, resolver, imports, &mut usages, &ctx);

        debug!("Extracted {} usages from file {:?}", usages.len(), file_id);

        usages
    }

    /// Recursively extract usages from an AST node
    fn extract_from_node(
        node: &SyntaxNode,
        file_id: FileId,
        resolver: &ImportResolver,
        imports: &ImportMap,
        usages: &mut Vec<Usage>,
        ctx: &ExtractionContext,
    ) {
        // First, handle this node based on its type
        match node.kind() {
            // Handle qualified field access: ResourceType.UNKNOWN, Math.PI, System.out
            SyntaxKind::FIELD_ACCESS_EXPR => {
                debug!("Found FIELD_ACCESS_EXPR at {:?}", node.text_range());
                field::extract_field_access(node, file_id, resolver, imports, usages, ctx);
                // Don't recurse into children - we've handled the entire expression
                return;
            }

            // Handle NAME_EXPR that might be followed by DOT+IDENT (qualified access without FIELD_ACCESS_EXPR marker)
            // Example: Inside ARG_LIST for method arguments like service.process(ResourceType.UNKNOWN)
            SyntaxKind::NAME_EXPR => {
                if let Some(method_ref) = node.descendants().find(|child| {
                    matches!(
                        child.kind(),
                        SyntaxKind::METHOD_REF_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR
                    )
                }) {
                    method::extract_method_reference(&method_ref, file_id, resolver, usages, ctx);
                    return;
                }
                let handled =
                    field::extract_qualified_name_expr(node, file_id, resolver, usages, ctx);
                if !handled {
                    identifier::extract_identifier_usage(node, file_id, resolver, ctx, usages);
                }
                // Don't recurse - we've handled this
                return;
            }

            // Handle method calls: obj.method(), Class.staticMethod()
            SyntaxKind::METHOD_CALL_EXPR => {
                method::extract_method_call(node, file_id, resolver, usages, ctx);
                // DO recurse into children - arguments may contain qualified access expressions
                // For example: service.process(ResourceType.USER) has ResourceType.USER inside ARG_LIST
            }

            // Handle constructor calls: new User(), new ArrayList<String>()
            SyntaxKind::NEW_EXPR => {
                method::extract_constructor_call(node, file_id, resolver, usages);
                // Recurse into children for nested expressions in constructor args
            }

            // Handle method references: Type::method, obj::method, Type::new
            SyntaxKind::METHOD_REF_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR => {
                method::extract_method_reference(node, file_id, resolver, usages, ctx);
            }

            // Handle type references in declarations
            SyntaxKind::CLASS_TYPE => {
                type_ref::extract_type_reference(
                    node,
                    file_id,
                    resolver,
                    ctx.enclosing_class_fqn.as_deref(),
                    usages,
                );
                // Continue recursing into TYPE_ARGS for nested generic types like List<User>
            }

            // Handle class/interface inheritance: extends Foo, implements Bar
            SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL => {
                // Extract class name and field types, update context for children
                let class_ctx = context::build_class_context(node, imports, resolver, ctx);
                let body_start = node.children_with_tokens().find_map(|child| {
                    child
                        .into_token()
                        .filter(|token| token.kind() == SyntaxKind::L_BRACE)
                        .map(|token| token.text_range().start())
                });
                for child in node.children() {
                    // A declaration header is in the enclosing lexical type's
                    // scope. The type being declared must not expose its body
                    // member types to its own extends/implements spellings.
                    let child_ctx = if body_start
                        .is_some_and(|body_start| child.text_range().end() <= body_start)
                    {
                        ctx
                    } else {
                        &class_ctx
                    };
                    Self::extract_from_node(&child, file_id, resolver, imports, usages, child_ctx);
                }
                return; // Already recursed with updated context
            }

            // Handle method declarations - extract parameters and local variables
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => {
                // Build method context with parameters
                let method_ctx = context::build_method_context(node, ctx, resolver);
                for child in node.children() {
                    Self::extract_from_node(
                        &child,
                        file_id,
                        resolver,
                        imports,
                        usages,
                        &method_ctx,
                    );
                }
                return; // Already recursed with method context
            }

            // Handle local variable declarations - add to context for subsequent siblings
            SyntaxKind::LOCAL_VAR_DECL => {
                // Extract type usages first
                context::extract_local_var_types(node, file_id, resolver, usages, ctx);
                // Note: We don't update context here because local vars are method-scoped
                // and the METHOD_DECL handler already extracts them
            }

            _ => {}
        }

        // Recurse into children (only if we didn't handle this node above)
        for child in node.children() {
            Self::extract_from_node(&child, file_id, resolver, imports, usages, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};
    use lsp_types::SymbolKind;
    use std::path::PathBuf;

    fn make_test_imports() -> ImportMap {
        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import(
            "ResourceType".to_string(),
            "com.example.ResourceType".to_string(),
        );
        imports
    }

    fn make_test_workspace() -> WorkspaceIndex {
        let workspace = WorkspaceIndex::new(PathBuf::from("/tmp/test"));
        workspace
            .index_file_with_source(
                PathBuf::from("/tmp/test/com/example/ResourceType.java").as_path(),
                r#"package com.example;
public class ResourceType {
    public static ResourceType UNKNOWN;
    public static ResourceType USER;
}"#,
            )
            .expect("shared ResourceType fixture should index");
        workspace
            .index_file_with_source(
                PathBuf::from("/tmp/test/java/lang/String.java").as_path(),
                r#"package java.lang;
public class String {
    public String toUpperCase() { return this; }
}"#,
            )
            .expect("shared String fixture should index");
        workspace
    }

    #[test]
    fn switch_label_usage_uses_the_nested_selector_type_identity() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace
            .index_file_with_source(
                &temp.path().join("q/Event.java"),
                "package q; public enum Event { READY }",
            )
            .expect("import distractor should index");
        workspace
            .index_file_with_source(
                &temp.path().join("p/Event.java"),
                "package p; public enum Event { READY }",
            )
            .expect("same-package distractor should index");
        let source = r#"
package p;
import q.Event;
class Processor {
    enum Event { READY }
    void process(Event event) {
        switch (event) {
            case READY: break;
        }
    }
}
"#;
        let path = temp.path().join("p/Processor.java");
        workspace
            .index_file_with_source(&path, source)
            .expect("processor should index");
        let file_id = workspace.get_file_id(&path).expect("processor file id");
        let imports = workspace.get_imports(file_id).expect("processor imports");

        let usages = UsageExtractor::extract(source, file_id, &imports, &workspace)
            .expect("usages should extract");
        let label_start = u32::try_from(source.find("case READY").expect("switch label") + 5)
            .expect("label offset fits u32");
        let usage = usages
            .iter()
            .find(|usage| usage.location.range.start == label_start)
            .expect("switch label usage");

        assert_eq!(usage.fqn, "p.Processor.Event.READY");
        assert_eq!(usage.location.kind, SymbolKind::ENUM_MEMBER);
    }

    #[test]
    fn type_usages_keep_the_nearest_same_named_nested_owner() {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
        let source = r#"
package p;
class Left {
    static class Node {}
    Node value;
}
class Right {
    static class Node {}
    Node value;
    Node echo(Node input) { return input; }
}
"#;
        let path = temp.path().join("p/Containers.java");
        workspace
            .index_file_with_source(&path, source)
            .expect("nested type fixture should index");
        let file_id = workspace.get_file_id(&path).expect("fixture file id");
        let imports = workspace.get_imports(file_id).expect("fixture imports");

        let usages = UsageExtractor::extract(source, file_id, &imports, &workspace)
            .expect("usages should extract");
        let node_offsets = source
            .match_indices("Node")
            .map(|(offset, _)| u32::try_from(offset).expect("fixture offset fits u32"))
            .collect::<Vec<_>>();
        assert_eq!(node_offsets.len(), 6);
        let type_identity_at = |offset| {
            usages
                .iter()
                .find(|usage| {
                    usage.location.range.start == offset && usage.location.kind == SymbolKind::CLASS
                })
                .map(|usage| usage.fqn.as_str())
        };

        assert_eq!(type_identity_at(node_offsets[1]), Some("p.Left.Node"));
        for offset in &node_offsets[3..] {
            assert_eq!(type_identity_at(*offset), Some("p.Right.Node"));
        }
        assert!(usages.iter().all(|usage| {
            usage.location.range.start < node_offsets[3] || usage.fqn != "p.Left.Node"
        }));

        let published_offsets = workspace
            .find_usages("p.Right.Node")
            .into_iter()
            .map(|location| location.range.start)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            published_offsets,
            node_offsets[3..].iter().copied().collect(),
            "index publication must use the revision-aware lexical resolver"
        );

        let current = workspace
            .find_current_usages_for_symbol_target(&crate::SymbolTarget::new(
                "p.Right.Node",
                crate::DeclarationNamespace::Type,
            ))
            .expect("current-usage audit should accept every nested type spelling");
        let current_offsets = current
            .locations()
            .map(|location| location.range.start)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            current_offsets,
            node_offsets[3..].iter().copied().collect(),
            "current usage publication must preserve the extractor's lexical identity"
        );
    }

    #[test]
    fn test_extract_method_reference_usage() {
        let source = r#"
package com.example;

import java.util.function.Function;

public class Main {
    public void run() {
        Function<String, String> f = String::toUpperCase;
    }
}
        "#;

        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);
        assert!(
            root.descendants()
                .any(|node| node.kind() == SyntaxKind::METHOD_REF_EXPR),
            "Expected METHOD_REF_EXPR in parse tree"
        );

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());

        let workspace = make_test_workspace();
        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &workspace).expect("extract");

        assert!(
            usages
                .iter()
                .any(|u| u.fqn == "java.lang.String.toUpperCase()"),
            "Expected method reference usage to be recorded, got: {:?}",
            usages.iter().map(|u| u.fqn.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_parser_creates_field_access_expr() {
        let source = r#"
package com.example;

public class Service {
    public void process() {
        ResourceType type = ResourceType.UNKNOWN;
    }
}
        "#;
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        // Walk and print all nodes with their content
        fn print_tree(node: &SyntaxNode, depth: usize) {
            let indent = "  ".repeat(depth);
            let text: String = node.text().to_string().chars().take(50).collect();
            println!(
                "{}Node: {:?} range={:?} text={:?}",
                indent,
                node.kind(),
                node.text_range(),
                text
            );
            for child in node.children() {
                print_tree(&child, depth + 1);
            }
        }

        print_tree(&root, 0);

        // Check if we have any FIELD_ACCESS_EXPR nodes
        println!("\nFIELD_ACCESS_EXPR nodes:");
        for node in root.descendants() {
            if node.kind() == SyntaxKind::FIELD_ACCESS_EXPR {
                println!(
                    "  Found at {:?}, text: {:?}",
                    node.text_range(),
                    node.text()
                );
                println!("  Children:");
                for child in node.children_with_tokens() {
                    println!("    {:?}", child);
                }
            }
        }

        println!("\nNAME_EXPR nodes:");
        for node in root.descendants() {
            if node.kind() == SyntaxKind::NAME_EXPR {
                println!(
                    "  Found at {:?}, text: {:?}",
                    node.text_range(),
                    node.text()
                );
                println!("  Children:");
                for child in node.children_with_tokens() {
                    println!("    {:?}", child);
                }
            }
        }

        println!("\nLOCAL_VAR_DECL nodes:");
        for node in root.descendants() {
            if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
                println!(
                    "  Found at {:?}, text: {:?}",
                    node.text_range(),
                    node.text()
                );
                println!("  Direct children with tokens:");
                for child in node.children_with_tokens() {
                    if let rowan::NodeOrToken::Node(n) = &child {
                        println!("    Node {:?} at {:?}", n.kind(), n.text_range());
                    } else if let rowan::NodeOrToken::Token(t) = &child {
                        println!(
                            "    Token {:?} at {:?} = {:?}",
                            t.kind(),
                            t.text_range(),
                            t.text()
                        );
                    }
                }
            }
        }

        let has_field_access = root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::FIELD_ACCESS_EXPR);

        assert!(
            has_field_access,
            "Parser should create FIELD_ACCESS_EXPR for ResourceType.UNKNOWN"
        );
    }

    #[test]
    fn test_parser_method_arg_with_qualified_access() {
        let source = r#"
public class Client {
    public void test() {
        service.process(ResourceType.UNKNOWN);
    }
}
        "#;
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        // Print tree
        fn print_tree(node: &SyntaxNode, depth: usize) {
            let indent = "  ".repeat(depth);
            println!(
                "{}Node: {:?} at {:?}",
                indent,
                node.kind(),
                node.text_range()
            );
            for child in node.children() {
                print_tree(&child, depth + 1);
            }
        }

        println!("\nTree for method call with qualified arg:");
        print_tree(&root, 0);

        println!("\nAll nodes:");
        for node in root.descendants() {
            println!("  {:?}", node.kind());
        }

        // Print all NAME_EXPR nodes
        println!("\nNAME_EXPR nodes:");
        for node in root.descendants() {
            if node.kind() == SyntaxKind::NAME_EXPR {
                println!(
                    "  Found at {:?}, text: {:?}",
                    node.text_range(),
                    node.text()
                );
                if let Some(parent) = node.parent() {
                    println!("  Parent: {:?}", parent.kind());
                    println!("  Siblings:");
                    for sibling in parent.children_with_tokens() {
                        match sibling {
                            rowan::NodeOrToken::Node(n) => println!(
                                "    Node {:?} at {:?} text={:?}",
                                n.kind(),
                                n.text_range(),
                                n.text().to_string().chars().take(20).collect::<String>()
                            ),
                            rowan::NodeOrToken::Token(t) => {
                                println!("    Token {:?} = {:?}", t.kind(), t.text())
                            }
                        }
                    }
                }
            }
        }

        // We expect NAME_EXPR even if no FIELD_ACCESS_EXPR
        let has_name_expr = root
            .descendants()
            .any(|n| n.kind() == SyntaxKind::NAME_EXPR);
        assert!(
            has_name_expr,
            "Should have NAME_EXPR for ResourceType.UNKNOWN"
        );
    }

    #[test]
    fn test_method_arg_user() {
        let source = r#"
public class Test {
    public void test() {
        process(ResourceType.USER);
    }
}
        "#;
        let imports = make_test_imports();
        let file_id = FileId::new(1);

        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!("Extracted {} usages:", usages.len());
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find ResourceType.USER
        let found = usages.iter().any(|u| u.fqn.contains("ResourceType.USER"));
        assert!(
            found,
            "Should extract ResourceType.USER usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    // Additional tests are kept for integration testing the full extraction pipeline
    // They test the public API and don't need to be split across modules

    #[test]
    fn test_extract_qualified_field_access() {
        let source = r#"
package com.example;

public class Service {
    public void process() {
        ResourceType type = ResourceType.UNKNOWN;
    }
}
        "#;

        let imports = make_test_imports();
        let file_id = FileId::new(1);

        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!("Extracted {} usages:", usages.len());
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find at least one usage of ResourceType.UNKNOWN
        let found = usages
            .iter()
            .any(|u| u.fqn.contains("ResourceType.UNKNOWN"));

        assert!(
            found,
            "Should extract ResourceType.UNKNOWN usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_multiple_qualified_accesses() {
        let source = r#"
package com.example;

public class Service {
    public void process() {
        ResourceType type1 = ResourceType.UNKNOWN;
        if (type1 == ResourceType.USER) {
            System.out.println("User");
        }
    }
}
        "#;

        let imports = make_test_imports();
        let file_id = FileId::new(1);

        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Should find multiple usages
        assert!(
            usages.len() >= 2,
            "Should find at least 2 qualified accesses"
        );
    }

    #[test]
    fn test_constructor_in_field_initializer() {
        // Verifies that constructor calls in field initializers are extracted as usages
        let source = r#"
package com.example;

public class ResourceType {
    public static final ResourceType UNKNOWN = new ResourceType();
}
        "#;

        let imports = make_test_imports();
        let file_id = FileId::new(1);

        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Should extract "new ResourceType()" as a usage
        // The class declaration itself (class ResourceType) is NOT a usage
        // But the constructor call IS a usage of the class
        let constructor_usage = usages.iter().any(|u| {
            u.location.name.as_ref() == "ResourceType" && u.fqn == "com.example.ResourceType"
        });

        assert!(
            constructor_usage,
            "Should extract new ResourceType() as a usage"
        );
    }

    #[test]
    fn test_static_method_call_extraction() {
        let source = r#"
package com.example;

import java.util.Objects;

public class Service {
    public void process(String a, String b) {
        Objects.equals(a, b);
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("Objects".to_string(), "java.util.Objects".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!("Extracted {} usages:", usages.len());
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Declared parameter types make the descriptor precise at index time.
        let found = usages
            .iter()
            .any(|u| u.fqn == "java.util.Objects.equals(java.lang.String,java.lang.String)");
        assert!(
            found,
            "Should extract the typed Objects.equals usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_constructor_call_extraction() {
        let source = r#"
package com.example;

import com.example.model.User;
import java.util.ArrayList;

public class Service {
    public void process() {
        User user = new User();
        ArrayList<String> list = new ArrayList<>();
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("User".to_string(), "com.example.model.User".to_string());
        imports.add_import("ArrayList".to_string(), "java.util.ArrayList".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Should find new User() usage
        // Phase 1: FQN now includes argument count: com.example.model.User.User()
        let found_user = usages.iter().any(|u| {
            u.fqn == "com.example.model.User.User()" && u.location.kind == SymbolKind::CONSTRUCTOR
        });
        assert!(
            found_user,
            "Should extract new User() usage with argument count, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find new ArrayList<>() usage
        // Phase 1: FQN now includes argument count: java.util.ArrayList.ArrayList()
        let found_arraylist = usages.iter().any(|u| {
            u.fqn == "java.util.ArrayList.ArrayList()" && u.location.kind == SymbolKind::CONSTRUCTOR
        });
        assert!(
            found_arraylist,
            "Should extract new ArrayList<>() usage with argument count, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn nested_constructor_keeps_its_declaring_type_identity() {
        let source = r#"
package com.example;

class Outer {
    static class Inner {}
}

class Use {
    Object value = new Outer.Inner();
}
"#;
        let workspace = WorkspaceIndex::new(PathBuf::new());
        let path = PathBuf::from("com/example/Use.java");
        workspace
            .index_file_with_source(&path, source)
            .expect("nested constructor fixture should index");
        let file_id = workspace.get_file_id(&path).expect("indexed file id");
        let imports = workspace.get_imports(file_id).expect("indexed imports");

        let usages = UsageExtractor::extract(source, file_id, &imports, &workspace).unwrap();

        assert!(
            usages.iter().any(|usage| {
                usage.fqn == "com.example.Outer.Inner.Inner()"
                    && usage.location.kind == SymbolKind::CONSTRUCTOR
                    && usage.location.name.as_ref() == "Inner"
            }),
            "nested constructor must not be attributed to the outer class: {:?}",
            usages.iter().map(|usage| &usage.fqn).collect::<Vec<_>>()
        );
        assert!(
            usages
                .iter()
                .all(|usage| usage.fqn != "com.example.Outer.Outer()"),
            "legacy outer-class constructor identity must disappear"
        );
    }

    #[test]
    fn multi_hop_field_chain_uses_inherited_and_projected_owners() {
        let source = r#"
package p;

class Address { String city; }
class Parent<T> { T address; }
class Child extends Parent<Address> {}

class Use {
    String read(Child child) {
        return child.address.city;
    }
}
"#;
        let workspace = WorkspaceIndex::new(PathBuf::new());
        let path = PathBuf::from("p/Use.java");
        workspace
            .index_file_with_source(&path, source)
            .expect("field-chain fixture should index");
        let file_id = workspace.get_file_id(&path).expect("indexed file id");
        let imports = workspace.get_imports(file_id).expect("indexed imports");

        let usages = UsageExtractor::extract(source, file_id, &imports, &workspace).unwrap();

        assert!(
            usages.iter().any(|usage| {
                usage.fqn == "p.Address.city"
                    && usage.location.name.as_ref() == "city"
                    && usage.location.container_name.as_deref() == Some("p.Address")
            }),
            "the final field must belong to its projected declaring type: {:?}",
            usages.iter().map(|usage| &usage.fqn).collect::<Vec<_>>()
        );
        assert!(
            usages
                .iter()
                .all(|usage| usage.fqn != "p.Child.address.city"),
            "a concatenated receiver spelling is not a declaration identity"
        );
    }

    #[test]
    fn test_type_reference_extraction() {
        let source = r#"
package com.example;

import com.example.model.User;
import java.util.List;

public class Service {
    private User currentUser;
    private List<User> users;

    public User getUser() {
        return currentUser;
    }

    public void process(User user) {
        // ...
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("User".to_string(), "com.example.model.User".to_string());
        imports.add_import("List".to_string(), "java.util.List".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!("Extracted {} usages (type reference test):", usages.len());
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find multiple User type references
        let user_count = usages
            .iter()
            .filter(|u| u.fqn == "com.example.model.User")
            .count();
        assert!(
            user_count >= 3,
            "Should find at least 3 User type references (field, return type, param), found: {}",
            user_count
        );

        // Should find List type reference
        let list_count = usages.iter().filter(|u| u.fqn == "java.util.List").count();
        assert!(
            list_count >= 1,
            "Should find at least 1 List type reference, found: {}",
            list_count
        );
    }

    #[test]
    fn test_extends_implements_extraction() {
        let source = r#"
package com.example;

import com.example.base.BaseService;
import com.example.api.UserHandler;
import java.io.Serializable;

public class Service extends BaseService implements UserHandler, Serializable {
    public void handle() {}
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import(
            "BaseService".to_string(),
            "com.example.base.BaseService".to_string(),
        );
        imports.add_import(
            "UserHandler".to_string(),
            "com.example.api.UserHandler".to_string(),
        );
        imports.add_import(
            "Serializable".to_string(),
            "java.io.Serializable".to_string(),
        );

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!(
            "Extracted {} usages (extends/implements test):",
            usages.len()
        );
        for usage in &usages {
            println!(
                "  - FQN: {} (name: {}, kind: {:?})",
                usage.fqn, usage.location.name, usage.location.kind
            );
        }

        // Should find extends BaseService
        let found_base = usages
            .iter()
            .any(|u| u.fqn == "com.example.base.BaseService");
        assert!(
            found_base,
            "Should extract extends BaseService, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find implements UserHandler
        let found_handler = usages
            .iter()
            .any(|u| u.fqn == "com.example.api.UserHandler");
        assert!(
            found_handler,
            "Should extract implements UserHandler, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find implements Serializable
        let found_serializable = usages.iter().any(|u| u.fqn == "java.io.Serializable");
        assert!(
            found_serializable,
            "Should extract implements Serializable, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_generic_type_args_extraction() {
        let source = r#"
package com.example;

import com.example.model.User;
import java.util.Map;
import java.util.List;

public class Service {
    private Map<String, List<User>> usersByName;
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("User".to_string(), "com.example.model.User".to_string());
        imports.add_import("Map".to_string(), "java.util.Map".to_string());
        imports.add_import("List".to_string(), "java.util.List".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!(
            "Extracted {} usages (generic type args test):",
            usages.len()
        );
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find Map
        let found_map = usages.iter().any(|u| u.fqn == "java.util.Map");
        assert!(
            found_map,
            "Should extract Map type, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find List inside generic args
        let found_list = usages.iter().any(|u| u.fqn == "java.util.List");
        assert!(
            found_list,
            "Should extract List from generic args, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find User inside nested generic args
        let found_user = usages.iter().any(|u| u.fqn == "com.example.model.User");
        assert!(
            found_user,
            "Should extract User from nested generic args, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_this_method_call_extraction() {
        let source = r#"
package com.example;

public class Service {
    private String data;

    public void process() {
        this.load();
        this.save();
    }

    private void load() {}
    private void save() {}
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!("Extracted {} usages (this.method test):", usages.len());
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find this.load() -> com.example.Service.load()
        let found_load = usages.iter().any(|u| u.fqn == "com.example.Service.load()");
        assert!(
            found_load,
            "Should extract this.load() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find this.save() -> com.example.Service.save()
        let found_save = usages.iter().any(|u| u.fqn == "com.example.Service.save()");
        assert!(
            found_save,
            "Should extract this.save() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_unqualified_method_call_extraction() {
        let source = r#"
package com.example;

public class Service {
    public void process() {
        load();
        validate();
    }

    private void load() {}
    private void validate() {}
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());

        let workspace = make_test_workspace();
        let path = PathBuf::from("/tmp/test/Service.java");
        workspace.index_file_with_source(&path, source).unwrap();
        let file_id = workspace.get_file_id(&path).expect("indexed test source");
        let usages = UsageExtractor::extract(source, file_id, &imports, &workspace).unwrap();

        println!(
            "Extracted {} usages (unqualified method test):",
            usages.len()
        );
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find load() -> com.example.Service.load()
        let found_load = usages.iter().any(|u| u.fqn == "com.example.Service.load()");
        assert!(
            found_load,
            "Should extract load() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find validate() -> com.example.Service.validate()
        let found_validate = usages
            .iter()
            .any(|u| u.fqn == "com.example.Service.validate()");
        assert!(
            found_validate,
            "Should extract validate() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_instance_method_call_extraction() {
        let source = r#"
package com.example;

import com.example.model.User;

public class Service {
    private User currentUser;

    public void process(User user) {
        User localUser = new User();

        // Instance method calls on different variable sources
        currentUser.getName();  // field
        user.getEmail();        // parameter
        localUser.getId();      // local variable
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("User".to_string(), "com.example.model.User".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!("Extracted {} usages (instance method test):", usages.len());
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find currentUser.getName() -> com.example.model.User.getName() (field)
        let found_get_name = usages
            .iter()
            .any(|u| u.fqn == "com.example.model.User.getName()");
        assert!(
            found_get_name,
            "Should extract currentUser.getName() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find user.getEmail() -> com.example.model.User.getEmail() (parameter)
        let found_get_email = usages
            .iter()
            .any(|u| u.fqn == "com.example.model.User.getEmail()");
        assert!(
            found_get_email,
            "Should extract user.getEmail() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );

        // Should find localUser.getId() -> com.example.model.User.getId() (local variable)
        let found_get_id = usages
            .iter()
            .any(|u| u.fqn == "com.example.model.User.getId()");
        assert!(
            found_get_id,
            "Should extract localUser.getId() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_chained_method_calls() {
        let source = r#"
package com.example;

import com.example.model.User;

public class Service {
    public void process(User user) {
        // Only the first call in a chain can be resolved
        // user.getName() is resolved, but .length() needs return type tracking
        user.getName();
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("User".to_string(), "com.example.model.User".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        println!("Extracted {} usages (chained method test):", usages.len());
        for usage in &usages {
            println!("  - FQN: {} (name: {})", usage.fqn, usage.location.name);
        }

        // Should find user.getName() -> com.example.model.User.getName()
        let found_get_name = usages
            .iter()
            .any(|u| u.fqn == "com.example.model.User.getName()");
        assert!(
            found_get_name,
            "Should extract user.getName() usage, got: {:?}",
            usages.iter().map(|u| &u.fqn).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_class_literal_method_call_uses_java_lang_class_receiver() {
        let source = r#"
package com.example;

class Entity {}

class ReflectionClient {
    void run() throws Exception {
        Entity.class.getDeclaredMethod("de" + "ad").invoke(new Entity());
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        let usages =
            UsageExtractor::extract(source, FileId::new(1), &imports, &make_test_workspace())
                .unwrap();

        assert!(
            usages.iter().any(|usage| {
                usage.location.name.as_ref() == "getDeclaredMethod"
                    && usage.fqn.starts_with("java.lang.Class.getDeclaredMethod(")
            }),
            "class-literal reflection call was not attributed to java.lang.Class: {usages:#?}"
        );
    }

    // Document highlight range tests - verify ranges only cover identifiers, not full expressions
    // This is critical for proper highlighting in editors when using document_highlight

    #[test]
    fn test_field_access_range_only_covers_identifier() {
        // Test that ResourceType.UNKNOWN range only covers "UNKNOWN", not "ResourceType.UNKNOWN"
        let source = r#"
package com.example;

public class Service {
    public void process() {
        ResourceType type = ResourceType.UNKNOWN;
    }
}
        "#;

        let imports = make_test_imports();
        let file_id = FileId::new(1);

        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Find the ResourceType.UNKNOWN usage
        let usage = usages
            .iter()
            .find(|u| u.fqn.contains("ResourceType.UNKNOWN"));
        assert!(usage.is_some(), "Should find ResourceType.UNKNOWN usage");

        let usage = usage.unwrap();
        let range_start: u32 = usage.location.range.start;
        let range_end: u32 = usage.location.range.end;
        let range_len = range_end - range_start;

        // The range should only cover "UNKNOWN" (7 chars), not "ResourceType.UNKNOWN" (20 chars)
        assert!(
            range_len <= 10,
            "Range should only cover identifier 'UNKNOWN' (7 chars), not full expression. Got range length: {}",
            range_len
        );
    }

    #[test]
    fn test_method_call_range_only_covers_method_name() {
        // Test that user.getName() range only covers "getName", not "user.getName"
        let source = r#"
package com.example;

import com.example.model.User;

public class Service {
    public void process(User user) {
        user.getName();
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("User".to_string(), "com.example.model.User".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Find the getName method call usage
        let usage = usages.iter().find(|u| u.fqn.contains("User.getName"));
        assert!(usage.is_some(), "Should find User.getName usage");

        let usage = usage.unwrap();
        let range_start: u32 = usage.location.range.start;
        let range_end: u32 = usage.location.range.end;
        let range_len = range_end - range_start;

        // The range should only cover "getName" (7 chars), not "user.getName" (12 chars)
        assert!(
            range_len <= 10,
            "Range should only cover method name 'getName' (7 chars), not full expression. Got range length: {}",
            range_len
        );
    }

    #[test]
    fn test_static_method_call_range_only_covers_method_name() {
        // Test that Objects.equals() range only covers "equals", not "Objects.equals"
        let source = r#"
package com.example;

import java.util.Objects;

public class Service {
    public void process(String a, String b) {
        Objects.equals(a, b);
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("Objects".to_string(), "java.util.Objects".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Find the Objects.equals method call usage
        let usage = usages.iter().find(|u| u.fqn.contains("Objects.equals"));
        assert!(usage.is_some(), "Should find Objects.equals usage");

        let usage = usage.unwrap();
        let range_start: u32 = usage.location.range.start;
        let range_end: u32 = usage.location.range.end;
        let range_len = range_end - range_start;

        // The range should only cover "equals" (6 chars), not "Objects.equals" (14 chars)
        assert!(
            range_len <= 10,
            "Range should only cover method name 'equals' (6 chars), not full expression. Got range length: {}",
            range_len
        );
    }

    #[test]
    fn test_type_reference_range_only_covers_type_name() {
        // Test that type references like "User user" range only covers "User"
        let source = r#"
package com.example;

import com.example.model.User;

public class Service {
    private User currentUser;
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());
        imports.add_import("User".to_string(), "com.example.model.User".to_string());

        let file_id = FileId::new(1);
        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Find the User type reference
        let usage = usages.iter().find(|u| u.fqn == "com.example.model.User");
        assert!(usage.is_some(), "Should find User type reference");

        let usage = usage.unwrap();
        let range_start: u32 = usage.location.range.start;
        let range_end: u32 = usage.location.range.end;
        let range_len = range_end - range_start;

        // The range should only cover "User" (4 chars)
        assert!(
            range_len <= 6,
            "Range should only cover type name 'User' (4 chars). Got range length: {}",
            range_len
        );
    }

    #[test]
    fn test_qualified_name_in_method_arg_range() {
        // Test that ResourceType.USER in method arg has correct range
        let source = r#"
package com.example;

public class Service {
    public void test() {
        process(ResourceType.USER);
    }
    public void process(Object o) {}
}
        "#;

        let imports = make_test_imports();
        let file_id = FileId::new(1);

        let usages =
            UsageExtractor::extract(source, file_id, &imports, &make_test_workspace()).unwrap();

        // Find the ResourceType.USER usage
        let usage = usages.iter().find(|u| u.fqn.contains("ResourceType.USER"));
        if let Some(usage) = usage {
            let range_start: u32 = usage.location.range.start;
            let range_end: u32 = usage.location.range.end;
            let range_len = range_end - range_start;

            // The range should only cover "USER" (4 chars), not "ResourceType.USER" (17 chars)
            assert!(
                range_len <= 8,
                "Range should only cover identifier 'USER' (4 chars), not full expression. Got range length: {}",
                range_len
            );
        }
    }

    #[test]
    fn test_unqualified_field_usage_tracks_field_reference() {
        let source = r#"
package com.example;

public class Counter {
    private int count = 0;

    public void increment() {
        count++;
    }

    public void shadow() {
        int count = 0;
        count++;
    }
}
        "#;

        let mut imports = ImportMap::new();
        imports.current_package = Some("com.example".into());

        let workspace = make_test_workspace();
        let path = PathBuf::from("/tmp/test/Counter.java");
        workspace.index_file_with_source(&path, source).unwrap();
        let file_id = workspace.get_file_id(&path).expect("indexed test source");
        let usages = UsageExtractor::extract(source, file_id, &imports, &workspace).unwrap();

        let field_usages: Vec<_> = usages
            .iter()
            .filter(|u| u.fqn == "com.example.Counter.count")
            .collect();

        assert!(
            field_usages.len() == 1,
            "Expected one field usage for Counter.count, got: {:?}",
            field_usages
                .iter()
                .map(|u| u.location.range)
                .collect::<Vec<_>>()
        );
    }
}
