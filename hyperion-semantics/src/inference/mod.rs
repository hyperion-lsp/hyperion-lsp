// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Inference Engine for Hyperion Semantics
//!
//! This module provides **expression type inference** for Java code, enabling
//! features like smart code completion, accurate hover information, and
//! reliable go-to-definition for chained method calls.
//!
//! # Algorithm Approach
//!
//! The type inference engine uses a **bottom-up, syntax-directed** approach:
//!
//! 1. **Syntax-Directed:** Each expression kind (literal, identifier, method call, etc.)
//!    has a specialized inference method
//! 2. **Bottom-Up:** Infer leaf types first (literals, identifiers), then propagate upward
//!    through the AST (method calls, field access, etc.)
//! 3. **Context-Sensitive:** Some expressions (lambdas, constructor refs) use expected type
//!    from context for inference
//!
//! **Contrast with type checking:**
//! - Type inference: *Determine* the type of an expression (e.g., `x.foo()` → `String`)
//! - Type checking: *Verify* an expression matches expected type (e.g., `int x = "hello"` → error)
//!
//! This module focuses on inference. Type checking is handled by the LSP inspections layer.
//!
//! # Supported Expressions
//!
//! The type inference engine operates on the Rowan CST (Concrete Syntax Tree)
//! and uses the SymbolResolver to look up symbols and types. It handles:
//!
//! **Basic Types:**
//! - Literal type inference: `42` → `int`, `"hello"` → `String`, `true` → `boolean`
//! - Identifier type lookup: Variable declarations, fields, parameters
//! - Array access types: `array[0]` → element type (reduces dimensions by 1)
//! - Cast expressions: `(String) obj` → `String`
//!
//! **Object-Oriented:**
//! - Method call return types: `list.get(0)` → `T` (with canonical overload resolution and generic substitution)
//! - Field access types: `user.name` → `String`
//! - Constructor types: `new User()` → `User`
//! - `this` and `super` types: Enclosing class and superclass
//!
//! **Functional Programming (Java 8+):**
//! - Lambda expressions: `x -> x.toUpperCase()` → inferred from expected type
//! - Method references: `String::toUpperCase` → return type from method signature
//! - Constructor references: `ArrayList::new` → `ArrayList<T>` (with generic inference)
//!
//! **Control Flow:**
//! - Ternary expressions: `cond ? "yes" : "no"` → LUB of both branches
//! - Switch expressions (Java 12+): `switch (x) { case 1 -> "one"; case 2 -> "two"; }` → LUB of all arms
//! - Binary expressions: `1 + 2` → `int`, `"hello" + "world"` → `String`
//!
//! **Generics:**
//! - Type argument substitution: `List<String>.get(0)` → `String`
//! - Nested generics: `Map<String, List<Integer>>` → parse and track type arguments
//! - Diamond operator: `new ArrayList<>()` → infer from context
//!
//! # Architecture
//!
//! ```text
//! TypeInferenceEngine
//! ├── infer_expression() - Main entry point (dispatches by syntax kind)
//! ├── Literals
//! │   └── infer_literal() - Primitive and string literals
//! ├── Identifiers
//! │   └── infer_identifier() - Variable, field, parameter lookup
//! ├── Method Calls
//! │   ├── infer_method_call() - Method return type resolution
//! │   ├── infer_argument_types() - Infer types for overload resolution
//! │   └── call_resolution::resolve_call() - Canonical overload and conversion selection
//! ├── Field Access
//! │   └── infer_field_access() - Field type resolution
//! ├── Arrays
//! │   └── infer_array_access() - Array element type
//! ├── Operators
//! │   ├── infer_binary_expr() - Arithmetic and logical operations
//! │   └── infer_unary_expr() - Unary +, -, !, ~
//! ├── Control Flow
//! │   ├── infer_ternary_expr() - Conditional expressions
//! │   ├── infer_switch_expression() - Switch expressions (Java 12+)
//! │   └── lub_two_types() - Least upper bound for branch types
//! ├── Functional
//! │   ├── infer_lambda_with_expected_type() - Lambda type from SAM
//! │   ├── infer_method_reference() - Method reference return type
//! │   └── infer_constructor_reference() - Constructor reference type
//! └── Utilities
//!     ├── parse_type_string() - Parse type strings from WorkspaceIndex
//!     ├── parse_type_arguments() - Parse generic type arguments
//!     └── find_common_supertype() - LUB of multiple types
//! ```
//!
//! # Generics and Type Substitution
//!
//! Generic types are handled by:
//! 1. Parsing type strings from WorkspaceIndex: `"List<String>"` → `ClassType { name: "List", type_arguments: [String] }`
//! 2. Tracking type arguments through method calls: `list.get(0)` → substitute `T` with `String`
//! 3. Inferring type arguments from context: `new ArrayList<>()` → diamond operator inference
//!
//! **Current limitations:**
//! - No full type parameter resolution (e.g., `<T extends Comparable<T>>`)
//! - No wildcard bounds (e.g., `? extends Number`, `? super Integer`)
//! - Method-level type-variable unification from lambda bodies remains limited
//!
//! Callers must treat these cases as conservative or unknown results rather
//! than assuming javac-equivalent inference.
//!
//! # Subtyping and Type Bounds
//!
//! The engine uses **least upper bound (LUB)** to find common supertypes:
//! - Primitives: Numeric promotion rules (JLS §5.6)
//! - Reference types: Check assignability via `Type::is_assignable_from()`
//! - Arrays: Element type LUB with same dimensions
//! - Unrelated types: Fallback to `Object`
//!
//! **Example:**
//! ```java
//! String result = switch (x) {
//!     case 1 -> "one";   // String
//!     case 2 -> "two";   // String
//!     default -> null;   // null
//! };
//! // LUB(String, String, null) = String
//! ```
//!
//! # Known Edge Cases
//!
//! 1. **Lambda without expected type:** Returns `Type::Unknown` (needs context detection)
//! 2. **Generic method inference:** Receiver type variables propagate through calls, while
//!    method-level variables are only partially inferred from arguments and target context
//! 3. **Mixed boolean/numeric:** `switch { case 1 -> true; case 2 -> 42; }` → boxes to `Object` (legal but likely error)
//!
//! # Links to JLS
//!
//! - [JLS §15.12 - Method Invocation Expressions](https://docs.oracle.com/javase/specs/jls/se21/html/jls-15.html#jls-15.12)
//! - [JLS §15.25 - Conditional Operator](https://docs.oracle.com/javase/specs/jls/se21/html/jls-15.html#jls-15.25)
//! - [JLS §15.28 - Switch Expressions](https://docs.oracle.com/javase/specs/jls/se21/html/jls-15.html#jls-15.28)
//! - [JLS §5.6 - Numeric Contexts](https://docs.oracle.com/javase/specs/jls/se21/html/jls-5.html#jls-5.6)
//! - [JLS §9.8 - Functional Interfaces](https://docs.oracle.com/javase/specs/jls/se21/html/jls-9.html#jls-9.8)
//!
//! # Example
//!
//! ```rust,ignore
//! use hyperion_semantics::inference::TypeInferenceEngine;
//! use hyperion_semantics::resolver::SymbolResolver;
//!
//! let engine = TypeInferenceEngine::new(workspace_index, file_id);
//! let resolver = SymbolResolver::new(/* ... */);
//!
//! // Infer type of expression at cursor position
//! let expr_node = find_expression_at_cursor(syntax_tree, cursor_pos);
//! let inferred_type = engine.infer_expression(&expr_node, &resolver, None);
//!
//! match inferred_type {
//!     Type::Class(class_type) => {
//!         println!("Type: {}", class_type.qualified_name());
//!     }
//!     Type::Primitive(prim) => {
//!         println!("Primitive type: {:?}", prim);
//!     }
//!     Type::Array { element_type, dimensions } => {
//!         println!("Array: {:?}[]{}", element_type, "[]".repeat((dimensions - 1) as usize));
//!     }
//!     _ => println!("Other type"),
//! }
//! ```

mod expr_wrappers;
mod expressions;
mod generics;
mod lambda;
#[cfg(test)]
mod sam_tests;
mod visitor;

use crate::resolver::{ResolverBuilder, SymbolResolver};
use crate::symbol::Type;
use hyperion_parser::syntax::SyntaxNode;
use hyperion_workspace::{FileId, WorkspaceIndex};
use rowan::TextRange;
use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    sync::Arc,
};

/// Type inference engine for Java expressions
///
/// This engine analyzes Java expressions and determines their types using
/// the symbol table and workspace index. It's the foundation for features
/// like smart completion, hover information, and navigation.
pub struct TypeInferenceEngine {
    /// Shares declaration type extraction (imports, lexical scope and bounds)
    /// for explicit cast and construction types. Its caches live for this tree.
    syntax_type_resolver: ResolverBuilder,
    /// Workspace index for symbol lookup
    workspace_index: Arc<WorkspaceIndex>,
    /// Accepted for API compatibility. Nothing reads it yet: import-aware resolution
    /// goes through `syntax_type_resolver`, which already holds the file.
    _file_id: FileId,
    /// Per-class member lookup cache. Avoids redundant workspace queries when
    /// the same receiver type appears in multiple expressions (e.g., 100 calls
    /// to `list.add()` reuse the same member list).
    member_cache: RefCell<HashMap<String, Vec<Arc<hyperion_workspace::Symbol>>>>,
    /// Expression results for this immutable syntax tree. Parent expressions
    /// recursively infer their children before the semantic walk reaches those
    /// children directly, so range caching prevents duplicate subtree work.
    expression_cache: RefCell<HashMap<TextRange, Type>>,
    /// Effective field types repeat heavily in ordinary source (`this.field`
    /// in a loop or generated method). Cache receiver-relative projection for
    /// the lifetime of this request-local engine.
    field_type_cache: RefCell<HashMap<(String, String), Option<Type>>>,
    /// Lexical types for implicitly typed lambda parameters while their body
    /// contributes constraints to a generic invocation.
    lambda_parameters: RefCell<Vec<HashMap<String, Type>>>,
    /// Imports from the immutable source tree analyzed by this engine.
    syntax_imports: OnceCell<hyperion_workspace::ImportMap>,
}

impl TypeInferenceEngine {
    /// Infer a lambda body under one overload candidate's functional inputs.
    /// The parameter bindings are scoped to this call and never shared with a
    /// competing overload.
    pub fn infer_lambda_with_parameters(
        &self,
        lambda: &SyntaxNode,
        resolver: &crate::resolver::SymbolResolver,
        parameter_types: &[Type],
    ) -> crate::call_resolution::LambdaReturnEvidence {
        lambda::infer_lambda_with_parameters(self, lambda, resolver, parameter_types)
    }

    /// Retain explicit lambda input types for functional target inference.
    pub fn lambda_argument(
        &self,
        argument_index: usize,
        lambda: &SyntaxNode,
    ) -> crate::call_resolution::LambdaArgument {
        let explicit_parameter_types = lambda
            .children()
            .filter(|child| child.kind() == hyperion_parser::SyntaxKind::PARAM)
            .map(|parameter| lambda::explicit_parameter_type(self, &parameter))
            .collect::<Option<Vec<_>>>()
            .filter(|parameters| !parameters.is_empty());
        crate::call_resolution::LambdaArgument {
            argument_index,
            explicit_parameter_types,
        }
    }

    /// Create a new type inference engine
    pub fn new(workspace_index: Arc<WorkspaceIndex>, file_id: FileId) -> Self {
        Self {
            syntax_type_resolver: ResolverBuilder::with_workspace(
                file_id,
                Arc::clone(&workspace_index),
            ),
            workspace_index,
            _file_id: file_id,
            member_cache: RefCell::new(HashMap::new()),
            expression_cache: RefCell::new(HashMap::new()),
            field_type_cache: RefCell::new(HashMap::new()),
            lambda_parameters: RefCell::new(Vec::new()),
            syntax_imports: OnceCell::new(),
        }
    }

    /// Access the workspace index this engine was built from. Inference
    /// submodules use this to drive type-parameter substitution.
    pub(super) fn workspace_index(&self) -> &WorkspaceIndex {
        &self.workspace_index
    }

    pub(super) fn imports_for(&self, root: &SyntaxNode) -> &hyperion_workspace::ImportMap {
        self.syntax_imports
            .get_or_init(|| crate::utils::build_imports_from_syntax(root))
    }

    /// Get class members with caching. Returns a clone of the cached member list.
    pub(super) fn get_class_members_cached(
        &self,
        class_fqn: &str,
    ) -> Vec<Arc<hyperion_workspace::Symbol>> {
        {
            let cache = self.member_cache.borrow();
            if let Some(members) = cache.get(class_fqn) {
                return members.clone();
            }
        }
        let members = self
            .workspace_index
            .get_class_members_with_inheritance(class_fqn);
        self.member_cache
            .borrow_mut()
            .insert(class_fqn.to_string(), members.clone());
        members
    }

    pub(super) fn cached_expression_type(&self, range: TextRange) -> Option<Type> {
        self.expression_cache.borrow().get(&range).cloned()
    }

    pub(super) fn cache_expression_type(&self, range: TextRange, inferred: Type) {
        self.expression_cache.borrow_mut().insert(range, inferred);
    }

    pub(super) fn resolve_field_type_cached(
        &self,
        resolver: &SymbolResolver,
        receiver_type: &Type,
        field_name: &str,
    ) -> Option<Type> {
        let key = (receiver_type.to_string(), field_name.to_owned());
        if let Some(cached) = self.field_type_cache.borrow().get(&key) {
            return cached.clone();
        }
        let resolved = resolver.resolve_field_type(receiver_type, field_name);
        self.field_type_cache
            .borrow_mut()
            .insert(key, resolved.clone());
        resolved
    }

    /// Infer the type of an expression node
    ///
    /// This is the main entry point for type inference. It dispatches to
    /// specialized methods based on the expression kind.
    ///
    /// # Arguments
    ///
    /// * `node` - The syntax node representing the expression
    /// * `resolver` - Symbol resolver for looking up declarations
    ///
    /// # Returns
    ///
    /// The inferred type, or Type::Unknown if inference fails
    #[must_use]
    pub fn infer_expression(
        &self,
        node: &SyntaxNode,
        resolver: &SymbolResolver,
        expected_type: Option<&Type>,
    ) -> Type {
        // Dispatch to expressions module
        expressions::infer_expression(self, node, resolver, expected_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{ClassType, PrimitiveType, Type};
    use std::sync::Arc;
    use tempfile::TempDir;

    fn create_test_workspace() -> (TempDir, Arc<WorkspaceIndex>) {
        let temp_dir = TempDir::new().unwrap();
        let workspace = Arc::new(WorkspaceIndex::new(temp_dir.path().to_path_buf()));
        (temp_dir, workspace)
    }

    #[test]
    fn test_parse_primitive_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        assert_eq!(
            engine.parse_type_string("int"),
            Some(Type::Primitive(PrimitiveType::Int))
        );
        assert_eq!(
            engine.parse_type_string("boolean"),
            Some(Type::Primitive(PrimitiveType::Boolean))
        );
        assert_eq!(
            engine.parse_type_string("long"),
            Some(Type::Primitive(PrimitiveType::Long))
        );
        assert_eq!(
            engine.parse_type_string("double"),
            Some(Type::Primitive(PrimitiveType::Double))
        );
    }

    #[test]
    fn test_parse_reference_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let result = engine
            .parse_type_string("String")
            .expect("Should parse String");
        match result {
            Type::Class(class_type) => {
                assert_eq!(&class_type.name, "String");
            }
            _ => panic!("Expected Class type for String"),
        }
    }

    #[test]
    fn test_parse_void_type() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        assert_eq!(engine.parse_type_string("void"), Some(Type::Void));
    }

    #[test]
    fn test_type_inference_engine_creation() {
        let (_temp, workspace) = create_test_workspace();
        let _engine = TypeInferenceEngine::new(workspace, FileId::new(1));
    }

    #[test]
    fn test_parse_array_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        // Simple array
        let result = engine
            .parse_type_string("String[]")
            .expect("Should parse String[]");
        match result {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 1);
                match *element_type {
                    Type::Class(ref c) => assert_eq!(&*c.name, "String"),
                    _ => panic!("Expected Class element type"),
                }
            }
            _ => panic!("Expected Array type for String[]"),
        }

        // Primitive array
        let result = engine
            .parse_type_string("int[]")
            .expect("Should parse int[]");
        match result {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 1);
                assert_eq!(*element_type, Type::Primitive(PrimitiveType::Int));
            }
            _ => panic!("Expected Array type for int[]"),
        }

        // Multi-dimensional array
        let result = engine
            .parse_type_string("double[][]")
            .expect("Should parse double[][]");
        match result {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 2);
                assert_eq!(*element_type, Type::Primitive(PrimitiveType::Double));
            }
            _ => panic!("Expected Array type for double[][]"),
        }
    }

    #[test]
    fn test_parse_generic_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        // Simple generic
        let result = engine
            .parse_type_string("List<String>")
            .expect("Should parse List<String>");
        match result {
            Type::Class(ref c) => {
                assert_eq!(&*c.name, "List");
                assert_eq!(c.type_arguments.len(), 1);
                match &c.type_arguments[0] {
                    Type::Class(inner) => assert_eq!(&inner.name, "String"),
                    _ => panic!("Expected Class type argument"),
                }
            }
            _ => panic!("Expected Class type for List<String>"),
        }

        // Multiple type arguments
        let result = engine
            .parse_type_string("Map<String, Integer>")
            .expect("Should parse Map<String, Integer>");
        match result {
            Type::Class(ref c) => {
                assert_eq!(&*c.name, "Map");
                assert_eq!(c.type_arguments.len(), 2);
                match &c.type_arguments[0] {
                    Type::Class(inner) => assert_eq!(&inner.name, "String"),
                    _ => panic!("Expected String as first type argument"),
                }
                match &c.type_arguments[1] {
                    Type::Class(inner) => assert_eq!(&inner.name, "Integer"),
                    _ => panic!("Expected Integer as second type argument"),
                }
            }
            _ => panic!("Expected Class type for Map<String, Integer>"),
        }
    }

    #[test]
    fn test_parse_nested_generic_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let result = engine
            .parse_type_string("List<Map<String, Integer>>")
            .expect("Should parse nested generic");
        match result {
            Type::Class(ref c) => {
                assert_eq!(&*c.name, "List");
                assert_eq!(c.type_arguments.len(), 1);
                match &c.type_arguments[0] {
                    Type::Class(inner) => {
                        assert_eq!(&inner.name, "Map");
                        assert_eq!(inner.type_arguments.len(), 2);
                    }
                    _ => panic!("Expected Map as type argument"),
                }
            }
            _ => panic!("Expected Class type for nested generic"),
        }
    }

    #[test]
    fn test_parse_fqn_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let result = engine
            .parse_type_string("java.util.List")
            .expect("Should parse FQN");
        match result {
            Type::Class(ref c) => {
                assert_eq!(&*c.name, "List");
                assert_eq!(c.package.as_deref(), Some("java.util".into()));
            }
            _ => panic!("Expected Class type for FQN"),
        }
    }

    #[test]
    fn test_lub_same_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let string_type = Type::Class(ClassType::string());
        let result = expressions::lub_two_types(&engine, &string_type, &string_type);
        assert_eq!(result, string_type);
    }

    #[test]
    fn test_lub_primitives_numeric_promotion() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let int_type = Type::Primitive(PrimitiveType::Int);
        let long_type = Type::Primitive(PrimitiveType::Long);
        let result = expressions::lub_two_types(&engine, &int_type, &long_type);
        assert_eq!(result, Type::Primitive(PrimitiveType::Long));

        let double_type = Type::Primitive(PrimitiveType::Double);
        let result = expressions::lub_two_types(&engine, &int_type, &double_type);
        assert_eq!(result, Type::Primitive(PrimitiveType::Double));

        let byte_type = Type::Primitive(PrimitiveType::Byte);
        let short_type = Type::Primitive(PrimitiveType::Short);
        let result = expressions::lub_two_types(&engine, &byte_type, &short_type);
        assert_eq!(result, Type::Primitive(PrimitiveType::Int));
    }

    #[test]
    fn test_lub_with_null() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let string_type = Type::Class(ClassType::string());
        let null_type = Type::Null;

        let result = expressions::lub_two_types(&engine, &string_type, &null_type);
        assert_eq!(result, string_type);

        let result = expressions::lub_two_types(&engine, &null_type, &string_type);
        assert_eq!(result, string_type);
    }

    #[test]
    fn test_lub_with_unknown() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let string_type = Type::Class(ClassType::string());
        let unknown = Type::Unknown;

        // A branch of unknown type makes the join unknown: `c ? <unknown> : "s"`
        // is only a String when the unknown branch is one, and presenting the
        // guess as fact produced false type errors downstream.
        let result = expressions::lub_two_types(&engine, &string_type, &unknown);
        assert_eq!(result, Type::Unknown);

        let result = expressions::lub_two_types(&engine, &unknown, &string_type);
        assert_eq!(result, Type::Unknown);
    }

    #[test]
    fn test_lub_numeric_wrappers() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let integer = Type::Class(ClassType::new("Integer".into(), Some("java.lang".into())));
        let double = Type::Class(ClassType::new("Double".into(), Some("java.lang".into())));

        let result = expressions::lub_two_types(&engine, &integer, &double);
        match result {
            Type::Class(c) => {
                assert_eq!(&*c.name, "Number");
                assert_eq!(c.package.as_deref(), Some("java.lang".into()));
            }
            _ => panic!("Expected Number type"),
        }
    }

    #[test]
    fn test_lub_unrelated_types_fallback_to_object() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let string_type = Type::Class(ClassType::string());
        let custom = Type::Class(ClassType::new("MyClass".to_string(), None));

        let result = expressions::lub_two_types(&engine, &string_type, &custom);
        match result {
            Type::Class(c) => {
                assert_eq!(&*c.name, "Object");
            }
            _ => panic!("Expected Object type"),
        }
    }

    #[test]
    fn test_lub_arrays_same_dimension() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let string_array = Type::Array {
            element_type: Box::new(Type::Class(ClassType::string())),
            dimensions: 1,
        };
        let object_array = Type::Array {
            element_type: Box::new(Type::Class(ClassType::object())),
            dimensions: 1,
        };

        let result = expressions::lub_two_types(&engine, &string_array, &object_array);
        match result {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 1);
                match *element_type {
                    Type::Class(c) => assert_eq!(&*c.name, "Object"),
                    _ => panic!("Expected Object element type"),
                }
            }
            _ => panic!("Expected array type"),
        }
    }

    #[test]
    fn test_find_common_supertype_multiple() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let types = vec![
            Type::Primitive(PrimitiveType::Int),
            Type::Primitive(PrimitiveType::Long),
            Type::Primitive(PrimitiveType::Double),
        ];

        let result = expressions::find_common_supertype(&engine, &types);
        assert_eq!(result, Type::Primitive(PrimitiveType::Double));
    }

    #[test]
    fn test_find_common_supertype_strings() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let types = vec![
            Type::Class(ClassType::string()),
            Type::Class(ClassType::string()),
            Type::Class(ClassType::string()),
        ];

        let result = expressions::find_common_supertype(&engine, &types);
        assert_eq!(result, Type::Class(ClassType::string()));
    }

    #[test]
    fn test_find_common_supertype_with_null() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let types = vec![
            Type::Class(ClassType::string()),
            Type::Null,
            Type::Class(ClassType::string()),
        ];

        let result = expressions::find_common_supertype(&engine, &types);
        assert_eq!(result, Type::Class(ClassType::string()));
    }

    #[test]
    fn test_find_common_supertype_empty() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let result = expressions::find_common_supertype(&engine, &[]);
        assert_eq!(result, Type::Unknown);
    }

    #[test]
    fn test_find_common_supertype_single() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let types = vec![Type::Primitive(PrimitiveType::Int)];
        let result = expressions::find_common_supertype(&engine, &types);
        assert_eq!(result, Type::Primitive(PrimitiveType::Int));
    }

    #[test]
    fn test_parse_type_string_edge_cases() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        // FIX: All malformed input should return None

        let result = engine.parse_type_string(".");
        assert!(result.is_none(), "Single dot should return None");

        let result = engine.parse_type_string(".User");
        assert!(result.is_none(), "Leading dot should return None");

        let result = engine.parse_type_string("User.");
        assert!(result.is_none(), "Trailing dot should return None");

        let result = engine.parse_type_string("");
        assert!(result.is_none(), "Empty string should return None");

        let result = engine.parse_type_string("   ");
        assert!(result.is_none(), "Whitespace-only should return None");

        let result = engine.parse_type_string("com..example.Foo");
        assert!(result.is_none(), "Empty package segment should return None");
    }

    #[test]
    fn test_parse_malformed_generic_types() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        let result = engine.parse_type_string("<String>");
        assert!(result.is_none(), "Empty base name should return None");

        let result = engine.parse_type_string("List<String");
        assert!(result.is_none(), "Unmatched brackets should return None");

        // Diamond operator is VALID
        let result = engine
            .parse_type_string("List<>")
            .expect("Diamond operator should parse");
        match &result {
            Type::Class(c) => {
                assert_eq!(&*c.name, "List");
                assert!(c.is_diamond, "List<> should be marked as diamond operator");
            }
            _ => panic!("Expected Class type for List<> (valid diamond operator)"),
        }

        // Nested generics are valid
        let result = engine
            .parse_type_string("Map<String, List<Integer>>")
            .expect("Nested generics should parse");
        match &result {
            Type::Class(c) => {
                assert_eq!(&*c.name, "Map");
                assert_eq!(c.type_arguments.len(), 2);
            }
            _ => panic!("Expected Class type for nested generics"),
        }
    }

    #[test]
    fn test_parse_array_type_edge_cases() {
        let (_temp, workspace) = create_test_workspace();
        let engine = TypeInferenceEngine::new(workspace, FileId::new(1));

        // Multi-dimensional arrays
        let result = engine
            .parse_type_string("int[][]")
            .expect("Should parse int[][]");
        match result {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 2, "Should have 2 dimensions for int[][]");
                assert_eq!(*element_type, Type::Primitive(PrimitiveType::Int));
            }
            _ => panic!("Expected Array type for int[][]"),
        }

        // Generic array type
        let result = engine
            .parse_type_string("List<String>[]")
            .expect("Should parse List<String>[]");
        match result {
            Type::Array {
                element_type,
                dimensions,
            } => {
                assert_eq!(dimensions, 1);
                match *element_type {
                    Type::Class(c) => {
                        assert_eq!(&*c.name, "List");
                        assert_eq!(c.type_arguments.len(), 1);
                    }
                    _ => panic!("Expected Class element type"),
                }
            }
            _ => panic!("Expected Array type for List<String>[]"),
        }

        // FIX: Empty array type should return None
        let result = engine.parse_type_string("[]");
        assert!(result.is_none(), "Empty array type should return None");
    }
}
