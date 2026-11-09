// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Cross-file analysis context for interprocedural taint analysis.
//!
//! Builds workspace-backed analysis infrastructure including:
//! - Constant field extraction
//! - Source annotation detection
//! - Call graph construction
//! - Method summary storage

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};
use hyperion_workspace::WorkspaceIndex;

use crate::constant_eval::{ConstantEvaluator, ConstantValue};
use crate::interprocedural::{MethodSummary, SummaryDatabase};
use crate::sources::SourceKind;

/// Cross-file analysis context for interprocedural taint analysis.
///
/// This structure holds all the pre-computed information needed to perform
/// cross-file taint analysis, including method summaries,
/// constant fields, and source annotations.
pub struct AnalysisContext {
    /// Reference to the workspace index for symbol resolution.
    pub workspace: Arc<WorkspaceIndex>,
    /// Database of method summaries.
    pub summaries: SummaryDatabase,
    /// Pool of constant field values (FQN -> ConstantValue).
    pub constant_pool: HashMap<String, ConstantValue>,
    /// Source annotations on method parameters (method FQN -> [(param_index, source_kind)]).
    pub source_annotations: HashMap<String, Vec<(usize, SourceKind)>>,
    /// Set of tainted parameters after fixpoint (method FQN -> set of param indices).
    pub tainted_params: HashMap<String, HashSet<usize>>,
}

impl AnalysisContext {
    /// Create an analysis context from a workspace index.
    pub fn from_workspace(workspace: Arc<WorkspaceIndex>) -> Self {
        let mut context = Self {
            workspace: workspace.clone(),
            summaries: SummaryDatabase::new(Some(workspace)),
            constant_pool: HashMap::new(),
            source_annotations: HashMap::new(),
            tainted_params: HashMap::new(),
        };

        context.extract_constant_fields();
        context.extract_source_annotations();

        context
    }

    /// Create an analysis context by scanning a directory.
    pub fn from_directory(path: &Path) -> std::io::Result<Self> {
        use walkdir::WalkDir;

        // Find all Java files
        let java_files: Vec<_> = WalkDir::new(path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| {
                e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "java")
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        let workspace = Arc::new(WorkspaceIndex::new(path.to_path_buf()));
        workspace.index_files_parallel(&java_files);

        Ok(Self::from_workspace(workspace))
    }

    /// Extract constant fields from all classes in the workspace.
    fn extract_constant_fields(&mut self) {
        let evaluator = ConstantEvaluator::new();

        // Iterate all indexed files
        for file_id in self.workspace.all_file_ids() {
            let Some(contents) = self.workspace.get_file_contents(file_id) else {
                continue;
            };
            let Some(path) = self.workspace.get_file_path(file_id) else {
                continue;
            };

            // Only process Java files
            if path.extension().is_none_or(|ext| ext != "java") {
                continue;
            }

            // Parse and extract constants
            let tokens: Vec<_> = Lexer::new(&contents).collect();
            let (green, _errors) = Parser::parse(tokens);
            let root = SyntaxNode::new_root(green);

            let package_name = extract_package_name(&root);

            for class_node in root.descendants() {
                if !matches!(
                    class_node.kind(),
                    SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL | SyntaxKind::ENUM_DECL
                ) {
                    continue;
                }

                let class_name = extract_class_name(&class_node);
                let Some(class_name) = class_name else {
                    continue;
                };

                let class_fqn = package_name
                    .as_ref()
                    .map_or_else(|| class_name.clone(), |pkg| format!("{pkg}.{class_name}"));

                // Extract constant fields
                for child in class_node.descendants() {
                    if child.kind() == SyntaxKind::FIELD_DECL {
                        if let Some((name, value)) = extract_constant_field(&child, &evaluator) {
                            let field_fqn = format!("{class_fqn}.{name}");
                            self.constant_pool.insert(field_fqn, value);
                        }
                    }
                }
            }
        }
    }

    /// Extract source annotations from method parameters.
    fn extract_source_annotations(&mut self) {
        for file_id in self.workspace.all_file_ids() {
            let Some(contents) = self.workspace.get_file_contents(file_id) else {
                continue;
            };
            let Some(path) = self.workspace.get_file_path(file_id) else {
                continue;
            };

            if path.extension().is_none_or(|ext| ext != "java") {
                continue;
            }

            let tokens: Vec<_> = Lexer::new(&contents).collect();
            let (green, _errors) = Parser::parse(tokens);
            let root = SyntaxNode::new_root(green);

            let package_name = extract_package_name(&root);

            for class_node in root.descendants() {
                if !matches!(
                    class_node.kind(),
                    SyntaxKind::CLASS_DECL | SyntaxKind::INTERFACE_DECL
                ) {
                    continue;
                }

                let class_name = extract_class_name(&class_node);
                let Some(class_name) = class_name else {
                    continue;
                };

                let class_fqn = package_name
                    .as_ref()
                    .map_or_else(|| class_name.clone(), |pkg| format!("{pkg}.{class_name}"));

                // Find methods with annotated parameters
                for method_node in class_node.descendants() {
                    if !matches!(
                        method_node.kind(),
                        SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
                    ) {
                        continue;
                    }

                    let method_name = extract_method_name(&method_node);
                    let Some(method_name) = method_name else {
                        continue;
                    };

                    let method_fqn = format!("{class_fqn}.{method_name}");
                    let annotations = extract_param_source_annotations(&method_node);

                    if !annotations.is_empty() {
                        self.source_annotations.insert(method_fqn, annotations);
                    }
                }
            }
        }
    }

    /// Check if a field is a constant.
    #[must_use]
    pub fn is_constant_field(&self, fqn: &str) -> bool {
        self.constant_pool.contains_key(fqn)
    }

    /// Get a constant value by FQN.
    #[must_use]
    pub fn get_constant(&self, fqn: &str) -> Option<&ConstantValue> {
        self.constant_pool.get(fqn)
    }

    /// Get source annotations for a method.
    #[must_use]
    pub fn get_source_annotations(&self, method_fqn: &str) -> Option<&Vec<(usize, SourceKind)>> {
        self.source_annotations.get(method_fqn)
    }

    /// Check if a parameter is tainted (after fixpoint analysis).
    #[must_use]
    pub fn is_param_tainted(&self, method_fqn: &str, param_idx: usize) -> bool {
        self.tainted_params
            .get(method_fqn)
            .is_some_and(|set| set.contains(&param_idx))
    }

    /// Mark a parameter as tainted.
    pub fn mark_param_tainted(&mut self, method_fqn: &str, param_idx: usize) -> bool {
        let set = self
            .tainted_params
            .entry(method_fqn.to_string())
            .or_default();
        set.insert(param_idx)
    }

    /// Add a method summary to the database.
    pub fn add_summary(&mut self, summary: MethodSummary) {
        self.summaries.add_summary(summary);
    }

    /// Get a method summary by FQN.
    #[must_use]
    pub fn get_summary(&self, fqn: &str) -> Option<Arc<MethodSummary>> {
        self.summaries.get_summary(fqn)
    }
}

// Helper functions

fn extract_package_name(root: &SyntaxNode) -> Option<String> {
    for child in root.children() {
        if child.kind() == SyntaxKind::PACKAGE_DECL {
            let mut parts = Vec::new();
            collect_name_parts(&child, &mut parts);
            if !parts.is_empty() {
                return Some(parts.join("."));
            }
        }
    }
    None
}

fn collect_name_parts(node: &SyntaxNode, parts: &mut Vec<String>) {
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT {
                    parts.push(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(node) => {
                collect_name_parts(&node, parts);
            }
        }
    }
}

fn extract_class_name(class_node: &SyntaxNode) -> Option<String> {
    for child in class_node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

fn extract_method_name(method_node: &SyntaxNode) -> Option<String> {
    // For constructor, use the class name
    if method_node.kind() == SyntaxKind::CONSTRUCTOR_DECL {
        for child in method_node.children_with_tokens() {
            if let Some(token) = child.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    return Some("<init>".to_string());
                }
            }
        }
        return None;
    }

    for child in method_node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

fn extract_constant_field(
    field: &SyntaxNode,
    evaluator: &ConstantEvaluator,
) -> Option<(String, ConstantValue)> {
    if !evaluator.is_constant_field(field) {
        return None;
    }

    let mut field_name = None;
    let mut field_value = None;

    for child in field.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT && field_name.is_none() {
                    field_name = Some(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(node) => {
                if is_expression_kind(node.kind()) {
                    field_value = evaluator.evaluate(&node);
                }
            }
        }
    }

    match (field_name, field_value) {
        (Some(name), Some(value)) => Some((name, value)),
        _ => None,
    }
}

fn extract_param_source_annotations(method_node: &SyntaxNode) -> Vec<(usize, SourceKind)> {
    let mut annotations = Vec::new();

    // Find PARAM_LIST
    for child in method_node.children() {
        if child.kind() == SyntaxKind::PARAM_LIST {
            let mut param_idx = 0;

            for param in child.children() {
                if param.kind() == SyntaxKind::PARAM {
                    // Check annotations on this parameter
                    for param_child in param.children() {
                        if param_child.kind() == SyntaxKind::MODIFIER_LIST {
                            for modifier in param_child.children() {
                                if modifier.kind() == SyntaxKind::ANNOTATION {
                                    if let Some(source_kind) = annotation_to_source_kind(&modifier)
                                    {
                                        annotations.push((param_idx, source_kind));
                                    }
                                }
                            }
                        }
                    }
                    param_idx += 1;
                }
            }
        }
    }

    annotations
}

fn annotation_to_source_kind(annotation: &SyntaxNode) -> Option<SourceKind> {
    // Extract annotation name — the last IDENT in the annotation path
    // e.g., @org.springframework.web.bind.annotation.RequestParam → "RequestParam"
    // The IDENT may be inside a QUALIFIED_NAME child, so use descendants
    let mut name = None;
    for child in annotation.descendants_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                name = Some(token.text().to_string());
            }
        }
    }

    match name.as_deref() {
        // Spring annotations and JAX-RS annotations
        Some(
            "RequestParam" | "PathVariable" | "RequestBody" | "RequestHeader" | "CookieValue"
            | "MatrixVariable" | "QueryParam" | "PathParam" | "FormParam" | "HeaderParam"
            | "CookieParam" | "BeanParam",
        ) => Some(SourceKind::HttpRequest),
        _ => None,
    }
}

const fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_workspace(files: &[(&str, &str)]) -> (TempDir, Arc<WorkspaceIndex>) {
        let temp_dir = TempDir::new().unwrap();

        let mut java_files = Vec::new();
        for (name, content) in files {
            let path = temp_dir.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            let mut file = std::fs::File::create(&path).unwrap();
            file.write_all(content.as_bytes()).unwrap();

            if std::path::Path::new(name)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
            {
                java_files.push((path, content.to_string()));
            }
        }

        let workspace = Arc::new(WorkspaceIndex::new(temp_dir.path().to_path_buf()));

        // Index files sequentially to avoid rayon thread pool issues in tests
        for (path, source) in &java_files {
            workspace
                .index_file_with_source(path, source)
                .unwrap_or_else(|e| panic!("Failed to index {}: {e}", path.display()));
        }

        let file_ids = workspace.all_file_ids();
        assert_eq!(
            file_ids.len(),
            java_files.len(),
            "Workspace indexing failed: expected {} files, got {}",
            java_files.len(),
            file_ids.len()
        );

        (temp_dir, workspace)
    }

    #[test]
    fn test_extract_constant_fields() {
        let (_temp, workspace) = create_test_workspace(&[(
            "Test.java",
            r#"
            package com.example;

            public class Test {
                public static final String QUERY = "SELECT * FROM users";
                private String nonConstant = someMethod();
            }
            "#,
        )]);

        let context = AnalysisContext::from_workspace(workspace);

        assert!(context.is_constant_field("com.example.Test.QUERY"));
        assert!(!context.is_constant_field("com.example.Test.nonConstant"));

        if let Some(value) = context.get_constant("com.example.Test.QUERY") {
            assert_eq!(value.as_string(), Some("SELECT * FROM users"));
        }
    }

    #[test]
    fn test_extract_source_annotations() {
        let (_temp, workspace) = create_test_workspace(&[(
            "Controller.java",
            r#"
            package com.example;

            public class Controller {
                public void endpoint(@RequestParam String id) {}
            }
            "#,
        )]);

        let context = AnalysisContext::from_workspace(workspace);

        let annotations = context.get_source_annotations("com.example.Controller.endpoint");
        assert!(annotations.is_some());
        let annotations = annotations.unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].0, 0); // First parameter
        assert_eq!(annotations[0].1, SourceKind::HttpRequest);
    }

    #[test]
    fn test_mark_param_tainted() {
        let (_temp, workspace) = create_test_workspace(&[("Test.java", "public class Test {}")]);

        let mut context = AnalysisContext::from_workspace(workspace);

        assert!(!context.is_param_tainted("Test.foo", 0));

        context.mark_param_tainted("Test.foo", 0);
        assert!(context.is_param_tainted("Test.foo", 0));
        assert!(!context.is_param_tainted("Test.foo", 1));
    }
}
