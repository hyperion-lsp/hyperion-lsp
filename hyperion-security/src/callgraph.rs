// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Call graph construction for inter-procedural taint analysis
//!
//! This module builds a call graph directly from the AST, tracking calls among
//! methods in one class for cross-method taint analysis.
//!
//! # Architecture
//!
//! The call graph operates at two levels:
//! 1. **Intra-class**: Methods calling other methods in the same class (implemented here)
//! 2. **Inter-class**: Not represented by this graph; cross-file analysis uses
//!    separate workspace summaries.
//!
//! # Example
//!
//! ```rust,ignore
//! use hyperion_security::callgraph::CallGraph;
//! use hyperion_parser::parse_java;
//!
//! let source = r#"
//!     class UserService {
//!         public void processUser(String name) {
//!             validate(name);
//!             save(name);
//!         }
//!
//!         private void validate(String s) { }
//!         private void save(String s) { }
//!     }
//! "#;
//!
//! let tree = parse_java(source);
//! let call_graph = CallGraph::build(&tree.root(), "UserService");
//!
//! // Query the graph
//! let process_user_id = MethodId::new("UserService", "processUser", 1);
//! let callees = call_graph.get_callees(&process_user_id);
//! // callees contains: validate(1 param), save(1 param)
//! ```

use hyperion_parser::SyntaxKind;
use rowan::TextRange;
use std::collections::{HashMap, HashSet};

/// Identifier for a method within a call graph
///
/// Uses a simplified signature (parameter count) for matching.
/// This is sufficient for intra-class analysis and avoids complex type resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodId {
    /// Class name (e.g., "UserService")
    pub class_name: String,

    /// Method name (e.g., "processUser")
    pub method_name: String,

    /// Simplified signature: parameter count
    /// This allows matching method calls without full type resolution
    pub param_count: usize,
}

impl MethodId {
    /// Create a new method identifier
    pub fn new(
        class_name: impl Into<String>,
        method_name: impl Into<String>,
        param_count: usize,
    ) -> Self {
        Self {
            class_name: class_name.into(),
            method_name: method_name.into(),
            param_count,
        }
    }
}

impl std::fmt::Display for MethodId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}({})",
            self.class_name, self.method_name, self.param_count
        )
    }
}

/// Metadata about a method
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// Method identifier
    pub id: MethodId,

    /// Number of parameters
    pub param_count: usize,

    /// Return type (simplified, extracted from AST)
    pub return_type: Option<String>,

    /// Location in source file
    pub range: TextRange,
}

impl MethodInfo {
    /// Create new method info
    pub const fn new(
        id: MethodId,
        param_count: usize,
        return_type: Option<String>,
        range: TextRange,
    ) -> Self {
        Self {
            id,
            param_count,
            return_type,
            range,
        }
    }
}

/// Call graph tracking method-to-method calls
///
/// This graph answers two key questions:
/// 1. What methods does X call? (forward edges)
/// 2. What methods call X? (reverse edges)
///
/// Both are needed for taint analysis:
/// - Forward edges: propagate taint from caller to callee
/// - Reverse edges: find all paths from source to sink
#[derive(Debug)]
pub struct CallGraph {
    /// Forward edges: method -> methods it calls
    calls: HashMap<MethodId, HashSet<MethodId>>,

    /// Reverse edges: method -> methods that call it
    called_by: HashMap<MethodId, HashSet<MethodId>>,

    /// All methods in the graph with their metadata
    methods: HashMap<MethodId, MethodInfo>,
}

impl CallGraph {
    /// Build a call graph from an AST
    ///
    /// This performs two passes:
    /// 1. Collect all method declarations
    /// 2. For each method body, extract method call expressions
    ///
    /// # Arguments
    /// * `source_file` - Root node of the parsed Java file
    /// * `class_name` - Name of the class being analyzed
    pub fn build(source_file: &hyperion_parser::SyntaxNode, class_name: &str) -> Self {
        let mut graph = Self {
            calls: HashMap::new(),
            called_by: HashMap::new(),
            methods: HashMap::new(),
        };

        // Pass 1: Collect method declarations
        graph.collect_method_declarations(source_file, class_name);

        // Pass 2: Collect method calls
        graph.collect_method_calls(source_file, class_name);

        graph
    }

    /// Get all methods that a given method calls
    pub fn get_callees(&self, method_id: &MethodId) -> HashSet<MethodId> {
        self.calls.get(method_id).cloned().unwrap_or_default()
    }

    /// Get all methods that call a given method
    pub fn get_callers(&self, method_id: &MethodId) -> HashSet<MethodId> {
        self.called_by.get(method_id).cloned().unwrap_or_default()
    }

    /// Get method metadata
    pub fn get_method(&self, id: &MethodId) -> Option<&MethodInfo> {
        self.methods.get(id)
    }

    /// Get all methods in the graph
    pub fn all_methods(&self) -> impl Iterator<Item = &MethodInfo> {
        self.methods.values()
    }

    /// Check if a method exists in the graph
    pub fn contains_method(&self, id: &MethodId) -> bool {
        self.methods.contains_key(id)
    }

    // --- Private methods ---

    /// Pass 1: Collect all method declarations
    fn collect_method_declarations(
        &mut self,
        node: &hyperion_parser::SyntaxNode,
        class_name: &str,
    ) {
        if node.kind() == SyntaxKind::METHOD_DECL {
            if let Some(method_info) = Self::extract_method_info(node, class_name) {
                self.methods.insert(method_info.id.clone(), method_info);
            }
        }

        for child in node.children() {
            self.collect_method_declarations(&child, class_name);
        }
    }

    /// Pass 2: Collect method calls within each method body
    fn collect_method_calls(&mut self, node: &hyperion_parser::SyntaxNode, class_name: &str) {
        if node.kind() == SyntaxKind::METHOD_DECL {
            if let Some(method_info) = Self::extract_method_info(node, class_name) {
                let caller_id = method_info.id;

                // Find method body
                for child in node.children() {
                    if child.kind() == SyntaxKind::BLOCK_STMT {
                        self.collect_calls_in_block(&child, &caller_id, class_name);
                    }
                }
            }
        }

        for child in node.children() {
            self.collect_method_calls(&child, class_name);
        }
    }

    /// Recursively collect method calls within a block
    fn collect_calls_in_block(
        &mut self,
        node: &hyperion_parser::SyntaxNode,
        caller_id: &MethodId,
        class_name: &str,
    ) {
        // Look for EXPR_STMT that contains method calls
        // Structure: EXPR_STMT -> NAME_EXPR (method name) + METHOD_CALL_EXPR (args)
        if node.kind() == SyntaxKind::EXPR_STMT {
            if let Some(callee_id) = Self::extract_method_call_from_expr_stmt(node, class_name) {
                // Only add edge if the callee is declared in this class
                if self.methods.contains_key(&callee_id) {
                    self.add_edge(caller_id.clone(), callee_id);
                }
            }
        }

        for child in node.children() {
            self.collect_calls_in_block(&child, caller_id, class_name);
        }
    }

    /// Extract method information from a METHOD_DECL node
    fn extract_method_info(
        node: &hyperion_parser::SyntaxNode,
        class_name: &str,
    ) -> Option<MethodInfo> {
        // Find method name - it's a token, not a node
        let method_name = node.children_with_tokens().find_map(|child| {
            if let rowan::NodeOrToken::Token(token) = child {
                if token.kind() == SyntaxKind::IDENT {
                    return Some(token.text().to_string());
                }
            }
            None
        })?;

        // Count parameters
        let param_count = node
            .children()
            .find(|n| n.kind() == SyntaxKind::PARAM_LIST)
            .map_or(0, |params| Self::count_parameters(&params));

        // Extract return type
        let return_type = Self::extract_return_type(node);

        let id = MethodId::new(class_name, method_name, param_count);
        let range = node.text_range();

        Some(MethodInfo::new(id, param_count, return_type, range))
    }

    /// Extract method call information from an EXPR_STMT node.
    /// With nested trees, EXPR_STMT contains a METHOD_CALL_EXPR which has
    /// receiver, DOT, IDENT (method name), and ARG_LIST as direct children.
    fn extract_method_call_from_expr_stmt(
        expr_stmt: &hyperion_parser::SyntaxNode,
        class_name: &str,
    ) -> Option<MethodId> {
        // Find the outermost METHOD_CALL_EXPR child
        let call_expr = expr_stmt
            .children()
            .find(|n| n.kind() == SyntaxKind::METHOD_CALL_EXPR)?;

        // Extract method name: IDENT token (for qualified calls) or NAME_EXPR text (unqualified)
        let method_name = call_expr
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|t| t.kind() == SyntaxKind::IDENT)
            .map(|t| t.text().to_string())
            .or_else(|| {
                // Unqualified call: NAME_EXPR contains method name
                call_expr
                    .children()
                    .find(|n| n.kind() == SyntaxKind::NAME_EXPR)
                    .map(|n| n.text().to_string())
            })?;

        let arg_count = call_expr
            .children()
            .find(|n| n.kind() == SyntaxKind::ARG_LIST)
            .map_or(0, |args| Self::count_arguments(&args));

        Some(MethodId::new(class_name, method_name, arg_count))
    }

    /// Count parameters in PARAM_LIST node
    fn count_parameters(params_node: &hyperion_parser::SyntaxNode) -> usize {
        params_node
            .children()
            .filter(|n| n.kind() == SyntaxKind::PARAM)
            .count()
    }

    /// Count arguments in ARG_LIST node
    fn count_arguments(args_node: &hyperion_parser::SyntaxNode) -> usize {
        // Count top-level expression children (each is an argument)
        // Args are separated by COMMA tokens, so: count = num_expressions
        // OR: count = num_commas + 1 (if there are expressions)

        let comma_count = args_node.children_with_tokens()
            .filter(|child| {
                matches!(child, rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA)
            })
            .count();

        // If there are commas, there are comma_count + 1 arguments
        // If there are no commas but there are children (nodes), there's 1 argument
        // If there are no children at all, there are 0 arguments
        if comma_count > 0 {
            comma_count + 1
        } else {
            usize::from(args_node.children().next().is_some())
        }
    }

    /// Extract return type from method declaration
    fn extract_return_type(node: &hyperion_parser::SyntaxNode) -> Option<String> {
        // Look for type nodes before the method name
        for child in node.children() {
            let kind = child.kind();
            if kind == SyntaxKind::PRIMITIVE_TYPE
                || kind == SyntaxKind::CLASS_TYPE
                || kind == SyntaxKind::ARRAY_TYPE
                || kind == SyntaxKind::VAR_TYPE
            {
                return Some(child.text().to_string());
            }
            // Check for void keyword
            if kind == SyntaxKind::VOID_KW {
                return Some("void".to_string());
            }
        }
        None
    }

    /// Add an edge to the call graph (caller -> callee)
    fn add_edge(&mut self, caller: MethodId, callee: MethodId) {
        self.calls
            .entry(caller.clone())
            .or_default()
            .insert(callee.clone());

        self.called_by.entry(callee).or_default().insert(caller);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser, SyntaxNode};

    fn parse_java(source: &str) -> SyntaxNode {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green_node)
    }

    #[test]
    fn test_simple_method_call() {
        let source = r#"
            class TestClass {
                public void caller() {
                    callee();
                }

                private void callee() {
                }
            }
        "#;

        let tree = parse_java(source);
        let call_graph = CallGraph::build(&tree, "TestClass");

        let caller_id = MethodId::new("TestClass", "caller", 0);
        let callee_id = MethodId::new("TestClass", "callee", 0);

        // Verify methods are registered
        assert!(call_graph.contains_method(&caller_id));
        assert!(call_graph.contains_method(&callee_id));

        // Verify call edge
        let callees = call_graph.get_callees(&caller_id);
        assert_eq!(callees.len(), 1);
        assert!(callees.contains(&callee_id));

        // Verify reverse edge
        let callers = call_graph.get_callers(&callee_id);
        assert_eq!(callers.len(), 1);
        assert!(callers.contains(&caller_id));
    }

    #[test]
    fn test_multiple_calls_in_chain() {
        let source = r#"
            class TestClass {
                public void a() {
                    b();
                }

                private void b() {
                    c();
                }

                private void c() {
                }
            }
        "#;

        let tree = parse_java(source);
        let call_graph = CallGraph::build(&tree, "TestClass");

        let id_a = MethodId::new("TestClass", "a", 0);
        let id_b = MethodId::new("TestClass", "b", 0);
        let id_c = MethodId::new("TestClass", "c", 0);

        // Verify a -> b
        let callees_a = call_graph.get_callees(&id_a);
        assert_eq!(callees_a.len(), 1);
        assert!(callees_a.contains(&id_b));

        // Verify b -> c
        let callees_b = call_graph.get_callees(&id_b);
        assert_eq!(callees_b.len(), 1);
        assert!(callees_b.contains(&id_c));

        // Verify c calls nothing
        let callees_c = call_graph.get_callees(&id_c);
        assert_eq!(callees_c.len(), 0);
    }

    #[test]
    fn test_method_with_no_calls() {
        let source = r#"
            class TestClass {
                public void standalone() {
                    int x = 5;
                    x = x + 1;
                }
            }
        "#;

        let tree = parse_java(source);
        let call_graph = CallGraph::build(&tree, "TestClass");

        let id = MethodId::new("TestClass", "standalone", 0);

        // Method should be registered
        assert!(call_graph.contains_method(&id));

        // But should have no calls
        let callees = call_graph.get_callees(&id);
        assert_eq!(callees.len(), 0);
    }

    #[test]
    fn test_recursive_method_call() {
        let source = r#"
            class TestClass {
                public void recursive(int n) {
                    if (n > 0) {
                        recursive(n - 1);
                    }
                }
            }
        "#;

        let tree = parse_java(source);
        let call_graph = CallGraph::build(&tree, "TestClass");

        let id = MethodId::new("TestClass", "recursive", 1);

        // Verify method calls itself
        let callees = call_graph.get_callees(&id);
        assert_eq!(callees.len(), 1);
        assert!(callees.contains(&id));

        // Verify it's listed as its own caller
        let callers = call_graph.get_callers(&id);
        assert_eq!(callers.len(), 1);
        assert!(callers.contains(&id));
    }

    #[test]
    fn test_method_with_parameters() {
        let source = r#"
            class TestClass {
                public void process(String input, int count) {
                    validate(input);
                }

                private void validate(String s) {
                }
            }
        "#;

        let tree = parse_java(source);
        let call_graph = CallGraph::build(&tree, "TestClass");

        let process_id = MethodId::new("TestClass", "process", 2);
        let validate_id = MethodId::new("TestClass", "validate", 1);

        // Verify methods with correct param counts
        assert!(call_graph.contains_method(&process_id));
        assert!(call_graph.contains_method(&validate_id));

        // Verify process calls validate
        let callees = call_graph.get_callees(&process_id);
        assert_eq!(callees.len(), 1);
        assert!(callees.contains(&validate_id));
    }

    #[test]
    fn test_method_info_metadata() {
        let source = r#"
            class TestClass {
                public String getName() {
                    return "test";
                }

                private void setName(String name) {
                }
            }
        "#;

        let tree = parse_java(source);
        let call_graph = CallGraph::build(&tree, "TestClass");

        let get_id = MethodId::new("TestClass", "getName", 0);
        let set_id = MethodId::new("TestClass", "setName", 1);

        // Verify getName has return type
        let get_info = call_graph.get_method(&get_id).unwrap();
        assert_eq!(get_info.param_count, 0);
        assert!(get_info.return_type.is_some());

        // Verify setName has parameter
        let set_info = call_graph.get_method(&set_id).unwrap();
        assert_eq!(set_info.param_count, 1);
    }

    #[test]
    fn test_multiple_callers_same_method() {
        let source = r#"
            class TestClass {
                public void method1() {
                    helper();
                }

                public void method2() {
                    helper();
                }

                private void helper() {
                }
            }
        "#;

        let tree = parse_java(source);
        let call_graph = CallGraph::build(&tree, "TestClass");

        let method1_id = MethodId::new("TestClass", "method1", 0);
        let method2_id = MethodId::new("TestClass", "method2", 0);
        let helper_id = MethodId::new("TestClass", "helper", 0);

        // Verify both methods call helper
        assert!(call_graph.get_callees(&method1_id).contains(&helper_id));
        assert!(call_graph.get_callees(&method2_id).contains(&helper_id));

        // Verify helper has two callers
        let callers = call_graph.get_callers(&helper_id);
        assert_eq!(callers.len(), 2);
        assert!(callers.contains(&method1_id));
        assert!(callers.contains(&method2_id));
    }
}
