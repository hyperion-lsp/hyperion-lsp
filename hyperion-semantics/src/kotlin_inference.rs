// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Inference Engine for Kotlin
//!
//! Provides expression type inference for Kotlin code, enabling smart navigation,
//! completion, and hover information for method chains, property access, and more.
//!
//! # Design
//!
//! Similar to Java's TypeInferenceEngine but adapted for Kotlin syntax:
//! - Property access (user.name) instead of getter methods
//! - Null-safe access (user?.name)
//! - Extension functions
//! - Lambda expressions with receivers
//!
//! # Example
//!
//! ```kotlin
//! val user = getUser()
//! user.name  // Infer type of 'user' → User, then 'name' → String
//! ```

use crate::resolver::SymbolResolver;
use crate::symbol::{ClassType, PrimitiveType, Type};
use hyperion_kotlin::{SyntaxKind as KtKind, SyntaxNode as KtSyntaxNode};
use hyperion_workspace::{
    DeclarationNamespace, FileId, ProjectedCallableParameter, ProjectedEffectiveMethod,
    SymbolTarget, TypeParameterInfo, WorkspaceIndex,
};
use lsp_types::SymbolKind;
use rowan::NodeOrToken;
use std::sync::Arc;
use tracing::debug;

thread_local! {
    static INFERENCE_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Maximum nesting of `infer_expression` on one thread. Real code is nowhere
/// near this; it only exists to turn runaway recursion into `Unknown`.
const MAX_INFERENCE_DEPTH: u32 = 200;

struct InferenceDepthGuard;

impl InferenceDepthGuard {
    fn enter() -> Option<Self> {
        INFERENCE_DEPTH.with(|depth| {
            if depth.get() >= MAX_INFERENCE_DEPTH {
                None
            } else {
                depth.set(depth.get() + 1);
                Some(Self)
            }
        })
    }
}

impl Drop for InferenceDepthGuard {
    fn drop(&mut self) {
        INFERENCE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Reads the types of a call's value arguments when they are first needed,
/// since only a member whose overloads disagree about their type needs them.
/// None for a member access that is not a call.
type CallArguments<'a> = Option<&'a mut dyn FnMut() -> Option<Vec<Type>>>;

/// How an inherited method is reached, as
/// [`KotlinTypeInferenceEngine::inherited_method_type`] reads it.
enum InheritedCall<'t, 'c> {
    /// The Java getter behind a property access, which takes no parameters.
    Getter,
    /// A call, writing `type_arguments` for the method's own type parameters
    /// (none when it writes none) and passing the value arguments
    /// `call_arguments` reads.
    Call {
        type_arguments: &'t [Type],
        call_arguments: CallArguments<'c>,
    },
}

/// Whether an argument can be passed to a parameter, as
/// [`KotlinTypeInferenceEngine::argument_fit`] tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgumentFit {
    Fits,
    Mismatch,
    Unknown,
}

/// Type inference engine for Kotlin expressions
pub struct KotlinTypeInferenceEngine {
    /// Workspace index for cross-file symbol lookup
    workspace_index: Arc<WorkspaceIndex>,
    /// Current file being analyzed (reserved for import-aware resolution)
    file_id: FileId,
    /// Memoised `callee -> lambda receiver type` for DSL builder functions.
    ///
    /// Resolving these re-parses the declaring file, and the semantic walker
    /// asks the same question for every call inside a builder block.
    dsl_receiver_cache: std::cell::RefCell<std::collections::HashMap<String, Option<Type>>>,
}

impl KotlinTypeInferenceEngine {
    /// Create a new Kotlin type inference engine
    pub fn new(workspace_index: Arc<WorkspaceIndex>, file_id: FileId) -> Self {
        Self {
            workspace_index,
            file_id,
            dsl_receiver_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// Infer the type of a Kotlin expression
    ///
    /// Main entry point - dispatches to specialized methods based on expression kind.
    pub fn infer_expression(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        // Inference is mutually recursive (call -> receiver -> lambda -> call).
        // A cycle we did not anticipate must degrade to `Unknown`, never
        // overflow the stack and abort the whole language server.
        let Some(_guard) = InferenceDepthGuard::enter() else {
            return Type::Unknown;
        };
        self.infer_expression_inner(node, resolver)
    }

    fn infer_expression_inner(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let kind = node.kind();
        debug!("KT: Inferring type for node kind: {:?}", kind);

        // Literal kinds, on the rare occasion they appear as nodes rather than
        // as the token inside a LITERAL_EXPR.
        if let Some(ty) = literal_token_type(kind) {
            return ty;
        }

        match kind {
            // Identifiers - look up in symbol table
            KtKind::IDENT => self.infer_identifier(node, resolver),

            // Member access (property or method call)
            KtKind::MEMBER_ACCESS_EXPR => self.infer_member_access(node, resolver),

            // Safe access (null-safe navigation)
            KtKind::SAFE_ACCESS_EXPR => self.infer_safe_access(node, resolver),

            // Call expression (function call)
            KtKind::CALL_EXPR => self.infer_call(node, resolver),

            // Binary expressions
            KtKind::BINARY_EXPR => self.infer_binary_expr(node, resolver),

            // Unary expressions
            KtKind::UNARY_EXPR => self.infer_unary_expr(node, resolver),

            // Parenthesized expression
            KtKind::PAREN_EXPR => node
                .children()
                .find_map(|child| {
                    if matches!(child.kind(), KtKind::L_PAREN | KtKind::R_PAREN) {
                        None
                    } else {
                        Some(self.infer_expression(&child, resolver))
                    }
                })
                .unwrap_or(Type::Unknown),

            // If expression
            KtKind::IF_EXPR => self.infer_if_expr(node, resolver),

            // Literal wrapper node: `LITERAL_EXPR > <literal token>`.
            // The parser never produces bare literal *nodes*, so without this
            // arm every literal inferred as Unknown.
            KtKind::LITERAL_EXPR => self.infer_literal_expr(node, resolver),

            // when-expression: least upper bound of the branch result types
            KtKind::WHEN_EXPR => self.infer_when_expr(node, resolver),

            // A single `value -> result` arm; type is the arm's result
            KtKind::WHEN_ENTRY => self.infer_when_entry_result(node, resolver),

            // try/catch as an expression: LUB of try body and catch bodies
            KtKind::TRY_EXPR => self.infer_try_expr(node, resolver),

            // Blocks evaluate to their last expression
            KtKind::BLOCK => self.infer_block(node, resolver),

            // Statement wrapper - transparent
            KtKind::EXPR_STMT => self.infer_first_child_expr(node, resolver),

            // Lambda: function type, parameters taken from the expected-type context
            KtKind::LAMBDA_EXPR => self.infer_lambda_expr(node, resolver),

            _ => {
                debug!("KT: Unknown expression kind for type inference: {:?}", kind);
                Type::Unknown
            }
        }
    }

    /// Infer the type of an identifier-like token in expression position.
    ///
    /// Callers that start from a cursor position (hover, signature help) have a
    /// token rather than a node, and the Kotlin parser does not wrap `it`,
    /// `this` or plain identifier operands in expression nodes, so there is
    /// often no node to pass to [`Self::infer_expression`].
    pub fn infer_token_expression(
        &self,
        token: &hyperion_kotlin::SyntaxToken,
        resolver: &mut SymbolResolver,
    ) -> Type {
        self.infer_token(token, resolver)
    }

    /// Infer a node or token uniformly.
    ///
    /// The Kotlin parser leaves many operands as bare tokens (identifiers and
    /// literals are *not* wrapped in nodes in most positions), so every place
    /// that walks children has to be able to type a token as well as a node.
    fn infer_element(
        &self,
        element: &NodeOrToken<KtSyntaxNode, hyperion_kotlin::SyntaxToken>,
        resolver: &mut SymbolResolver,
    ) -> Type {
        match element {
            NodeOrToken::Node(n) => self.infer_expression(n, resolver),
            NodeOrToken::Token(t) => self.infer_token(t, resolver),
        }
    }

    /// Infer the type of a bare token used in expression position.
    fn infer_token(
        &self,
        token: &hyperion_kotlin::SyntaxToken,
        resolver: &mut SymbolResolver,
    ) -> Type {
        if let Some(ty) = literal_token_type(token.kind()) {
            return ty;
        }
        if is_kotlin_identifier_token(token) {
            let Some(parent) = token.parent() else {
                return Type::Unknown;
            };
            return self.infer_identifier_named(
                token.text(),
                &parent,
                token.text_range().start().into(),
                resolver,
            );
        }
        Type::Unknown
    }

    /// `LITERAL_EXPR` wraps a single literal token.
    fn infer_literal_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        for element in node.children_with_tokens() {
            let ty = self.infer_element(&element, resolver);
            if !matches!(ty, Type::Unknown) {
                return ty;
            }
        }
        Type::Unknown
    }

    /// First meaningful child element of a transparent wrapper node.
    fn infer_first_child_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        for element in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &element {
                if t.text().trim().is_empty() || is_structural_token(t.kind()) {
                    continue;
                }
            }
            let ty = self.infer_element(&element, resolver);
            if !matches!(ty, Type::Unknown) {
                return ty;
            }
        }
        Type::Unknown
    }

    /// A block evaluates to the type of its last expression.
    fn infer_block(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let mut last: Option<NodeOrToken<KtSyntaxNode, hyperion_kotlin::SyntaxToken>> = None;
        for element in node.children_with_tokens() {
            match &element {
                NodeOrToken::Token(t) => {
                    if t.text().trim().is_empty() || is_structural_token(t.kind()) {
                        continue;
                    }
                }
                NodeOrToken::Node(n) => {
                    // Declarations are not the block's value.
                    if matches!(
                        n.kind(),
                        KtKind::PROPERTY_DECL | KtKind::FUNCTION_DECL | KtKind::CLASS_DECL
                    ) {
                        continue;
                    }
                }
            }
            last = Some(element);
        }
        last.map_or(Type::Unknown, |element| {
            self.infer_element(&element, resolver)
        })
    }

    /// Infer a `when` expression as the least upper bound of its arm results.
    ///
    /// Subject bindings (`when (val x = e) { ... }`) are handled lazily: an
    /// identifier inside an arm walks up to the enclosing `WHEN_EXPR` and picks
    /// up the binding from there, so the binding is only visible inside the
    /// `when` (see `lookup_when_subject_binding`).
    fn infer_when_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let mut branch_types = Vec::new();
        for entry in node
            .children()
            .filter(|child| child.kind() == KtKind::WHEN_ENTRY)
        {
            let ty = self.infer_when_entry_result(&entry, resolver);
            if !matches!(ty, Type::Unknown) {
                branch_types.push(ty);
            }
        }

        let lub = least_upper_bound(&branch_types);
        debug!("KT: when-expression inferred as {:?}", lub);
        lub
    }

    /// The result expression of a `WHEN_ENTRY` is everything after the `->`.
    fn infer_when_entry_result(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let mut seen_arrow = false;
        for element in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &element {
                if t.kind() == KtKind::ARROW {
                    seen_arrow = true;
                    continue;
                }
            }
            if !seen_arrow {
                continue;
            }
            if let NodeOrToken::Token(t) = &element {
                if t.text().trim().is_empty() || is_structural_token(t.kind()) {
                    continue;
                }
            }
            let ty = self.infer_element(&element, resolver);
            if !matches!(ty, Type::Unknown) {
                return ty;
            }
        }
        Type::Unknown
    }

    /// `try { a } catch (e: E) { b } finally { }` is the LUB of `a` and `b`.
    ///
    /// The `finally` block never contributes a value, matching Kotlin semantics.
    fn infer_try_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let mut branch_types = Vec::new();

        for child in node.children() {
            match child.kind() {
                KtKind::BLOCK => {
                    let ty = self.infer_block(&child, resolver);
                    if !matches!(ty, Type::Unknown) {
                        branch_types.push(ty);
                    }
                }
                KtKind::CATCH_CLAUSE => {
                    if let Some(block) = child.children().find(|n| n.kind() == KtKind::BLOCK) {
                        let ty = self.infer_block(&block, resolver);
                        if !matches!(ty, Type::Unknown) {
                            branch_types.push(ty);
                        }
                    }
                }
                // FINALLY_CLAUSE contributes no value.
                _ => {}
            }
        }

        least_upper_bound(&branch_types)
    }

    /// Infer the type of an identifier (variable, parameter, property)
    fn infer_identifier(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let name = node.text().to_string();
        self.infer_identifier_named(&name, node, node.text_range().start().into(), resolver)
    }

    /// Infer the type of an identifier by name, using `context` for scope walking.
    ///
    /// `context` is the syntax node the identifier occurs in; it is used to find
    /// enclosing lambdas (for `it`/`this`) and enclosing `when` subject bindings.
    fn infer_identifier_named(
        &self,
        name: &str,
        context: &KtSyntaxNode,
        offset: u32,
        resolver: &mut SymbolResolver,
    ) -> Type {
        debug!("KT: Inferring identifier type: {}", name);

        // `when (val x = e)` subject bindings are only visible inside the when.
        if let Some(ty) = self.lookup_when_subject_binding(name, context, offset, resolver) {
            debug!("KT: '{}' resolved as when-subject binding: {:?}", name, ty);
            return ty;
        }

        // Explicitly declared lambda parameters: `{ a: Int -> ... }` / `{ a -> ... }`
        if let Some(ty) = self.lookup_lambda_parameter(name, context, resolver) {
            debug!("KT: '{}' resolved as lambda parameter: {:?}", name, ty);
            return ty;
        }

        // Special handling for implicit 'it' parameter
        if name == "it" {
            if let Some(it_type) = self.infer_implicit_it_type_at(context, resolver) {
                debug!("KT: Inferred 'it' type from context: {:?}", it_type);
                return it_type;
            }
        }

        // `this` inside a lambda with receiver (`with`/`apply`/`run`/DSL blocks)
        if name == "this" {
            if let Some(receiver) = self.enclosing_lambda_receiver_type(context, resolver) {
                debug!("KT: 'this' resolved to lambda receiver {:?}", receiver);
                return receiver;
            }
        }

        // Look up in the lexical scope containing the use site. The Kotlin
        // walker never pushes scopes onto the resolver, so resolving from the
        // resolver's current scope alone would miss parameters and locals.
        let resolved = resolver
            .find_scope_at_offset(offset)
            .and_then(|scope| resolver.resolve_in_scope_at_offset(name, scope, offset))
            .or_else(|| resolver.resolve(name));

        let declaration_start = if let Some(resolved) = resolved {
            debug!(
                "KT: Found '{}' in scope at offset {} with type {:?}",
                name, offset, resolved.ty
            );
            if !matches!(resolved.ty, Type::Unknown) {
                return resolved.ty;
            }
            resolver
                .symbol_table()
                .get_symbol(resolved.symbol_id)
                .map(|symbol| symbol.location.range.start)
        } else {
            None
        };

        // The scope builder deliberately leaves unannotated properties as
        // `Unknown`. Infer the nearest preceding declaration on demand so
        // consumers such as hover and completion see the value of expression
        // initializers (`val result = when { ... }`) without making symbol-table
        // construction depend on the workspace index.
        if let Some(ty) =
            self.lookup_property_initializer(name, context, offset, declaration_start, resolver)
        {
            debug!("KT: Inferred '{}' from its initializer: {:?}", name, ty);
            return ty;
        }

        // Resolve as class/object by import/current package/simple-name lookup.
        if let Some(class_fqn) = self.resolve_class_fqn_from_name(name) {
            let (simple_name, package) = self.split_fqn(&class_fqn);
            return Type::Class(ClassType {
                name: simple_name,
                package,
                type_arguments: vec![],
                is_diamond: false,
            });
        }

        // Implicit member of an enclosing lambda receiver: `with(sb) { length }`
        if let Some(receiver) = self.enclosing_lambda_receiver_type(context, resolver) {
            let ty = self.resolve_member(&receiver, name);
            if !matches!(ty, Type::Unknown) {
                return ty;
            }
        }

        debug!("KT: Could not resolve identifier '{}'", name);
        Type::Unknown
    }

    /// Infer an unannotated property from the closest declaration visible
    /// before the use site.
    fn lookup_property_initializer(
        &self,
        name: &str,
        context: &KtSyntaxNode,
        offset: u32,
        declaration_start: Option<u32>,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let root = context.ancestors().last()?;
        let declaration = root
            .descendants()
            .filter(|node| node.kind() == KtKind::PROPERTY_DECL)
            // `when (val x = value)` is scoped to the when body and is handled
            // by `lookup_when_subject_binding` above. It must never participate
            // in the general preceding-property fallback.
            .filter(|node| {
                !node
                    .ancestors()
                    .skip(1)
                    .any(|ancestor| ancestor.kind() == KtKind::WHEN_EXPR)
            })
            .filter(|node| u32::from(node.text_range().end()) <= offset)
            .filter(|node| property_decl_name(node).as_deref() == Some(name))
            .filter(|node| {
                declaration_start.is_none_or(|start| {
                    let range = node.text_range();
                    u32::from(range.start()) <= start && start < u32::from(range.end())
                })
            })
            .max_by_key(|node| u32::from(node.text_range().start()))?;

        let ty = self.infer_property_decl_initializer(&declaration, resolver);
        (!matches!(ty, Type::Unknown)).then_some(ty)
    }

    /// Split `receiver.member` into the receiver element and the member name.
    ///
    /// The receiver may be a node (`f().x`) or a bare token (`a.x`), because the
    /// parser does not wrap simple identifiers in nodes.
    fn split_member_access(
        node: &KtSyntaxNode,
    ) -> Option<(
        NodeOrToken<KtSyntaxNode, hyperion_kotlin::SyntaxToken>,
        String,
    )> {
        let mut receiver = None;
        let mut member_name = None;

        for child in node.children_with_tokens() {
            match &child {
                NodeOrToken::Node(_) => {
                    if receiver.is_none() {
                        receiver = Some(child);
                    }
                }
                NodeOrToken::Token(t) => {
                    if is_kotlin_identifier_token(t) {
                        if receiver.is_none() {
                            receiver = Some(child.clone());
                        } else {
                            member_name = Some(t.text().to_string());
                        }
                    }
                }
            }
        }

        Some((receiver?, member_name?))
    }

    /// Infer the type of a member access expression (receiver.member)
    fn infer_member_access(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        debug!("KT: Inferring member access");

        let Some((receiver, member_name)) = Self::split_member_access(node) else {
            if let Some(ty) = self.qualified_constructor_call_type(node) {
                return ty;
            }
            if let Some(ty) = self.member_call_with_type_arguments_type(node, resolver) {
                return ty;
            }
            debug!("KT: Incomplete member access");
            return Type::Unknown;
        };

        let receiver_type = self.infer_element(&receiver, resolver);
        debug!(
            "KT: Receiver type: {:?}, member: {}",
            receiver_type, member_name
        );

        self.resolve_member(&receiver_type, &member_name)
    }

    /// The type a member call with type arguments on a value returns:
    /// `factory.make<String>()`, `make` declared `fun <T> make():
    /// List<T>`, is a `List<String>`, the call's arguments bound to the
    /// member's own type parameters as [`Self::receiver_substituted_type`]
    /// binds them. The parser shapes that call as it does a qualified
    /// constructor call with type arguments, the member access `factory` `.`
    /// `make<String>()`, the call taking only the member's name.
    ///
    /// None when the access does not end in such a call or has no receiver
    /// before it. A type argument that is a projection or does not parse
    /// binds nothing, so the member's declared type is read as before.
    ///
    /// When the receiver's class has no such member, a stdlib extension the
    /// registry declares binds its own type parameters the same way:
    /// `listOf("a").filterIsInstance<String>()`, declared `fun <R>
    /// Iterable<*>.filterIsInstance(): List<R>`, is a `List<String>`.
    fn member_call_with_type_arguments_type(
        &self,
        node: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let call = node
            .children()
            .last()
            .filter(|child| child.kind() == KtKind::CALL_EXPR)?;
        let receiver = node
            .children_with_tokens()
            .find(|element| match element {
                NodeOrToken::Node(_) => true,
                NodeOrToken::Token(token) => is_kotlin_identifier_token(token),
            })
            .filter(|element| element.as_node() != Some(&call))?;
        let member_name = Self::callee_name_token(&call)?;
        let type_arguments = self.call_type_arguments(&call).unwrap_or_default();
        let receiver_type = self.infer_element(&receiver, resolver);
        let member = self.resolve_member_in_namespace(
            &receiver_type,
            &member_name,
            DeclarationNamespace::Callable,
            &type_arguments,
            None,
        );
        if !matches!(member, Type::Unknown)
            || matches!(receiver_type, Type::Unknown)
            || type_arguments.is_empty()
            || hyperion_kotlin::STDLIB.lookup(&member_name).is_none()
        {
            return Some(member);
        }
        let arg_types: Vec<_> = self
            .call_argument_types(&call, resolver)
            .iter()
            .map(semantics_type_to_kotlin)
            .collect();
        let extension = hyperion_kotlin::inference::infer_method_return_type_with_type_arguments(
            &semantics_type_to_kotlin(&receiver_type),
            &member_name,
            &type_arguments
                .iter()
                .map(semantics_type_to_kotlin)
                .collect::<Vec<_>>(),
            &arg_types,
        );
        Some(extension.map_or(member, |kt| kotlin_type_to_semantics(&kt)))
    }

    /// The type a qualified constructor call with type arguments creates:
    /// `java.util.ArrayList<String>()` is an `ArrayList` of
    /// `String`. The parser shapes that call as the member access `java.util`
    /// `.` `ArrayList<String>()`, the call taking only the last segment, so
    /// the class is the one the whole qualified name binds.
    ///
    /// None when the qualifier is not a plain dotted name or the name binds
    /// no class, as for a member call with type arguments on a value.
    fn qualified_constructor_call_type(&self, node: &KtSyntaxNode) -> Option<Type> {
        let call = node
            .children()
            .last()
            .filter(|child| child.kind() == KtKind::CALL_EXPR)?;
        let mut written: String = node
            .children_with_tokens()
            .take_while(|element| element.as_node() != Some(&call))
            .map(|element| element.to_string())
            .collect::<String>()
            .split_whitespace()
            .collect();
        written.push_str(&Self::callee_name_token(&call)?);
        self.dotted_constructor_call_type(&written, &call)
    }

    /// The type a qualified constructor call without type arguments creates:
    /// `java.util.ArrayList()` is the raw `ArrayList`, as
    /// `ArrayList()` is. Unlike the call with type arguments, the parser
    /// shapes it as a call whose callee is the member access
    /// `java.util.ArrayList`, so the class is the one the callee binds.
    ///
    /// None when the callee is not a plain dotted name or binds no class.
    fn qualified_bare_constructor_call_type(
        &self,
        callee: &KtSyntaxNode,
        call: &KtSyntaxNode,
    ) -> Option<Type> {
        let written: String = callee.text().to_string().split_whitespace().collect();
        self.dotted_constructor_call_type(&written, call)
    }

    /// The type the constructor call `call` creates when `written`, its
    /// class's dotted name, binds a class.
    fn dotted_constructor_call_type(&self, written: &str, call: &KtSyntaxNode) -> Option<Type> {
        if !written.split('.').all(is_kotlin_identifier_text) {
            return None;
        }
        let class_fqn = self.resolve_class_fqn_from_name(written)?;
        Some(self.constructor_call_type(&class_fqn, written, call))
    }

    /// Infer the type of a safe access expression (receiver?.member)
    fn infer_safe_access(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        // Safe access is like member access, but the result is nullable
        // For now, treat it the same as regular member access
        self.infer_member_access(node, resolver)
    }

    /// Infer the type of a call expression (function call)
    fn infer_call(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        debug!("KT: Inferring call expression");

        // Method call on a receiver: `receiver.method(args)`
        if let Some(callee) = node.children().find(|child| {
            matches!(
                child.kind(),
                KtKind::MEMBER_ACCESS_EXPR | KtKind::SAFE_ACCESS_EXPR
            )
        }) {
            let ty = self.infer_method_call_on_receiver(&callee, node, resolver);
            if matches!(ty, Type::Unknown) {
                if let Some(ty) = self.qualified_bare_constructor_call_type(&callee, node) {
                    return ty;
                }
            }
            return ty;
        }

        // Simple function call: the callee is the first identifier token.
        let Some(func_name) = Self::callee_name_token(node) else {
            debug!("KT: Could not infer call expression");
            return Type::Unknown;
        };

        // Kotlin stdlib generic functions (listOf, mapOf, ...) resolve via the
        // shared signature registry so type arguments survive. Inferring the
        // arguments is not cheap, so only do it when the registry knows the name.
        // The type arguments the call writes bind the function's own type
        // parameters: `emptyList<String>()` is a `List<String>`.
        if hyperion_kotlin::STDLIB.lookup(&func_name).is_some() {
            let arg_types = self.call_argument_types(node, resolver);
            if let Some(kt) = hyperion_kotlin::inference::infer_generic_call_with_type_arguments(
                &func_name,
                &self.kotlin_call_type_arguments(node),
                &arg_types
                    .iter()
                    .map(semantics_type_to_kotlin)
                    .collect::<Vec<_>>(),
            ) {
                let ty = kotlin_type_to_semantics(&kt);
                if !matches!(ty, Type::Unknown) {
                    debug!("KT: stdlib call {} -> {:?}", func_name, ty);
                    return ty;
                }
            }
        }

        let ty = self.infer_function_call(&func_name, node, resolver);
        if !matches!(ty, Type::Unknown) {
            return ty;
        }

        // Implicit receiver call inside a lambda-with-receiver:
        // `with(sb) { append("x") }` resolves `append` against StringBuilder.
        if let Some(receiver) = self.enclosing_lambda_receiver_type(node, resolver) {
            let ty = self.resolve_method_return_type(&receiver, &func_name);
            if !matches!(ty, Type::Unknown) {
                debug!(
                    "KT: '{}' resolved against lambda receiver {:?}",
                    func_name, receiver
                );
                return ty;
            }
        }

        Type::Unknown
    }

    /// Infer the type of a method call on a receiver (receiver.method())
    fn infer_method_call_on_receiver(
        &self,
        member_access_node: &KtSyntaxNode,
        call_node: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Type {
        let Some((receiver, method_name)) = Self::split_member_access(member_access_node) else {
            return Type::Unknown;
        };

        let receiver_type = self.infer_element(&receiver, resolver);
        debug!(
            "KT: Method call - receiver type: {:?}, method: {}",
            receiver_type, method_name
        );

        // Try the Kotlin stdlib signature registry first so generic methods such
        // as `List<T>.map((T) -> R): List<R>` keep their type arguments.
        if !matches!(receiver_type, Type::Unknown)
            && hyperion_kotlin::STDLIB.lookup(&method_name).is_some()
        {
            let arg_types = self.call_argument_types(call_node, resolver);
            if let Some(kt) = hyperion_kotlin::inference::infer_method_return_type(
                &semantics_type_to_kotlin(&receiver_type),
                &method_name,
                &arg_types
                    .iter()
                    .map(semantics_type_to_kotlin)
                    .collect::<Vec<_>>(),
            ) {
                let ty = kotlin_type_to_semantics(&kt);
                if !matches!(ty, Type::Unknown) {
                    debug!("KT: stdlib method {} -> {:?}", method_name, ty);
                    return ty;
                }
            }
        }

        let mut call_arguments = || self.call_value_argument_types(call_node, resolver);
        self.resolve_member_in_namespace(
            &receiver_type,
            &method_name,
            DeclarationNamespace::Callable,
            &[],
            Some(&mut call_arguments),
        )
    }

    /// The types of a call's value arguments, in order, a trailing lambda
    /// last. None when an argument is named or spread, so its
    /// position need not be its parameter's, or is not one expression, and
    /// when the parentheses do not close.
    fn call_value_argument_types(
        &self,
        call: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Vec<Type>> {
        let mut types = Vec::new();
        let mut current = Vec::new();
        let mut inside = false;
        let mut closed = false;
        for element in call.children_with_tokens() {
            let kind = element.kind();
            if kind.is_trivia() {
                continue;
            }
            match kind {
                KtKind::L_PAREN if !inside && !closed => inside = true,
                KtKind::COMMA | KtKind::R_PAREN if inside => {
                    match current.as_slice() {
                        [] => {}
                        [argument] if !is_named_or_spread_argument(argument) => {
                            types.push(self.infer_element(argument, resolver));
                        }
                        _ => return None,
                    }
                    current.clear();
                    if kind == KtKind::R_PAREN {
                        inside = false;
                        closed = true;
                    }
                }
                _ if inside => current.push(element),
                KtKind::LAMBDA_EXPR => types.push(self.infer_element(&element, resolver)),
                _ => {}
            }
        }
        (!inside).then_some(types)
    }

    /// The identifier token naming the callee of a `CALL_EXPR`, when the callee
    /// is not a member access (i.e. `foo(...)` rather than `x.foo(...)`).
    fn callee_name_token(call_node: &KtSyntaxNode) -> Option<String> {
        for child in call_node.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) => {
                    if t.kind() == KtKind::L_PAREN || t.kind() == KtKind::L_BRACE {
                        return None;
                    }
                    if is_kotlin_identifier_token(&t) {
                        return Some(t.text().to_string());
                    }
                }
                NodeOrToken::Node(_) => return None,
            }
        }
        None
    }

    /// Types of the value arguments of a call, including a trailing lambda.
    fn call_argument_types(
        &self,
        call_node: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Vec<Type> {
        let mut args = Vec::new();
        let mut depth = 0_i32;

        for element in call_node.children_with_tokens() {
            match &element {
                NodeOrToken::Token(t) => match t.kind() {
                    KtKind::L_PAREN => depth += 1,
                    KtKind::R_PAREN => depth -= 1,
                    KtKind::COMMA => {}
                    _ => {
                        if depth > 0 && !t.text().trim().is_empty() {
                            args.push(self.infer_token(t, resolver));
                        }
                    }
                },
                NodeOrToken::Node(n) => {
                    // Trailing lambda sits outside the parentheses.
                    if depth > 0 || n.kind() == KtKind::LAMBDA_EXPR {
                        args.push(self.infer_expression(n, resolver));
                    }
                }
            }
        }

        args
    }

    /// Infer the type of a simple function call, `call`
    fn infer_function_call(
        &self,
        func_name: &str,
        call: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Type {
        debug!("KT: Looking up function: {}", func_name);

        // Try local scope first
        if let Some(resolved) = resolver.resolve(func_name) {
            debug!(
                "KT: Found function '{}' with type {:?}",
                func_name, resolved.ty
            );
            // Constructors/type aliases may resolve directly to class type.
            return resolved.ty;
        }

        // Constructor/simple class call: Query() / User(...)
        if let Some(class_fqn) = self.resolve_class_fqn_from_name(func_name) {
            return self.constructor_call_type(&class_fqn, func_name, call);
        }

        Type::Unknown
    }

    /// The type a constructor call, `call`, of the class `class_fqn` creates,
    /// its class written `written` (`ArrayList`, `java.util.ArrayList`): the
    /// class with the type arguments the call writes, read as a
    /// declared type's are, so `val roster = ArrayList<String>()` is an
    /// `ArrayList<String>`, as `val roster: ArrayList<String>` is.
    ///
    /// A call that writes none, of a typealias whose expansion gives its class
    /// arguments, has the expansion's: `Names()` under `typealias Names =
    /// java.util.ArrayList<String>` is an `ArrayList<String>`.
    ///
    /// The class is raw, as before, when the call writes no type arguments
    /// and names no such alias, since they are not inferred from the value
    /// arguments or an expected type; when an argument is a projection, which
    /// a constructor call cannot take, or does not parse; and when `written`
    /// binds a typealias and the call writes arguments, which are the alias's,
    /// not the class's.
    fn constructor_call_type(&self, class_fqn: &str, written: &str, call: &KtSyntaxNode) -> Type {
        let (name, package) = self.split_fqn(class_fqn);
        Type::Class(ClassType {
            name,
            package,
            type_arguments: self
                .constructor_type_arguments(written, call)
                .unwrap_or_default(),
            is_diamond: false,
        })
    }

    /// The type arguments a constructor call writes, as
    /// [`Self::constructor_call_type`] describes.
    fn constructor_type_arguments(&self, written: &str, call: &KtSyntaxNode) -> Option<Vec<Type>> {
        let Some(spellings) = Self::call_type_argument_spellings(call) else {
            return self.alias_expansion_arguments(written);
        };
        if !matches!(
            self.workspace_index.resolve_kotlin_type_alias_arguments(
                self.file_id,
                &[],
                written,
                &spellings,
            ),
            hyperion_workspace::KotlinTypeAliasArguments::NotAnAlias
        ) {
            return None;
        }
        self.call_type_arguments_from(&spellings)
    }

    /// The type arguments the typealias `written` binds to gives its class at
    /// a use that writes none: `String` for `Names` under
    /// `typealias Names = java.util.ArrayList<String>`. Such a use matches
    /// only an alias with no type parameters, so none of them is one. An
    /// expansion that gives its class no arguments gives none, so `Text`
    /// under `typealias Text = java.lang.StringBuilder` is the class itself.
    ///
    /// None when `written` binds no alias, when the expansion gives its class
    /// a projected argument, and when the alias has type parameters, which a
    /// use without arguments leaves unbound: `StringMap` under `typealias
    /// StringMap<V> = java.util.HashMap<String, V>`.
    fn alias_expansion_arguments(&self, written: &str) -> Option<Vec<Type>> {
        let hyperion_workspace::KotlinTypeAliasArguments::Expanded(spellings) = self
            .workspace_index
            .resolve_kotlin_type_alias_arguments(self.file_id, &[], written, &[])
        else {
            return None;
        };
        let projected = spellings.iter().any(|spelling| {
            spelling
                .split(['<', '>', ','])
                .map(str::trim)
                .any(|part| part == "*" || part.starts_with("out ") || part.starts_with("in "))
        });
        if projected {
            return None;
        }
        self.call_type_arguments_from(&spellings)
    }

    /// The type arguments a call writes, each read as a declared type is.
    /// None when it writes none, or one is a projection, which a call cannot
    /// take, or does not parse.
    fn call_type_arguments(&self, call: &KtSyntaxNode) -> Option<Vec<Type>> {
        self.call_type_arguments_from(&Self::call_type_argument_spellings(call)?)
    }

    /// [`Self::call_type_arguments`] in the stdlib registry's type model, for
    /// a call to a function the registry declares. Empty, so the
    /// call binds no type parameter, when the call writes none or one does
    /// not read.
    fn kotlin_call_type_arguments(&self, call: &KtSyntaxNode) -> Vec<hyperion_kotlin::KotlinType> {
        self.call_type_arguments(call)
            .unwrap_or_default()
            .iter()
            .map(semantics_type_to_kotlin)
            .collect()
    }

    /// The spellings of the type arguments a call writes, as
    /// [`Self::call_type_arguments`] describes.
    fn call_type_argument_spellings(call: &KtSyntaxNode) -> Option<Vec<String>> {
        let list = call
            .children()
            .find(|child| child.kind() == KtKind::TYPE_ARGUMENT_LIST)?;
        if list.children_with_tokens().any(|element| {
            matches!(
                element.kind(),
                KtKind::STAR | KtKind::OUT_KW | KtKind::IN_KW
            )
        }) {
            return None;
        }
        let spellings: Vec<String> = list
            .children()
            .filter(|child| child.kind() == KtKind::TYPE_REF)
            .map(|type_ref| type_ref.text().to_string().trim().to_string())
            .collect();
        (!spellings.is_empty()).then_some(spellings)
    }

    /// `spellings` read as declared types in the file's package.
    fn call_type_arguments_from(&self, spellings: &[String]) -> Option<Vec<Type>> {
        let package = self
            .workspace_index
            .get_imports(self.file_id)
            .and_then(|imports| imports.current_package);
        spellings
            .iter()
            .map(|spelling| {
                crate::kotlin_resolver::kotlin_type_from_text(spelling, package.as_deref())
            })
            .collect()
    }

    /// Resolve a member (property or method) on a type
    fn resolve_member(&self, receiver_type: &Type, member_name: &str) -> Type {
        self.resolve_member_in_namespace(
            receiver_type,
            member_name,
            hyperion_workspace::DeclarationNamespace::Value,
            &[],
            None,
        )
    }

    /// The type of the member `member_name` of `receiver_type`, its call
    /// writing `type_arguments` for the member's own type parameters and
    /// passing the value arguments `call_arguments` gives.
    fn resolve_member_in_namespace(
        &self,
        receiver_type: &Type,
        member_name: &str,
        namespace: hyperion_workspace::DeclarationNamespace,
        type_arguments: &[Type],
        call_arguments: CallArguments<'_>,
    ) -> Type {
        if let Type::Class(class_type) = receiver_type {
            let imported = self.imported_receiver_class(class_type);
            let class_type = imported.as_ref().unwrap_or(class_type);
            // Build FQN for the member
            let class_fqn = class_type.qualified_name();
            let member_fqn = format!("{class_fqn}.{member_name}");

            debug!("KT: Looking up member: {}", member_fqn);

            // Get the full symbol with metadata (return_type, etc.). Kotlin
            // companion members are invoked through the owning class but are
            // indexed under the explicit `Companion` container.
            let symbol = self
                .workspace_index
                .get_symbol_target(&hyperion_workspace::SymbolTarget::new(
                    member_fqn.as_str(),
                    namespace,
                ))
                .or_else(|| {
                    self.workspace_index
                        .get_symbol_target(&hyperion_workspace::SymbolTarget::new(
                            format!("{class_fqn}.Companion.{member_name}"),
                            namespace,
                        ))
                });
            // A private Java field is not what `obj.name` means in Kotlin: the
            // synthetic property backed by `getName()` is.
            let symbol = symbol.filter(|symbol| {
                !(namespace == hyperion_workspace::DeclarationNamespace::Value
                    && symbol.kind == SymbolKind::FIELD
                    && symbol.modifiers.is_private)
            });
            let symbol = symbol.or_else(|| {
                if namespace == hyperion_workspace::DeclarationNamespace::Value {
                    self.java_getter_symbol(&class_fqn, member_name)
                } else {
                    None
                }
            });
            if let Some(symbol) = symbol {
                debug!(
                    "KT: Found symbol: kind={:?}, return_type={:?}",
                    symbol.kind, symbol.return_type
                );
                if !self.source_overloads_agree(&symbol) {
                    debug!("KT: Overloads of {} disagree about their type", member_fqn);
                    return Type::Unknown;
                }

                // For methods/properties, extract the return type
                if let Some(return_type_str) = &symbol.return_type {
                    debug!("KT: Parsing return type: {}", return_type_str);
                    if let Some(substituted) = self.receiver_substituted_type(
                        &symbol,
                        class_type,
                        type_arguments,
                        return_type_str,
                    ) {
                        debug!("KT: Return type {} is {:?}", return_type_str, substituted);
                        return substituted;
                    }
                    return self.parse_type_string(
                        return_type_str,
                        class_type.package.as_deref(),
                        Some(class_fqn.as_ref()),
                    );
                }

                debug!("KT: Symbol has no return_type metadata");
            } else if let Some(inherited) = self.inherited_member_type(
                class_type,
                &class_fqn,
                member_name,
                namespace,
                type_arguments,
                call_arguments,
            ) {
                debug!("KT: Inherited member {} is {:?}", member_fqn, inherited);
                return inherited;
            }

            debug!("KT: Member '{}' not found in workspace", member_fqn);
            Type::Unknown
        } else {
            debug!(
                "KT: Cannot resolve member on non-class type: {:?}",
                receiver_type
            );
            Type::Unknown
        }
    }

    /// The class a receiver's type names when it is written as a simple name
    /// that the file imports. The Kotlin symbol table is built
    /// without the index, so it places such a name in the file's own package:
    /// `val roster: ArrayList<String>` under `import java.util.ArrayList` is an
    /// `app.ArrayList<String>` there. When that is not a class, the name is
    /// resolved through the file's imports, the default ones included, as
    /// completion resolves the receiver, and the receiver's type arguments are
    /// kept. So is a name with no package at all, as the symbol table records
    /// the constructor call that initializes `val factory = Factory()`. A
    /// name that binds a typealias, with no type arguments, has the arguments
    /// its expansion gives the class: `val roster = Names()` under `typealias
    /// Names = java.util.ArrayList<String>` is an `ArrayList<String>`, and
    /// `val text = Text()` under `typealias Text = java.lang.StringBuilder` a
    /// `StringBuilder`. A Kotlin collection, which the index does not hold,
    /// is the Java class it maps to, with its type arguments: the
    /// `kotlin.collections.List<String>` a stdlib call such as
    /// `emptyList<String>()` returns is a `java.util.List<String>`, so its
    /// `get(0)` is a `String`.
    ///
    /// None when the receiver is a class as it is, when its package is
    /// neither the file's nor absent (a qualified spelling, or a type read
    /// elsewhere), when the file does not bind the name, when the name binds
    /// a typealias with type arguments, which are the alias's, not the
    /// class's (`typealias Inverse<K, V> = java.util.HashMap<V, K>`), and
    /// when it binds a generic one without, or one whose expansion has a
    /// projected argument (see [`Self::alias_expansion_arguments`]).
    fn imported_receiver_class(&self, receiver: &ClassType) -> Option<ClassType> {
        let qualified = receiver.qualified_name();
        if self.workspace_index.has_class(&qualified) {
            return None;
        }
        if let Some(mapped) = receiver
            .package
            .as_deref()
            .filter(|package| *package == "kotlin.collections")
            .and_then(|_| hyperion_workspace::kotlin_builtin_jvm_fqn(&qualified))
            .filter(|mapped| self.workspace_index.has_class(mapped))
        {
            let (name, package) = self.split_fqn(mapped);
            return Some(ClassType {
                name,
                package,
                type_arguments: receiver.type_arguments.clone(),
                is_diamond: receiver.is_diamond,
            });
        }
        let imports = self.workspace_index.get_imports(self.file_id)?;
        if receiver.package.is_some() && receiver.package != imports.current_package {
            return None;
        }
        let hyperion_workspace::TypeNameResolution::Exact(fqn) = self
            .workspace_index
            .resolve_type_name(self.file_id, &[], &receiver.name)
        else {
            return None;
        };
        let arguments: Vec<String> = receiver
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        let type_arguments = if matches!(
            self.workspace_index.resolve_kotlin_type_alias_arguments(
                self.file_id,
                &[],
                &receiver.name,
                &arguments,
            ),
            hyperion_workspace::KotlinTypeAliasArguments::NotAnAlias
        ) {
            receiver.type_arguments.clone()
        } else if receiver.type_arguments.is_empty() {
            self.alias_expansion_arguments(&receiver.name)?
        } else {
            return None;
        };
        let (name, package) = self.split_fqn(&fqn);
        Some(ClassType {
            name,
            package,
            type_arguments,
            is_diamond: receiver.is_diamond,
        })
    }

    /// A member's declared type with the receiver's type arguments substituted
    /// for the type parameters of the class declaring the member.
    /// A declared type that is such a parameter is the receiver's argument:
    /// `E get(int index)` in `java.util.ArrayList<E>`, called on an
    /// `ArrayList<String>`, returns `String`, and `V get(Object key)` on a
    /// `HashMap<String, List<Int>>` returns `List<Int>`. A generic declared
    /// type takes the argument wherever the parameter stands among its own
    /// arguments, at any depth: `Iterator<E> iterator()` there returns an
    /// `Iterator<String>`, and `HashMap<K, ArrayList<V>>` on a `Table<Int,
    /// String>` a map of `Int` to `ArrayList<String>`. Java and Kotlin members
    /// alike, from source or bytecode.
    ///
    /// The type arguments the call writes, `type_arguments`, are substituted
    /// for the member's own type parameters the same way, and win over the
    /// receiver's for a name the member redeclares: `make<String>()`, `make`
    /// declared `fun <T> make(): List<T>`, returns a `List<String>`, whether
    /// or not its class is generic.
    ///
    /// None, so the declared type is read as before, when neither the
    /// receiver gives an argument for each of the class's type parameters (a
    /// raw receiver) nor the call writes type arguments, and as
    /// [`Self::substituted_type`] describes.
    fn receiver_substituted_type(
        &self,
        member: &hyperion_workspace::Symbol,
        receiver: &ClassType,
        type_arguments: &[Type],
        declared: &str,
    ) -> Option<Type> {
        let owner_fqn = member.container.as_deref()?;
        let owner = if receiver.type_arguments.is_empty() {
            None
        } else {
            self.workspace_index
                .get_symbol_target(&SymbolTarget::new(owner_fqn, DeclarationNamespace::Type))
                .filter(|owner| owner.type_parameters.len() == receiver.type_arguments.len())
        };
        if owner.is_none() && type_arguments.is_empty() {
            return None;
        }
        let bindings: Vec<(&str, &Type)> = owner
            .iter()
            .flat_map(|owner| owner.type_parameters.iter())
            .map(|parameter| parameter.name.as_ref())
            .zip(&receiver.type_arguments)
            .collect();
        self.substituted_type(declared, type_arguments, &bindings, member, owner_fqn)
    }

    /// `declared`, read in the class `container`, with each of `member`'s own
    /// type parameters replaced by the call's argument for it in
    /// `own_arguments`, and each other name `bindings` binds replaced by its
    /// type: the whole type, or an argument of a generic type at any depth,
    /// whose other arguments are read as class types with their own
    /// arguments. A Kotlin use-site projection among the arguments
    /// (`out E`, `in E`) is substituted inside and kept as a wildcard, so
    /// `ArrayList<out T>` with `T` bound to `String` is an `ArrayList` of
    /// `out String`; a whole type bound to an `out` projection is its bound,
    /// so `get(0)` on that list is a `String`.
    ///
    /// A Java array, `declared` or its element, is Kotlin's view of it,
    /// whether or not anything in it is bound: `T[]` with `T`
    /// bound to `String` is an `Array<String>`, `Object[]` an `Array<Any>` and
    /// `int[]` an `IntArray`.
    ///
    /// None, so the caller reads the declared type as before, when nothing in
    /// it is bound and it is not an array; when a bound type is a type
    /// variable, a star or an `in` projection where the whole type is bound,
    /// or anything else that is not a class, primitive or array type or a
    /// projection of one; when `own_arguments` is not empty and does not give
    /// one argument for each of `member`'s type parameters; when `member`
    /// declares a type parameter it names that `own_arguments` does not bind,
    /// which shadows the class's; and when any part is not a class this
    /// inference knows: a star projection, a Java wildcard (`? extends E`), an
    /// array inside a type argument, a function type, or a name that does not
    /// resolve.
    fn substituted_type(
        &self,
        declared: &str,
        own_arguments: &[Type],
        bindings: &[(&str, &Type)],
        member: &hyperion_workspace::Symbol,
        container: &str,
    ) -> Option<Type> {
        if !own_arguments.is_empty() && own_arguments.len() != member.type_parameters.len() {
            return None;
        }
        let own = || {
            member
                .type_parameters
                .iter()
                .map(|parameter| parameter.name.as_ref())
        };
        let bindings: Vec<(&str, &Type)> = own()
            .zip(own_arguments)
            .chain(
                bindings
                    .iter()
                    .copied()
                    .filter(|(name, _)| !own().any(|parameter| parameter == *name)),
            )
            .collect();
        if let Some(element) = declared.trim().strip_suffix("[]") {
            return self.array_type(element, &bindings, member, container);
        }
        let (substituted, bound) =
            self.substituted_type_part(declared, &bindings, member, container, true)?;
        bound.then_some(substituted)
    }

    /// Kotlin's view of a Java array whose element is `element`, read as
    /// [`Self::substituted_type_part`] reads a type argument, as
    /// [`Self::substituted_type`] describes. An `Object` element is `Any`.
    fn array_type(
        &self,
        element: &str,
        bindings: &[(&str, &Type)],
        member: &hyperion_workspace::Symbol,
        container: &str,
    ) -> Option<Type> {
        let element = element.trim();
        if let Some(primitive) = java_primitive(element) {
            return Some(kotlin_class(primitive_array_name(primitive), Vec::new()));
        }
        let element = match element.strip_suffix("[]") {
            Some(inner) => self.array_type(inner, bindings, member, container)?,
            None => {
                match self
                    .substituted_type_part(element, bindings, member, container, false)?
                    .0
                {
                    Type::Class(class) if class.qualified_name() == "java.lang.Object" => {
                        kotlin_class("Any", Vec::new())
                    }
                    element => element,
                }
            }
        };
        Some(kotlin_class("Array", vec![element]))
    }

    /// [`Self::substituted_type`] for one part of the declared type, the
    /// whole type when `whole`, otherwise a type argument, and whether a
    /// binding was used in it.
    fn substituted_type_part(
        &self,
        declared: &str,
        bindings: &[(&str, &Type)],
        member: &hyperion_workspace::Symbol,
        container: &str,
        whole: bool,
    ) -> Option<(Type, bool)> {
        let declared = declared.trim();
        let declared = declared.strip_suffix('?').unwrap_or(declared).trim_end();
        if let Some((_, argument)) = bindings.iter().find(|(name, _)| *name == declared) {
            return match argument {
                argument if is_projectable(argument) => Some(((*argument).clone(), true)),
                // What an `out` projection gives out is its bound; as an
                // argument, the projection itself is kept.
                Type::Wildcard {
                    upper_bound: Some(bound),
                    lower_bound: None,
                } if whole => is_projectable(bound).then(|| ((**bound).clone(), true)),
                Type::Wildcard {
                    upper_bound: Some(bound),
                    lower_bound: None,
                }
                | Type::Wildcard {
                    upper_bound: None,
                    lower_bound: Some(bound),
                } if !whole => is_projectable(bound).then(|| ((*argument).clone(), true)),
                _ => None,
            };
        }
        // A type parameter of the member's own that the call gives no
        // argument for: the class's of the same name is shadowed.
        if member
            .type_parameters
            .iter()
            .any(|parameter| parameter.name.as_ref() == declared)
        {
            return None;
        }
        if !whole {
            let projection = declared
                .strip_prefix("out ")
                .map(|bound| (bound, true))
                .or_else(|| declared.strip_prefix("in ").map(|bound| (bound, false)));
            if let Some((projected, covariant)) = projection {
                let (inner, bound) =
                    self.substituted_type_part(projected, bindings, member, container, false)?;
                let inner = match inner {
                    Type::Wildcard {
                        upper_bound: Some(inner),
                        lower_bound: None,
                    } if covariant => *inner,
                    Type::Wildcard { .. } => return None,
                    inner => inner,
                };
                let inner = Some(Box::new(inner));
                let wildcard = if covariant {
                    Type::Wildcard {
                        upper_bound: inner,
                        lower_bound: None,
                    }
                } else {
                    Type::Wildcard {
                        upper_bound: None,
                        lower_bound: inner,
                    }
                };
                return Some((wildcard, bound));
            }
        }
        if declared.is_empty()
            || declared.contains(['(', '[', '@', '*'])
            || declared.starts_with('?')
            || declared.starts_with("out ")
            || declared.starts_with("in ")
        {
            return None;
        }
        let Some((head, arguments)) = split_type_arguments(declared) else {
            return self
                .known_type(declared, Some(container))
                .map(|known| (known, false));
        };
        let Some(Type::Class(mut class)) = self.known_type(head, Some(container)) else {
            return None;
        };
        let mut bound = false;
        for argument in arguments {
            let (argument, argument_bound) =
                self.substituted_type_part(argument, bindings, member, container, false)?;
            class.type_arguments.push(argument);
            bound |= argument_bound;
        }
        Some((Type::Class(class), bound))
    }

    /// The type of a member the receiver's class does not declare but
    /// inherits, transitively, from a superclass or superinterface:
    /// `stream()` on a `java.util.ArrayList<String>` is the
    /// default method of `java.util.Collection`, reached through
    /// `AbstractList` and `List`. The index's effective-member election picks
    /// the declaration, so an override nearer the receiver wins over the one
    /// it overrides, and projects its declared type through the generic
    /// supertypes onto the receiver's arguments: `Stream<E>` in
    /// `Collection<E>` is `Stream<String>` there.
    ///
    /// For a property access, an inherited field or Kotlin property comes
    /// first, then an inherited Java getter. A static or private declaration
    /// is not a member of a Kotlin receiver. None when nothing inherited has
    /// the name, when the projection is unavailable (a malformed or
    /// conflicting generic path), and, for a call, when the elected overloads
    /// disagree about their type or one returns a type parameter of its own,
    /// unless the call's type arguments, `type_arguments`, or its value
    /// arguments, `call_arguments`, settle it as
    /// [`Self::inherited_method_type`] describes.
    fn inherited_member_type(
        &self,
        receiver: &ClassType,
        class_fqn: &str,
        member_name: &str,
        namespace: DeclarationNamespace,
        type_arguments: &[Type],
        call_arguments: CallArguments<'_>,
    ) -> Option<Type> {
        let arguments: Vec<String> = receiver
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        if namespace != DeclarationNamespace::Value {
            return self.inherited_method_type(
                receiver,
                class_fqn,
                &arguments,
                member_name,
                InheritedCall::Call {
                    type_arguments,
                    call_arguments,
                },
            );
        }
        if let Some(field) = self.workspace_index.find_projected_effective_field_by_name(
            class_fqn,
            &arguments,
            member_name,
        ) {
            let symbol = &field.definition.symbol;
            if symbol.container.as_deref() != Some(class_fqn)
                && !symbol.modifiers.is_private
                && !symbol.modifiers.is_static
            {
                let declared = field.projected_type?;
                return Some(self.projected_member_type(
                    receiver,
                    &arguments,
                    symbol,
                    &declared,
                    &[],
                ));
            }
        }
        hyperion_types::jvm_names::java_getter_names(member_name)
            .into_iter()
            .find_map(|getter| {
                self.inherited_method_type(
                    receiver,
                    class_fqn,
                    &arguments,
                    &getter,
                    InheritedCall::Getter,
                )
            })
    }

    /// The projected type of the inherited method `name` of `class_fqn`, as
    /// [`Self::inherited_member_type`] describes. A getter takes no
    /// parameters. The elected overloads, the receiver's own among them,
    /// must agree about their type.
    ///
    /// When they do not, the call's value arguments, which `call_arguments`
    /// reads, rule out each overload that cannot take them, as
    /// [`Self::call_own_arguments`] tells, and the overloads left, with the
    /// type parameters of their own the arguments bind, must agree:
    /// `toArray()` on a `java.util.ArrayList<String>` is
    /// `Object[] toArray()`, an `Array<Any>`, and `toArray(names)`, `names` an
    /// `Array<String>`, is `<T> T[] toArray(T[] a)` with `T` bound to
    /// `String`, an `Array<String>`, the inherited `toArray(IntFunction<T[]>)`
    /// ruled out. When those left disagree, the call takes the most specific
    /// of them, as Kotlin does: the one overload the arguments certainly fit
    /// that is as specific as each other left, as
    /// [`Self::at_least_as_specific`] tells, so `pick("x")` is
    /// `pick(CharSequence)`, not `pick(Object)`. None when no overload is
    /// left, when none or more than one is the most specific, and when the
    /// arguments are not read.
    ///
    /// A call that writes type arguments is read instead by binding them to
    /// the method's own type parameters, as [`Self::receiver_substituted_type`]
    /// does for a member of the receiver's class:
    /// `sub.make<String>()`, `make` declared `fun <T> make(): List<T>` in a
    /// superclass of `sub`'s, is a `List<String>`. Only the overloads with one
    /// type parameter for each argument are read, and they must agree.
    fn inherited_method_type(
        &self,
        receiver: &ClassType,
        class_fqn: &str,
        arguments: &[String],
        name: &str,
        call: InheritedCall<'_, '_>,
    ) -> Option<Type> {
        let (getter, type_arguments, call_arguments) = match call {
            InheritedCall::Getter => (true, &[][..], None),
            InheritedCall::Call {
                type_arguments,
                call_arguments,
            } => (false, type_arguments, call_arguments),
        };
        let methods = self
            .workspace_index
            .find_projected_effective_methods_by_name(class_fqn, arguments, name);
        let methods: Vec<&ProjectedEffectiveMethod> = methods
            .iter()
            .filter(|method| {
                let symbol = &method.definition.symbol;
                symbol.is_method()
                    && !symbol.modifiers.is_static
                    && !symbol.modifiers.is_private
                    && (!getter || symbol.parameters.is_empty())
            })
            .collect();
        if !methods
            .iter()
            .all(|method| self.source_overloads_agree(&method.definition.symbol))
        {
            return None;
        }
        if !type_arguments.is_empty() {
            return agreed_type(
                methods
                    .iter()
                    .filter(|method| {
                        method.definition.symbol.type_parameters.len() == type_arguments.len()
                    })
                    .map(|method| self.overload_type(receiver, arguments, method, type_arguments)),
            );
        }
        if let Some(agreed) = agreed_type(
            methods
                .iter()
                .map(|method| self.overload_type(receiver, arguments, method, &[])),
        ) {
            return Some(agreed);
        }
        let call_arguments = call_arguments?()?;
        let fitting: Vec<(&ProjectedEffectiveMethod, Vec<Type>, bool)> = methods
            .iter()
            .filter_map(|method| {
                let (own_arguments, certain) = self.call_own_arguments(method, &call_arguments)?;
                Some((*method, own_arguments, certain))
            })
            .collect();
        if let Some(agreed) = agreed_type(fitting.iter().map(|(method, own_arguments, _)| {
            self.overload_type(receiver, arguments, method, own_arguments)
        })) {
            return Some(agreed);
        }
        let mut most_specific = fitting.iter().filter(|(method, _, certain)| {
            *certain
                && fitting.iter().all(|(other, _, _)| {
                    std::ptr::eq(*method, *other) || self.at_least_as_specific(method, other)
                })
        });
        let (method, own_arguments, _) = most_specific.next()?;
        if most_specific.next().is_some() {
            return None;
        }
        self.overload_type(receiver, arguments, method, own_arguments)
    }

    /// Whether every declaration `member`'s source makes under its name
    /// gives a call the type `member` does, the same declared type and type
    /// parameters. Kotlin overloads share one FQN, and the index
    /// keeps one of them, `member`: with `fun pick(x: Any): Int` and `fun
    /// pick(x: CharSequence): String` declared, `pick("x")` is not the kept
    /// overload's `Int`. True for a name with one declaration.
    fn source_overloads_agree(&self, member: &Arc<hyperion_workspace::Symbol>) -> bool {
        self.workspace_index
            .same_source_declarations(member)
            .iter()
            .all(|overload| {
                overload.return_type == member.return_type
                    && overload.type_parameters == member.type_parameters
            })
    }

    /// Whether each parameter of `method` certainly is, or extends, the
    /// same parameter of `other`, both read as [`Self::parameter_class`]
    /// reads them and the index tells: `pick(CharSequence)` is
    /// as specific as `pick(Object)`, and not the reverse. False when the
    /// two differ in their count of parameters, when either has no
    /// projection or takes a vararg, and when a parameter is not read or
    /// its class's hierarchy is not all indexed.
    fn at_least_as_specific(
        &self,
        method: &ProjectedEffectiveMethod,
        other: &ProjectedEffectiveMethod,
    ) -> bool {
        let (Some(mine), Some(theirs)) = (method.projection.as_ref(), other.projection.as_ref())
        else {
            return false;
        };
        if mine.parameters.len() != theirs.parameters.len()
            || mine
                .parameters
                .iter()
                .chain(&theirs.parameters)
                .any(ProjectedCallableParameter::is_variadic)
        {
            return false;
        }
        let class = |method: &ProjectedEffectiveMethod, parameter: &ProjectedCallableParameter| {
            let symbol = &method.definition.symbol;
            self.parameter_class(
                &parameter.type_name,
                &symbol.type_parameters,
                symbol.container.as_deref().unwrap_or_default(),
            )
        };
        mine.parameters
            .iter()
            .zip(&theirs.parameters)
            .all(|(narrow, wide)| {
                let (Some(narrow), Some(wide)) = (class(method, narrow), class(other, wide)) else {
                    return false;
                };
                narrow == wide
                    || wide == "java.lang.Object"
                    || crate::kotlin_certain_errors::indexed_subtype(
                        &self.workspace_index,
                        &narrow,
                        &wide,
                    ) == Some(true)
            })
    }

    /// The JVM name of the class a parameter's projected type, `parameter`,
    /// certainly names, read in the class `container` as
    /// [`Self::argument_fit`] reads a class parameter; a primitive is its
    /// boxed class, as an `Int` is a `java.lang.Integer`. None for any other
    /// parameter: one of `own`, the method's type parameters, an array, a
    /// generic, nullable or function type, and a simple name the
    /// declaration's scope does not certainly bind.
    fn parameter_class(
        &self,
        parameter: &str,
        own: &[TypeParameterInfo],
        container: &str,
    ) -> Option<String> {
        let parameter = parameter.trim();
        if parameter.is_empty()
            || parameter.contains(['(', '[', '<', '>', '*', '@', '?', ' '])
            || own.iter().any(|own| own.name.as_ref() == parameter)
        {
            return None;
        }
        match self.known_type(parameter, Some(container))? {
            Type::Primitive(primitive) => Some(boxed_class_name(primitive).to_string()),
            Type::Class(class) if names_in_declaring_scope(&class, parameter, container) => {
                Some(jvm_class_name(&class))
            }
            _ => None,
        }
    }

    /// The projected type `method` returns, its own type parameters bound to
    /// `own_arguments`, one for each or none. None when it has no projection,
    /// and, with no `own_arguments`, when it returns a type parameter of its
    /// own.
    fn overload_type(
        &self,
        receiver: &ClassType,
        arguments: &[String],
        method: &ProjectedEffectiveMethod,
        own_arguments: &[Type],
    ) -> Option<Type> {
        let projection = method.projection.as_ref()?;
        let declared = projection.return_type.as_deref()?;
        let head = declared.split(['<', '[', '?']).next().unwrap_or("").trim();
        if own_arguments.is_empty()
            && projection
                .type_parameters
                .iter()
                .any(|parameter| parameter.name.as_ref() == head)
        {
            return None;
        }
        Some(self.projected_member_type(
            receiver,
            arguments,
            &method.definition.symbol,
            declared,
            own_arguments,
        ))
    }

    /// The arguments a call whose value arguments have the types
    /// `call_arguments` gives `method`'s own type parameters, one
    /// for each, as [`Self::argument_fit`] binds them, or none when an
    /// argument binds none of some, and whether every argument certainly
    /// fits its parameter.
    ///
    /// None, ruling the overload out, when it cannot take that many
    /// arguments, or an argument certainly does not fit its parameter. With
    /// no projection, a vararg, or a default value the call does not
    /// replace, only the count is read, since an argument's parameter is not
    /// known, and the fit is not certain.
    fn call_own_arguments(
        &self,
        method: &ProjectedEffectiveMethod,
        call_arguments: &[Type],
    ) -> Option<(Vec<Type>, bool)> {
        let Some(projection) = method.projection.as_ref() else {
            return Some((Vec::new(), false));
        };
        let symbol = &method.definition.symbol;
        let parameters = &projection.parameters;
        let variadic = parameters
            .iter()
            .any(ProjectedCallableParameter::is_variadic);
        let defaults = symbol.kotlin.as_ref().map_or(0, |kotlin| {
            kotlin
                .parameters
                .iter()
                .filter(|parameter| parameter.has_default)
                .count()
        });
        let required = parameters
            .len()
            .saturating_sub(usize::from(variadic) + defaults);
        let count = call_arguments.len();
        if count < required || (!variadic && count > parameters.len()) {
            return None;
        }
        if variadic || count != parameters.len() {
            return Some((Vec::new(), false));
        }
        let container = symbol.container.as_deref().unwrap_or_default();
        let mut bound = Vec::new();
        let mut certain = true;
        for (parameter, argument) in parameters.iter().zip(call_arguments) {
            match self.argument_fit(
                &parameter.type_name,
                argument,
                &symbol.type_parameters,
                container,
                &mut bound,
            ) {
                ArgumentFit::Fits => {}
                ArgumentFit::Unknown => certain = false,
                ArgumentFit::Mismatch => return None,
            }
        }
        let own_arguments = symbol
            .type_parameters
            .iter()
            .map(|parameter| {
                bound
                    .iter()
                    .find(|(name, _)| *name == parameter.name.as_ref())
                    .map(|(_, ty)| ty.clone())
            })
            .collect::<Option<_>>()
            .unwrap_or_default();
        Some((own_arguments, certain))
    }

    /// Whether a value of type `argument` can be passed to a parameter whose
    /// projected type is `parameter`, read in the class `container`, a
    /// parameter that is one of `own`, the method's type parameters, bound in
    /// `bound` to the argument it takes.
    ///
    /// It fits when the parameter is such a type parameter with no bound
    /// and the argument is a class, primitive or array type; when the
    /// parameter is an array whose element the argument's element fits, a
    /// type parameter there bound to the element; when the parameter is a
    /// class the argument's class is or extends, as the index tells (a
    /// primitive's class is its boxed class, and an array extends only
    /// `Object`, `Cloneable` and `Serializable`); and when the parameter is
    /// a primitive the argument is, or whose boxed class it is.
    ///
    /// Unknown, ruling nothing out, when the argument's type is not known or
    /// is a function type, which a functional interface can take; when the
    /// argument's class has a supertype the index does not resolve; when two
    /// numeric primitives differ, since a literal takes the parameter's type;
    /// for a type parameter with a bound or already bound to another type;
    /// for a class named by a simple name its declaration's scope does not
    /// certainly bind, as [`names_in_declaring_scope`] tells; and for a
    /// parameter this does not read, a function type among them.
    fn argument_fit(
        &self,
        parameter: &str,
        argument: &Type,
        own: &[TypeParameterInfo],
        container: &str,
        bound: &mut Vec<(String, Type)>,
    ) -> ArgumentFit {
        let parameter = parameter.trim();
        let parameter = parameter.strip_suffix('?').unwrap_or(parameter).trim_end();
        if let Some(element) = parameter.strip_suffix("[]") {
            return self.array_argument_fit(element, argument, own, container, bound);
        }
        if let Some(own_parameter) = own
            .iter()
            .find(|own_parameter| own_parameter.name.as_ref() == parameter)
        {
            let unbounded = own_parameter.bounds.iter().all(|bound| {
                matches!(
                    bound.trim(),
                    "java.lang.Object" | "Object" | "Any" | "Any?" | "kotlin.Any" | "kotlin.Any?"
                )
            });
            if !unbounded || !is_projectable(argument) {
                return ArgumentFit::Unknown;
            }
            if let Some((_, earlier)) = bound.iter().find(|(name, _)| name == parameter) {
                return if earlier == argument {
                    ArgumentFit::Fits
                } else {
                    ArgumentFit::Unknown
                };
            }
            bound.push((parameter.to_string(), argument.clone()));
            return ArgumentFit::Fits;
        }
        let head = split_type_arguments(parameter).map_or(parameter, |(head, _)| head);
        if head.is_empty() || head.contains(['(', '[', '<', '>', '*', '@', '?', ' ']) {
            return ArgumentFit::Unknown;
        }
        match self.known_type(head, Some(container)) {
            Some(Type::Primitive(primitive)) => self.primitive_argument_fit(primitive, argument),
            Some(Type::Class(class)) if names_in_declaring_scope(&class, head, container) => {
                self.class_argument_fit(&jvm_class_name(&class), argument)
            }
            _ => ArgumentFit::Unknown,
        }
    }

    /// [`Self::argument_fit`] for an array parameter whose element is
    /// `element`. A Kotlin `Array<Int>` is an `Integer[]`, not an `int[]`,
    /// which is an `IntArray`.
    fn array_argument_fit(
        &self,
        element: &str,
        argument: &Type,
        own: &[TypeParameterInfo],
        container: &str,
        bound: &mut Vec<(String, Type)>,
    ) -> ArgumentFit {
        let element = element.trim();
        let Type::Class(class) = argument else {
            return match argument {
                Type::Primitive(_) => ArgumentFit::Mismatch,
                _ => ArgumentFit::Unknown,
            };
        };
        if let Some(primitive) = primitive_array_element(class) {
            return if java_primitive(element) == Some(primitive) {
                ArgumentFit::Fits
            } else {
                ArgumentFit::Mismatch
            };
        }
        if !is_kotlin_array(class) {
            return self.unrelated_class_fit(class);
        }
        if java_primitive(element).is_some() {
            return ArgumentFit::Mismatch;
        }
        let actual = match class.type_arguments.as_slice() {
            [Type::Wildcard {
                upper_bound: Some(upper),
                lower_bound: None,
            }] => upper.as_ref(),
            [actual] => actual,
            _ => return ArgumentFit::Unknown,
        };
        self.argument_fit(element, actual, own, container, bound)
    }

    /// [`Self::argument_fit`] for a primitive parameter.
    fn primitive_argument_fit(&self, primitive: PrimitiveType, argument: &Type) -> ArgumentFit {
        match argument {
            Type::Primitive(actual) if *actual == primitive => ArgumentFit::Fits,
            Type::Primitive(actual)
                if is_numeric_primitive(*actual) && is_numeric_primitive(primitive) =>
            {
                ArgumentFit::Unknown
            }
            Type::Primitive(_) => ArgumentFit::Mismatch,
            Type::Class(class) if is_kotlin_array(class) => ArgumentFit::Mismatch,
            Type::Class(class) if jvm_class_name(class) == boxed_class_name(primitive) => {
                ArgumentFit::Fits
            }
            Type::Class(class) => self.unrelated_class_fit(class),
            _ => ArgumentFit::Unknown,
        }
    }

    /// [`Self::argument_fit`] for a parameter of the class `expected`, a JVM
    /// name.
    fn class_argument_fit(&self, expected: &str, argument: &Type) -> ArgumentFit {
        let actual = match argument {
            Type::Primitive(primitive) => boxed_class_name(*primitive).to_string(),
            Type::Class(class) if is_kotlin_array(class) => {
                return if matches!(
                    expected,
                    "java.lang.Object" | "java.lang.Cloneable" | "java.io.Serializable"
                ) {
                    ArgumentFit::Fits
                } else {
                    ArgumentFit::Mismatch
                };
            }
            Type::Class(class)
                if class.package.as_deref() == Some("kotlin")
                    && class.name.starts_with("Function") =>
            {
                return ArgumentFit::Unknown;
            }
            Type::Class(class) => {
                let imported = self.imported_receiver_class(class);
                jvm_class_name(imported.as_ref().unwrap_or(class))
            }
            _ => return ArgumentFit::Unknown,
        };
        if actual == expected || expected == "java.lang.Object" {
            return ArgumentFit::Fits;
        }
        match crate::kotlin_certain_errors::indexed_subtype(
            &self.workspace_index,
            &actual,
            expected,
        ) {
            Some(true) => ArgumentFit::Fits,
            Some(false) => ArgumentFit::Mismatch,
            None => ArgumentFit::Unknown,
        }
    }

    /// The fit of an argument of the class `class` where a primitive or an
    /// array is expected: none when the index has the class, unknown when
    /// it does not, as for a name that does not resolve.
    fn unrelated_class_fit(&self, class: &ClassType) -> ArgumentFit {
        let imported = self.imported_receiver_class(class);
        if self
            .workspace_index
            .has_class(&jvm_class_name(imported.as_ref().unwrap_or(class)))
        {
            ArgumentFit::Mismatch
        } else {
            ArgumentFit::Unknown
        }
    }

    /// An inherited member's projected type, `declared`. The projection
    /// spells a receiver argument as `arguments` does, so a type that is one
    /// of them, or a generic type with one of them among its arguments
    /// (`Stream<kotlin.String>`), has that argument there, with its own
    /// arguments kept, as [`Self::receiver_substituted_type`] gives it for a
    /// member of the receiver's class. The member's own type parameters are
    /// bound to `own_arguments`, one for each or none. Any other type is read
    /// in the declaring class.
    fn projected_member_type(
        &self,
        receiver: &ClassType,
        arguments: &[String],
        member: &hyperion_workspace::Symbol,
        declared: &str,
        own_arguments: &[Type],
    ) -> Type {
        let owner = member.container.as_deref();
        let bindings: Vec<(&str, &Type)> = arguments
            .iter()
            .map(String::as_str)
            .zip(&receiver.type_arguments)
            .collect();
        if let Some(substituted) = owner.and_then(|owner| {
            self.substituted_type(declared, own_arguments, &bindings, member, owner)
        }) {
            return substituted;
        }
        let package = owner.and_then(|owner| self.split_fqn(owner).1);
        self.parse_type_string(declared, package.as_deref(), owner)
    }

    /// The Java getter behind a Kotlin synthetic property: `c.radius` reads
    /// `getRadius()`, and `t.isActive` reads `isActive()`. Only public,
    /// non-static, zero-argument members qualify.
    fn java_getter_symbol(
        &self,
        class_fqn: &str,
        property: &str,
    ) -> Option<std::sync::Arc<hyperion_workspace::Symbol>> {
        for getter in hyperion_types::jvm_names::java_getter_names(property) {
            let target = hyperion_workspace::SymbolTarget::new(
                format!("{class_fqn}.{getter}"),
                hyperion_workspace::DeclarationNamespace::Callable,
            );
            if let Some(symbol) = self.workspace_index.get_symbol_target(&target) {
                if symbol.parameters.is_empty()
                    && !symbol.modifiers.is_static
                    && !symbol.modifiers.is_private
                    && symbol.is_method()
                {
                    return Some(symbol);
                }
            }
        }
        None
    }

    /// Parse a type string (from workspace index) into a Type
    ///
    /// Examples:
    /// - "String" -> Type::Class(ClassType { name: "String", package: Some("kotlin") })
    /// - "Int" -> Type::Primitive(PrimitiveType::Int)
    /// - "User" -> Type::Class(ClassType { name: "User", package: current_package })
    fn parse_type_string(
        &self,
        type_str: &str,
        current_package: Option<&str>,
        current_container_fqn: Option<&str>,
    ) -> Type {
        let type_str = type_str.trim();
        let type_str = type_str.strip_suffix('?').unwrap_or(type_str);
        let base = type_str.split('<').next().unwrap_or(type_str).trim();

        // Final fallback: keep simple type name with package hint.
        self.known_type(base, current_container_fqn)
            .unwrap_or_else(|| {
                Type::Class(ClassType {
                    name: base.to_string(),
                    package: current_package.map(String::from),
                    type_arguments: vec![],
                    is_diamond: false,
                })
            })
    }

    /// The type that `base`, a type name without arguments, names: a
    /// primitive, `String`, a class nested in `current_container_fqn`, a
    /// qualified class, or a class the file resolves the name to. None when
    /// it names none of these, where [`Self::parse_type_string`] guesses a
    /// class of that name.
    fn known_type(&self, base: &str, current_container_fqn: Option<&str>) -> Option<Type> {
        // Check for primitive types first
        match base {
            "Int" => return Some(Type::Primitive(PrimitiveType::Int)),
            "Long" => return Some(Type::Primitive(PrimitiveType::Long)),
            "Short" => return Some(Type::Primitive(PrimitiveType::Short)),
            "Byte" => return Some(Type::Primitive(PrimitiveType::Byte)),
            "Float" => return Some(Type::Primitive(PrimitiveType::Float)),
            "Double" => return Some(Type::Primitive(PrimitiveType::Double)),
            "Boolean" => return Some(Type::Primitive(PrimitiveType::Boolean)),
            "Char" => return Some(Type::Primitive(PrimitiveType::Char)),
            // Java spellings, as they appear in the index for Java members and
            // for the JVM view of Kotlin declarations.
            "int" => return Some(Type::Primitive(PrimitiveType::Int)),
            "long" => return Some(Type::Primitive(PrimitiveType::Long)),
            "short" => return Some(Type::Primitive(PrimitiveType::Short)),
            "byte" => return Some(Type::Primitive(PrimitiveType::Byte)),
            "float" => return Some(Type::Primitive(PrimitiveType::Float)),
            "double" => return Some(Type::Primitive(PrimitiveType::Double)),
            "boolean" => return Some(Type::Primitive(PrimitiveType::Boolean)),
            "char" => return Some(Type::Primitive(PrimitiveType::Char)),
            "java.lang.String" | "String" => {
                return Some(Type::Class(ClassType {
                    name: "String".into(),
                    package: Some("kotlin".into()),
                    type_arguments: vec![],
                    is_diamond: false,
                }))
            }
            _ => {}
        }

        // Prefer class-like symbols from workspace (imports/current package/simple-name).
        if let Some(container_fqn) = current_container_fqn {
            // Nested type in current container/class scope, e.g. `Builder` in `User`.
            let nested = format!("{container_fqn}.{base}");
            if self.workspace_index.has_class(&nested) {
                let (simple_name, package) = self.split_fqn(&nested);
                return Some(Type::Class(ClassType {
                    name: simple_name,
                    package,
                    type_arguments: vec![],
                    is_diamond: false,
                }));
            }
        }

        if base.contains('.') && self.workspace_index.has_class(base) {
            let (simple_name, package) = self.split_fqn(base);
            return Some(Type::Class(ClassType {
                name: simple_name,
                package,
                type_arguments: vec![],
                is_diamond: false,
            }));
        }

        let resolved_fqn = self.resolve_class_fqn_from_name(base)?;
        let (simple_name, package) = self.split_fqn(&resolved_fqn);
        Some(Type::Class(ClassType {
            name: simple_name,
            package,
            type_arguments: vec![],
            is_diamond: false,
        }))
    }

    fn resolve_class_fqn_from_name(&self, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }

        // Fast path for already-qualified names.
        if let Some(sym) = self.workspace_index.get_symbol(name) {
            if is_class_like_symbol_kind(sym.kind) {
                return Some(sym.fqn.to_string());
            }
        }

        let resolution = self
            .workspace_index
            .resolve_type_name(self.file_id, &[], name);
        let hyperion_workspace::TypeNameResolution::Exact(fqn) = resolution else {
            return None;
        };
        Some(fqn)
    }

    /// Resolve the return type of a method
    fn resolve_method_return_type(&self, receiver_type: &Type, method_name: &str) -> Type {
        self.resolve_member_in_namespace(
            receiver_type,
            method_name,
            hyperion_workspace::DeclarationNamespace::Callable,
            &[],
            None,
        )
    }

    /// Infer the type of a binary expression
    fn infer_binary_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        // Find the operator to determine result type
        let mut op_kind = None;
        let mut operands = Vec::new();

        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) => {
                    // Capture the operator
                    if matches!(
                        t.kind(),
                        KtKind::PLUS
                            | KtKind::MINUS
                            | KtKind::STAR
                            | KtKind::SLASH
                            | KtKind::PERCENT
                            | KtKind::EQEQ
                            | KtKind::BANGEQ
                            | KtKind::LT
                            | KtKind::GT
                            | KtKind::LTEQ
                            | KtKind::GTEQ
                            | KtKind::AMPAMP
                            | KtKind::PIPEPIPE
                    ) {
                        op_kind = Some(t.kind());
                    } else if !t.text().trim().is_empty() && !is_structural_token(t.kind()) {
                        // Identifiers and literals are bare tokens in Kotlin's tree.
                        operands.push(NodeOrToken::Token(t));
                    }
                }
                NodeOrToken::Node(n) => {
                    operands.push(NodeOrToken::Node(n));
                }
            }
        }

        // Infer based on operator
        match op_kind {
            // Comparison and logical operators return Boolean
            Some(
                KtKind::EQEQ
                | KtKind::BANGEQ
                | KtKind::LT
                | KtKind::GT
                | KtKind::LTEQ
                | KtKind::GTEQ
                | KtKind::AMPAMP
                | KtKind::PIPEPIPE,
            ) => Type::Primitive(PrimitiveType::Boolean),

            // Arithmetic operators - infer from operands
            Some(KtKind::PLUS | KtKind::MINUS | KtKind::STAR | KtKind::SLASH | KtKind::PERCENT) => {
                // Widen across both operands (Kotlin numeric promotion).
                let operand_types: Vec<Type> = operands
                    .iter()
                    .map(|operand| self.infer_element(operand, resolver))
                    .collect();
                operand_types
                    .first()
                    .map_or(Type::Unknown, |first| match first {
                        Type::Primitive(p) if is_numeric_primitive(*p) => {
                            least_upper_bound(&operand_types)
                        }
                        other => other.clone(),
                    })
            }

            _ => Type::Unknown,
        }
    }

    /// Infer the type of a unary expression
    fn infer_unary_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        self.infer_first_child_expr(node, resolver)
    }

    /// Infer the type of an if expression as the LUB of its branches.
    fn infer_if_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let mut branch_types = Vec::new();
        let mut depth = 0_i32;
        let mut past_condition = false;

        for element in node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &element {
                match t.kind() {
                    KtKind::L_PAREN => depth += 1,
                    KtKind::R_PAREN => {
                        depth -= 1;
                        if depth == 0 {
                            past_condition = true;
                        }
                    }
                    _ => {}
                }
                if !past_condition || t.text().trim().is_empty() || is_structural_token(t.kind()) {
                    continue;
                }
            } else if !past_condition || depth > 0 {
                continue;
            }

            if let NodeOrToken::Token(t) = &element {
                if t.kind() == KtKind::IF_KW || t.kind() == KtKind::ELSE_KW {
                    continue;
                }
            }

            let ty = self.infer_element(&element, resolver);
            if !matches!(ty, Type::Unknown) {
                branch_types.push(ty);
            }
        }

        least_upper_bound(&branch_types)
    }

    /// Split a fully qualified name into simple name and package
    #[allow(clippy::unused_self)]
    fn split_fqn(&self, fqn: &str) -> (String, Option<String>) {
        fqn.rfind('.').map_or_else(
            || (fqn.to_string(), None),
            |last_dot| {
                (
                    fqn[last_dot + 1..].to_string(),
                    Some(fqn[..last_dot].to_string()),
                )
            },
        )
    }

    /// Infer the type of implicit 'it' parameter from lambda context
    ///
    /// This walks up the AST to find the parent method call and infers the
    /// lambda parameter type based on the receiver type and method signature.
    ///
    /// For example, in `list.filter { it > 0 }`:
    /// - Receiver is `list` of type `List<Int>`
    /// - Method is `filter`
    /// - Lambda parameter type is `Int` (element type of List<Int>)
    fn infer_implicit_it_type_at(
        &self,
        context: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let lambda = enclosing_lambda(context)?;

        // An explicit parameter list means `it` is not in scope for this lambda.
        if lambda_declared_parameters(&lambda).is_some() {
            return None;
        }

        self.lambda_parameter_type(&lambda, resolver)
    }

    /// The type bound to the lambda's single implicit parameter (`it`).
    fn lambda_parameter_type(
        &self,
        lambda: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let call_node = lambda_owner_call(lambda)?;
        let (callee, receiver_type) = self.call_callee_and_receiver(&call_node, resolver);
        let callee = callee?;

        // Scope functions that pass the receiver as `it`.
        if matches!(
            callee.as_str(),
            "let" | "also" | "takeIf" | "takeUnless" | "onEach"
        ) {
            let receiver = receiver_type?;
            if !matches!(receiver, Type::Unknown) {
                return Some(receiver);
            }
            return None;
        }

        // `with`/`apply`/`run` bind `this`, not `it`.
        if matches!(callee.as_str(), "with" | "apply" | "run") {
            return None;
        }

        if let Some(receiver) = receiver_type
            .as_ref()
            .filter(|t| !matches!(t, Type::Unknown))
        {
            // Reuse the shared Kotlin stdlib signature registry.
            if let Some(kt) = hyperion_kotlin::inference::infer_lambda_it_type(
                &semantics_type_to_kotlin(receiver),
                &callee,
            ) {
                let ty = kotlin_type_to_semantics(&kt);
                if !matches!(ty, Type::Unknown) {
                    return Some(ty);
                }
            }

            if let Some(ty) = self.infer_lambda_param_from_receiver_type(receiver, &callee) {
                return Some(ty);
            }
        }

        // Fallback: source-based inference for local declarations like
        // `val users = listOf(User(...)); users.map { it.foo() }`.
        let receiver_name = call_receiver_name(&call_node)?;
        self.infer_lambda_param_from_source(&receiver_name, &callee, &call_node)
    }

    /// The receiver type (`this`) inside the lambda enclosing `context`, if the
    /// lambda is the argument of a function taking a function-type-with-receiver.
    ///
    /// Handles the stdlib scope functions plus user/DSL functions whose declared
    /// parameter type is spelled `Receiver.() -> R` in the workspace index.
    fn enclosing_lambda_receiver_type(
        &self,
        context: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let mut lambda = enclosing_lambda(context);

        while let Some(current) = lambda {
            if let Some(ty) = self.lambda_receiver_type(&current, resolver) {
                if !matches!(ty, Type::Unknown) {
                    return Some(ty);
                }
            }
            lambda = current.parent().as_ref().and_then(enclosing_lambda);
        }

        None
    }

    fn lambda_receiver_type(
        &self,
        lambda: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let call_node = lambda_owner_call(lambda)?;
        let (callee, receiver_type) = self.call_callee_and_receiver(&call_node, resolver);
        let callee = callee?;

        match callee.as_str() {
            // `with(receiver) { this: receiver }` - receiver is the first argument
            "with" => self.first_value_argument_type(&call_node, resolver),
            // `T.apply { this: T }` / `T.run { this: T }`
            "apply" | "run" => receiver_type.filter(|t| !matches!(t, Type::Unknown)),
            // `buildString { this: StringBuilder }`
            "buildString" => Some(Type::Class(ClassType {
                name: "StringBuilder".into(),
                package: Some("kotlin.text".into()),
                type_arguments: vec![],
                is_diamond: false,
            })),
            // User-defined DSL builders: `fun html(block: HTML.() -> Unit)`
            other => self.declared_receiver_lambda_param(other),
        }
    }

    /// Look up `callee` and, if one of its parameters is declared as a function
    /// type with receiver (`Foo.() -> Bar`), return `Foo`.
    ///
    /// The workspace index does not currently record parameter type strings for
    /// Kotlin functions, so the declaring file is re-read and scanned. Results
    /// are memoised per engine instance because the semantic walker asks this
    /// question once per call expression inside a lambda.
    fn declared_receiver_lambda_param(&self, callee: &str) -> Option<Type> {
        if let Some(cached) = self.dsl_receiver_cache.borrow().get(callee) {
            return cached.clone();
        }

        let resolved = self.lookup_declared_receiver_lambda_param(callee);
        self.dsl_receiver_cache
            .borrow_mut()
            .insert(callee.to_string(), resolved.clone());
        resolved
    }

    fn lookup_declared_receiver_lambda_param(&self, callee: &str) -> Option<Type> {
        let imports = self.workspace_index.get_imports(self.file_id)?;
        let callable = |fqn: &str| {
            self.workspace_index
                .find_symbol_definition(&SymbolTarget::new(fqn, DeclarationNamespace::Callable))
        };

        let definitions = if let Some(imported) = imports.explicit.get(callee) {
            callable(imported).into_iter().collect::<Vec<_>>()
        } else {
            let same_package = imports.current_package.as_ref().map_or_else(
                || callee.to_owned(),
                |package| format!("{package}.{callee}"),
            );
            if let Some(definition) = callable(&same_package) {
                vec![definition]
            } else {
                imports
                    .wildcards
                    .iter()
                    .filter_map(|package| callable(&format!("{package}.{callee}")))
                    .collect()
            }
        };

        let mut receiver_spellings = Vec::new();
        let mut source_declarations =
            std::collections::HashSet::from([(self.file_id, callee.to_owned())]);
        for definition in definitions {
            for parameter in &definition.symbol.parameters {
                if let Some(receiver) = extract_function_type_receiver(parameter) {
                    receiver_spellings.push(receiver.to_owned());
                }
            }
            source_declarations.insert((
                definition.location.file_id,
                definition.symbol.simple_name.to_string(),
            ));
        }

        for (file_id, declaration_name) in source_declarations {
            let Some(source) = self.workspace_index.get_file_contents(file_id) else {
                continue;
            };
            receiver_spellings.extend(function_receiver_lambda_params(
                source.as_ref(),
                &declaration_name,
            ));
        }

        let mut receiver_types = receiver_spellings
            .into_iter()
            .map(|receiver| self.parse_type_string(&receiver, None, None))
            .filter(|receiver| !matches!(receiver, Type::Unknown));
        let first = receiver_types.next()?;
        receiver_types
            .all(|receiver| receiver == first)
            .then_some(first)
    }

    /// `(callee name, receiver type)` for a call expression.
    ///
    /// This deliberately avoids inferring any trailing lambda so that it can be
    /// called *from* lambda-body inference without recursing.
    fn call_callee_and_receiver(
        &self,
        call_node: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> (Option<String>, Option<Type>) {
        if let Some(member_access) = call_node.children().find(|child| {
            matches!(
                child.kind(),
                KtKind::MEMBER_ACCESS_EXPR | KtKind::SAFE_ACCESS_EXPR
            )
        }) {
            let Some((receiver, method_name)) = Self::split_member_access(&member_access) else {
                return (None, None);
            };
            let receiver_type = self.infer_element(&receiver, resolver);
            return (Some(method_name), Some(receiver_type));
        }

        (Self::callee_name_token(call_node), None)
    }

    /// Type of the first parenthesised argument of a call (lambdas excluded).
    fn first_value_argument_type(
        &self,
        call_node: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let mut depth = 0_i32;
        for element in call_node.children_with_tokens() {
            match &element {
                NodeOrToken::Token(t) => match t.kind() {
                    KtKind::L_PAREN => depth += 1,
                    KtKind::R_PAREN => return None,
                    _ => {
                        if depth > 0 && !t.text().trim().is_empty() {
                            let ty = self.infer_token(t, resolver);
                            if !matches!(ty, Type::Unknown) {
                                return Some(ty);
                            }
                        }
                    }
                },
                NodeOrToken::Node(n) => {
                    if depth > 0 && n.kind() != KtKind::LAMBDA_EXPR {
                        let ty = self.infer_expression(n, resolver);
                        if !matches!(ty, Type::Unknown) {
                            return Some(ty);
                        }
                    }
                }
            }
        }
        None
    }

    /// A lambda's type is `kotlin.FunctionN<P1, .., Pn, R>`.
    fn infer_lambda_expr(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let mut param_types: Vec<Type> = Vec::new();

        if let Some(params) = lambda_declared_parameters(node) {
            for (_, annotation) in &params {
                let ty = annotation
                    .as_ref()
                    .map_or(Type::Unknown, |ann| self.parse_type_string(ann, None, None));
                param_types.push(ty);
            }
        } else if let Some(it_type) = self.lambda_parameter_type(node, resolver) {
            param_types.push(it_type);
        }

        let return_type = self.infer_lambda_body_type(node, resolver);

        let mut type_arguments = param_types;
        let arity = type_arguments.len();
        type_arguments.push(return_type);

        Type::Class(ClassType {
            name: format!("Function{arity}"),
            package: Some("kotlin".into()),
            type_arguments,
            is_diamond: false,
        })
    }

    /// The value of a lambda body is its last expression.
    fn infer_lambda_body_type(&self, node: &KtSyntaxNode, resolver: &mut SymbolResolver) -> Type {
        let mut last: Option<NodeOrToken<KtSyntaxNode, hyperion_kotlin::SyntaxToken>> = None;
        let mut seen_arrow = lambda_declared_parameters(node).is_none();

        for element in node.children_with_tokens() {
            match &element {
                NodeOrToken::Token(t) => {
                    if t.kind() == KtKind::ARROW {
                        seen_arrow = true;
                        continue;
                    }
                    if !seen_arrow || t.text().trim().is_empty() || is_structural_token(t.kind()) {
                        continue;
                    }
                }
                NodeOrToken::Node(n) => {
                    // The parameter list node also swallows the `->`.
                    if n.kind() == KtKind::LAMBDA_PARAMETERS {
                        seen_arrow = true;
                        continue;
                    }
                    if !seen_arrow {
                        continue;
                    }
                }
            }
            last = Some(element);
        }

        last.map_or(Type::Unknown, |element| {
            self.infer_element(&element, resolver)
        })
    }

    /// `when (val x = e) { ... }` binds `x` for the duration of the `when` body.
    fn lookup_when_subject_binding(
        &self,
        name: &str,
        context: &KtSyntaxNode,
        offset: u32,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let mut current = Some(context.clone());

        while let Some(node) = current {
            if node.kind() == KtKind::WHEN_EXPR {
                if let Some(decl) = node
                    .children()
                    .find(|child| child.kind() == KtKind::PROPERTY_DECL)
                {
                    // Don't resolve the binding from inside its own initializer.
                    let decl_range = decl.text_range();
                    let inside_initializer = offset >= u32::from(decl_range.start())
                        && offset < u32::from(decl_range.end());

                    if !inside_initializer && property_decl_name(&decl).as_deref() == Some(name) {
                        return Some(self.infer_property_decl_initializer(&decl, resolver));
                    }
                }
            }
            current = node.parent();
        }

        None
    }

    /// Type of the initializer of a `PROPERTY_DECL` (`val x = <expr>`).
    fn infer_property_decl_initializer(
        &self,
        decl: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Type {
        // Explicit annotation wins: `val x: Foo = ...`
        if let Some(type_ref) = decl.children().find(|c| c.kind() == KtKind::TYPE_REF) {
            return self.parse_type_string(&type_ref.text().to_string(), None, None);
        }

        let mut seen_eq = false;
        for element in decl.children_with_tokens() {
            if let NodeOrToken::Token(t) = &element {
                if t.kind() == KtKind::EQ {
                    seen_eq = true;
                    continue;
                }
                if !seen_eq || t.text().trim().is_empty() {
                    continue;
                }
            } else if !seen_eq {
                continue;
            }

            let ty = self.infer_element(&element, resolver);
            if !matches!(ty, Type::Unknown) {
                return ty;
            }
        }

        Type::Unknown
    }

    /// Explicitly declared lambda parameters: `{ a: Int -> ... }` / `{ a, b -> ... }`.
    fn lookup_lambda_parameter(
        &self,
        name: &str,
        context: &KtSyntaxNode,
        resolver: &mut SymbolResolver,
    ) -> Option<Type> {
        let mut lambda = enclosing_lambda(context);

        while let Some(current) = lambda {
            if let Some(params) = lambda_declared_parameters(&current) {
                for (index, (param_name, annotation)) in params.iter().enumerate() {
                    if param_name != name {
                        continue;
                    }
                    if let Some(annotation) = annotation {
                        return Some(self.parse_type_string(annotation, None, None));
                    }
                    // Unannotated: take it from the expected lambda type.
                    if index == 0 {
                        if let Some(ty) = self.lambda_parameter_type(&current, resolver) {
                            return Some(ty);
                        }
                    }
                    return Some(Type::Unknown);
                }
            }
            lambda = current.parent().as_ref().and_then(enclosing_lambda);
        }

        None
    }

    fn infer_lambda_param_from_receiver_type(
        &self,
        receiver_type: &Type,
        method_name: &str,
    ) -> Option<Type> {
        // Collection-like lambda methods where `it` corresponds to element type.
        let needs_element_type = matches!(
            method_name,
            "filter"
                | "filterNot"
                | "forEach"
                | "any"
                | "all"
                | "none"
                | "count"
                | "map"
                | "flatMap"
                | "find"
                | "first"
                | "firstOrNull"
                | "last"
                | "lastOrNull"
        );

        if !needs_element_type {
            return None;
        }

        if let Type::Class(class_type) = receiver_type {
            if let Some(first_arg) = class_type.type_arguments.first() {
                return Some(first_arg.clone());
            }
        }

        None
    }

    fn infer_lambda_param_from_source(
        &self,
        receiver_name: &str,
        method_name: &str,
        call_node: &KtSyntaxNode,
    ) -> Option<Type> {
        let needs_element_type = matches!(
            method_name,
            "filter"
                | "filterNot"
                | "forEach"
                | "any"
                | "all"
                | "none"
                | "count"
                | "map"
                | "flatMap"
                | "find"
                | "first"
                | "firstOrNull"
                | "last"
                | "lastOrNull"
        );
        if !needs_element_type {
            return None;
        }

        let source = self.workspace_index.get_file_contents(self.file_id)?;
        let call_pos: usize = call_node.text_range().start().into();
        let prefix = source
            .get(..call_pos.min(source.len()))
            .unwrap_or(source.as_str());

        let mut inferred_type_name: Option<String> = None;

        for line in prefix.lines().rev() {
            let trimmed = line.trim();
            if !(trimmed.starts_with("val ") || trimmed.starts_with("var ")) {
                continue;
            }

            let Some(eq_idx) = trimmed.find('=') else {
                continue;
            };
            let (lhs, rhs) = trimmed.split_at(eq_idx);
            let rhs = rhs.trim_start_matches('=').trim();

            // Parse declaration name from lhs.
            let lhs = lhs
                .trim_start_matches("val ")
                .trim_start_matches("var ")
                .trim();
            let decl_name = lhs
                .split(|c: char| c == ':' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if decl_name != receiver_name {
                continue;
            }

            // 1) Explicit type annotation: val users: List<User> = ...
            if let Some(colon_idx) = lhs.find(':') {
                let annotated = lhs[colon_idx + 1..].trim();
                if let Some(element) = extract_first_generic_arg(annotated) {
                    inferred_type_name = Some(element.to_string());
                } else {
                    inferred_type_name = Some(annotated.to_string());
                }
            }

            // 2) Constructor-like collection initialization:
            //    listOf(User(...)), mutableListOf<User>(), setOf(...)
            if inferred_type_name.is_none() {
                if let Some(type_arg) = extract_type_arg_from_collection_ctor(rhs) {
                    inferred_type_name = Some(type_arg.to_string());
                } else if let Some(first_ctor) = extract_first_constructor_name(rhs) {
                    inferred_type_name = Some(first_ctor.to_string());
                }
            }

            break;
        }

        let type_name = inferred_type_name?;
        let imports = self.workspace_index.get_imports(self.file_id);
        let current_package = imports.as_ref().and_then(|m| m.current_package.as_deref());
        Some(self.parse_type_string(&type_name, current_package, None))
    }
}

/// Type of a literal token, if the token is a literal.
fn literal_token_type(kind: KtKind) -> Option<Type> {
    Some(match kind {
        KtKind::INT_LITERAL => Type::Primitive(PrimitiveType::Int),
        KtKind::LONG_LITERAL => Type::Primitive(PrimitiveType::Long),
        KtKind::FLOAT_LITERAL => Type::Primitive(PrimitiveType::Float),
        KtKind::DOUBLE_LITERAL => Type::Primitive(PrimitiveType::Double),
        KtKind::CHAR_LITERAL => Type::Primitive(PrimitiveType::Char),
        KtKind::TRUE_KW | KtKind::FALSE_KW => Type::Primitive(PrimitiveType::Boolean),
        // String templates ("a $b c") are lexed as a single STRING_LITERAL and
        // are always of type kotlin.String regardless of the interpolations.
        KtKind::STRING_LITERAL | KtKind::RAW_STRING_LITERAL => Type::Class(ClassType {
            name: "String".into(),
            package: Some("kotlin".into()),
            type_arguments: vec![],
            is_diamond: false,
        }),
        KtKind::NULL_KW => Type::Null,
        _ => return None,
    })
}

/// Punctuation and keywords that never carry a value on their own.
const fn is_structural_token(kind: KtKind) -> bool {
    matches!(
        kind,
        KtKind::L_BRACE
            | KtKind::R_BRACE
            | KtKind::L_PAREN
            | KtKind::R_PAREN
            | KtKind::SEMICOLON
            | KtKind::COMMA
            | KtKind::ARROW
            | KtKind::COLON
            | KtKind::EQ
            | KtKind::PLUSEQ
            | KtKind::MINUSEQ
            | KtKind::STAREQ
            | KtKind::SLASHEQ
            | KtKind::PERCENTEQ
            | KtKind::VAL_KW
            | KtKind::VAR_KW
            | KtKind::ELSE_KW
            | KtKind::IS_KW
            | KtKind::WHEN_KW
            | KtKind::TRY_KW
            | KtKind::CATCH_KW
            | KtKind::FINALLY_KW
    )
}

/// Nearest enclosing `LAMBDA_EXPR`, starting at `node` itself.
fn enclosing_lambda(node: &KtSyntaxNode) -> Option<KtSyntaxNode> {
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if n.kind() == KtKind::LAMBDA_EXPR {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// The `CALL_EXPR` that a lambda is an argument of, if any.
fn lambda_owner_call(lambda: &KtSyntaxNode) -> Option<KtSyntaxNode> {
    let mut current = lambda.parent();
    while let Some(node) = current {
        match node.kind() {
            KtKind::CALL_EXPR => return Some(node),
            // A lambda stored in a variable or nested in another lambda has no
            // owning call.
            KtKind::LAMBDA_EXPR | KtKind::FUNCTION_DECL | KtKind::BLOCK => return None,
            // `{ .. }.also { .. }`: the lambda is the *receiver* of the member
            // access, not an argument of the enclosing call. Treating that call
            // as its owner makes receiver inference recurse into the lambda again.
            KtKind::MEMBER_ACCESS_EXPR | KtKind::SAFE_ACCESS_EXPR => return None,
            _ => current = node.parent(),
        }
    }
    None
}

/// Explicitly declared lambda parameters, as `(name, optional type annotation)`.
///
/// Returns `None` when the lambda has no `->`, i.e. when `it` is in scope.
fn lambda_declared_parameters(lambda: &KtSyntaxNode) -> Option<Vec<(String, Option<String>)>> {
    // Preferred shape: a dedicated LAMBDA_PARAMETERS node holding `a: Int ->`.
    if let Some(params_node) = lambda
        .children()
        .find(|child| child.kind() == KtKind::LAMBDA_PARAMETERS)
    {
        let params = collect_declared_parameters(&params_node);
        return (!params.is_empty()).then_some(params);
    }

    // Fallback shape: parameters spliced directly into the lambda before `->`.
    let has_arrow = lambda
        .children_with_tokens()
        .any(|element| matches!(&element, NodeOrToken::Token(t) if t.kind() == KtKind::ARROW));
    if !has_arrow {
        return None;
    }

    let params = collect_declared_parameters(lambda);
    (!params.is_empty()).then_some(params)
}

/// Collect `name` / `name: Type` pairs up to (and excluding) the `->`.
fn collect_declared_parameters(node: &KtSyntaxNode) -> Vec<(String, Option<String>)> {
    let mut params: Vec<(String, Option<String>)> = Vec::new();
    let mut pending_annotation = false;

    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Token(t) => match t.kind() {
                KtKind::ARROW => break,
                KtKind::L_BRACE | KtKind::COMMA => pending_annotation = false,
                KtKind::COLON => pending_annotation = true,
                _ if t.text().trim().is_empty() => {}
                _ if is_kotlin_identifier_token(&t) => {
                    if pending_annotation {
                        if let Some(last) = params.last_mut() {
                            last.1 = Some(t.text().to_string());
                        }
                        pending_annotation = false;
                    } else {
                        params.push((t.text().to_string(), None));
                    }
                }
                _ => {}
            },
            NodeOrToken::Node(n) => {
                if n.kind() == KtKind::TYPE_REF && pending_annotation {
                    if let Some(last) = params.last_mut() {
                        last.1 = Some(n.text().to_string());
                    }
                    pending_annotation = false;
                } else {
                    // A body statement before any `->`: no parameter list here.
                    break;
                }
            }
        }
    }

    params
}

/// The declared name of a `PROPERTY_DECL` (`val <name> ...`).
fn property_decl_name(decl: &KtSyntaxNode) -> Option<String> {
    let mut seen_keyword = false;
    for element in decl.children_with_tokens() {
        if let NodeOrToken::Token(t) = element {
            if matches!(t.kind(), KtKind::VAL_KW | KtKind::VAR_KW) {
                seen_keyword = true;
                continue;
            }
            if seen_keyword && is_kotlin_identifier_token(&t) {
                return Some(t.text().to_string());
            }
        }
    }
    None
}

/// The simple name of the receiver of a member-access call, if it is a plain
/// identifier (`users.map { .. }` -> `users`).
fn call_receiver_name(call_node: &KtSyntaxNode) -> Option<String> {
    let member_access = call_node.children().find(|child| {
        matches!(
            child.kind(),
            KtKind::MEMBER_ACCESS_EXPR | KtKind::SAFE_ACCESS_EXPR
        )
    })?;

    member_access
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(is_kotlin_identifier_token)
        .map(|t| t.text().to_string())
}

/// Find `fun <name>(... : Receiver.() -> R)` in `source` and return `Receiver`.
///
/// The Kotlin parser is reused rather than a regex so nested generics and
/// multi-parameter lists are handled the same way the rest of the pipeline
/// handles them.
fn function_receiver_lambda_params(source: &str, callee: &str) -> Vec<String> {
    let tree = hyperion_kotlin::parse(source);
    let root = KtSyntaxNode::new_root(tree.green_node.clone());
    let mut receivers = Vec::new();

    for decl in root
        .descendants()
        .filter(|node| node.kind() == KtKind::FUNCTION_DECL)
    {
        let name = decl
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|t| is_kotlin_identifier_token(t) && t.kind() != KtKind::FUN_KW)
            .map(|t| t.text().to_string());
        if name.as_deref() != Some(callee) {
            continue;
        }

        let Some(params) = decl
            .children()
            .find(|child| child.kind() == KtKind::PARAMETER_LIST)
        else {
            continue;
        };

        for param in params
            .children()
            .filter(|child| child.kind() == KtKind::PARAMETER)
        {
            let text = param.text().to_string();
            if let Some(receiver) = extract_function_type_receiver(&text) {
                receivers.push(receiver.to_string());
            }
        }
    }

    receivers.sort_unstable();
    receivers.dedup();
    receivers
}

/// For a declared parameter type such as `HTML.() -> Unit`, return `HTML`.
fn extract_function_type_receiver(type_name: &str) -> Option<&str> {
    let arrow = type_name.find("->")?;
    let head = &type_name[..arrow];
    // The receiver is whatever precedes the function type's parameter list.
    let paren = head.rfind('(')?;
    let receiver = head[..paren].trim_end().trim_end_matches('.').trim_end();
    // Drop a leading `name:` when the whole parameter declaration was passed in.
    let receiver = receiver.rsplit(':').next()?.trim();
    if receiver.is_empty() || receiver.contains(char::is_whitespace) {
        None
    } else {
        Some(receiver)
    }
}

/// Least upper bound of a set of branch types.
///
/// Pragmatic approximation of Kotlin's LUB:
/// 1. no usable branches -> `Unknown`
/// 2. all branches identical -> that type
/// 3. all branches numeric -> the widest numeric type (Kotlin promotion order)
/// 4. all branches the same class (ignoring type arguments) -> that class
/// 5. otherwise -> `kotlin.Any`
fn least_upper_bound(types: &[Type]) -> Type {
    let usable: Vec<&Type> = types
        .iter()
        .filter(|t| !matches!(t, Type::Unknown | Type::Null))
        .collect();

    let Some(first) = usable.first() else {
        return Type::Unknown;
    };

    if usable.iter().all(|t| *t == *first) {
        return (*first).clone();
    }

    // Numeric promotion.
    if usable
        .iter()
        .all(|t| matches!(t, Type::Primitive(p) if is_numeric_primitive(*p)))
    {
        let mut widest = PrimitiveType::Byte;
        for ty in &usable {
            if let Type::Primitive(p) = ty {
                if numeric_rank(*p) > numeric_rank(widest) {
                    widest = *p;
                }
            }
        }
        return Type::Primitive(widest);
    }

    // Same class, differing type arguments -> erase to the raw class.
    if let Type::Class(first_class) = first {
        let same_class = usable.iter().all(|t| match t {
            Type::Class(c) => c.name == first_class.name && c.package == first_class.package,
            _ => false,
        });
        if same_class {
            return Type::Class(ClassType {
                name: first_class.name.clone(),
                package: first_class.package.clone(),
                type_arguments: vec![],
                is_diamond: false,
            });
        }
    }

    Type::Class(ClassType {
        name: "Any".into(),
        package: Some("kotlin".into()),
        type_arguments: vec![],
        is_diamond: false,
    })
}

const fn is_numeric_primitive(p: PrimitiveType) -> bool {
    matches!(
        p,
        PrimitiveType::Byte
            | PrimitiveType::Short
            | PrimitiveType::Int
            | PrimitiveType::Long
            | PrimitiveType::Float
            | PrimitiveType::Double
    )
}

const fn numeric_rank(p: PrimitiveType) -> u8 {
    match p {
        PrimitiveType::Byte => 1,
        PrimitiveType::Short => 2,
        PrimitiveType::Int => 3,
        PrimitiveType::Long => 4,
        PrimitiveType::Float => 5,
        PrimitiveType::Double => 6,
        _ => 0,
    }
}

/// Bridge into the shared `hyperion-kotlin` type model.
///
/// Reuses the stdlib signature registry and its `TypeUnifier` instead of
/// duplicating them: converts an inferred semantic [`Type`] into the Kotlin
/// surface type used for display (`KotlinType::format()`), so inference
/// results render in the same house style as types read straight out of the
/// syntax tree.
pub fn semantics_type_to_kotlin(ty: &Type) -> hyperion_kotlin::KotlinType {
    use hyperion_kotlin::types::TypeArgument;
    use hyperion_kotlin::KotlinType;

    match ty {
        Type::Primitive(p) => KotlinType::simple(format!("kotlin.{}", primitive_kotlin_name(*p))),
        Type::Class(c) => {
            if c.name == "Any" {
                return KotlinType::Any;
            }
            if c.name == "Unit" {
                return KotlinType::Unit;
            }
            if let Some(arity) = c
                .name
                .strip_prefix("Function")
                .and_then(|n| n.parse::<usize>().ok())
            {
                let mut params: Vec<KotlinType> = c
                    .type_arguments
                    .iter()
                    .map(semantics_type_to_kotlin)
                    .collect();
                let ret = if params.len() > arity {
                    params.remove(arity)
                } else {
                    KotlinType::Unknown
                };
                params.truncate(arity);
                return KotlinType::function(params, ret);
            }
            let args = c
                .type_arguments
                .iter()
                .map(|a| match a {
                    Type::Wildcard {
                        upper_bound: Some(bound),
                        lower_bound: None,
                    } => TypeArgument::Projected {
                        variance: hyperion_kotlin::Variance::Covariant,
                        bound: semantics_type_to_kotlin(bound),
                    },
                    Type::Wildcard {
                        upper_bound: None,
                        lower_bound: Some(bound),
                    } => TypeArgument::Projected {
                        variance: hyperion_kotlin::Variance::Contravariant,
                        bound: semantics_type_to_kotlin(bound),
                    },
                    Type::Wildcard { .. } => TypeArgument::Star,
                    _ => TypeArgument::Type(semantics_type_to_kotlin(a)),
                })
                .collect();
            KotlinType::generic_with_args(c.qualified_name().to_string(), args)
        }
        Type::Null => KotlinType::Nothing,
        Type::Void => KotlinType::Unit,
        _ => KotlinType::Unknown,
    }
}

fn kotlin_type_to_semantics(ty: &hyperion_kotlin::KotlinType) -> Type {
    use hyperion_kotlin::types::TypeArgument;
    use hyperion_kotlin::KotlinType;

    match ty {
        KotlinType::Class {
            fqn,
            type_arguments,
            ..
        } => {
            let simple = fqn.rsplit('.').next().unwrap_or(fqn);
            let is_kotlin_pkg = fqn == simple || fqn.starts_with("kotlin.");
            if is_kotlin_pkg {
                if let Some(p) = primitive_from_kotlin_name(simple) {
                    return Type::Primitive(p);
                }
            }
            let (name, package) = fqn.rsplit_once('.').map_or_else(
                || (fqn.clone(), None),
                |(pkg, n)| (n.to_string(), Some(pkg.to_string())),
            );
            Type::Class(ClassType {
                name,
                package,
                type_arguments: type_arguments
                    .iter()
                    .map(|arg| match arg {
                        TypeArgument::Type(t) => kotlin_type_to_semantics(t),
                        _ => Type::Unknown,
                    })
                    .collect(),
                is_diamond: false,
            })
        }
        KotlinType::Function {
            parameters,
            return_type,
            ..
        } => {
            let mut type_arguments: Vec<Type> =
                parameters.iter().map(kotlin_type_to_semantics).collect();
            let arity = type_arguments.len();
            type_arguments.push(kotlin_type_to_semantics(return_type));
            Type::Class(ClassType {
                name: format!("Function{arity}"),
                package: Some("kotlin".into()),
                type_arguments,
                is_diamond: false,
            })
        }
        KotlinType::Any => Type::Class(ClassType {
            name: "Any".into(),
            package: Some("kotlin".into()),
            type_arguments: vec![],
            is_diamond: false,
        }),
        KotlinType::Unit => Type::Class(ClassType {
            name: "Unit".into(),
            package: Some("kotlin".into()),
            type_arguments: vec![],
            is_diamond: false,
        }),
        KotlinType::Nothing => Type::Class(ClassType {
            name: "Nothing".into(),
            package: Some("kotlin".into()),
            type_arguments: vec![],
            is_diamond: false,
        }),
        KotlinType::TypeVariable { name, .. } => Type::TypeVariable {
            name: name.clone(),
            bounds: vec![],
        },
        KotlinType::Star | KotlinType::Unknown => Type::Unknown,
    }
}

const fn primitive_kotlin_name(p: PrimitiveType) -> &'static str {
    match p {
        PrimitiveType::Boolean => "Boolean",
        PrimitiveType::Byte => "Byte",
        PrimitiveType::Short => "Short",
        PrimitiveType::Int => "Int",
        PrimitiveType::Long => "Long",
        PrimitiveType::Char => "Char",
        PrimitiveType::Float => "Float",
        PrimitiveType::Double => "Double",
    }
}

fn primitive_from_kotlin_name(name: &str) -> Option<PrimitiveType> {
    Some(match name {
        "Boolean" => PrimitiveType::Boolean,
        "Byte" => PrimitiveType::Byte,
        "Short" => PrimitiveType::Short,
        "Int" => PrimitiveType::Int,
        "Long" => PrimitiveType::Long,
        "Char" => PrimitiveType::Char,
        "Float" => PrimitiveType::Float,
        "Double" => PrimitiveType::Double,
        _ => return None,
    })
}

const fn is_class_like_symbol_kind(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::CLASS
            | SymbolKind::INTERFACE
            | SymbolKind::ENUM
            | SymbolKind::OBJECT
            | SymbolKind::STRUCT
    )
}

fn is_kotlin_identifier_text(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_alphanumeric())
}

fn is_kotlin_identifier_token(token: &hyperion_kotlin::SyntaxToken) -> bool {
    token.kind() == KtKind::IDENT || is_kotlin_identifier_text(token.text())
}

/// Whether `ty` can stand for a member's type or as the bound of a projected
/// argument in a substitution: a class, primitive or array type.
const fn is_projectable(ty: &Type) -> bool {
    matches!(ty, Type::Class(_) | Type::Primitive(_) | Type::Array { .. })
}

/// The type every one of `types` is, when there is at least one and each
/// is known.
fn agreed_type(mut types: impl Iterator<Item = Option<Type>>) -> Option<Type> {
    let first = types.next()??;
    types
        .all(|other| other.as_ref() == Some(&first))
        .then_some(first)
}

/// Whether a call's value argument is named (`name = value`) or spread
/// (`*values`).
fn is_named_or_spread_argument(
    argument: &NodeOrToken<KtSyntaxNode, hyperion_kotlin::SyntaxToken>,
) -> bool {
    let Some(node) = argument.as_node() else {
        return false;
    };
    let mut parts = node
        .children_with_tokens()
        .filter(|part| !part.kind().is_trivia());
    match node.kind() {
        KtKind::BINARY_EXPR => parts.nth(1).is_some_and(|part| part.kind() == KtKind::EQ),
        KtKind::UNARY_EXPR => parts.next().is_some_and(|part| part.kind() == KtKind::STAR),
        _ => false,
    }
}

/// The class `name` of the `kotlin` package with `type_arguments`.
fn kotlin_class(name: &str, type_arguments: Vec<Type>) -> Type {
    Type::Class(ClassType {
        name: name.to_string(),
        package: Some("kotlin".to_string()),
        type_arguments,
        is_diamond: false,
    })
}

/// Whether `class`, which `written`, a parameter type of a member of
/// `container`, was resolved to, is certainly what the declaration names:
/// the qualified name written, `String`, or a class `written` names in
/// `container`, a class enclosing it, or its package. Any other simple name
/// was resolved through this file's imports, not the declaring file's.
fn names_in_declaring_scope(class: &ClassType, written: &str, container: &str) -> bool {
    let qualified = class.qualified_name();
    if written == "String" || qualified == written || jvm_class_name(class) == written {
        return true;
    }
    qualified
        .strip_suffix(written)
        .and_then(|qualifier| qualifier.strip_suffix('.'))
        .is_some_and(|qualifier| {
            container == qualifier
                || container
                    .strip_prefix(qualifier)
                    .is_some_and(|rest| rest.starts_with('.'))
        })
}

/// Whether `class` is Kotlin's `Array`.
fn is_kotlin_array(class: &ClassType) -> bool {
    class.package.as_deref() == Some("kotlin")
        && (class.name == "Array" || primitive_array_element(class).is_some())
}

/// The primitive Java spelling `name` names: `int` is `Int`.
fn java_primitive(name: &str) -> Option<PrimitiveType> {
    Some(match name {
        "int" => PrimitiveType::Int,
        "long" => PrimitiveType::Long,
        "short" => PrimitiveType::Short,
        "byte" => PrimitiveType::Byte,
        "float" => PrimitiveType::Float,
        "double" => PrimitiveType::Double,
        "boolean" => PrimitiveType::Boolean,
        "char" => PrimitiveType::Char,
        _ => return None,
    })
}

/// Kotlin's name for an array of `primitive`: `int[]` is an `IntArray`.
const fn primitive_array_name(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Int => "IntArray",
        PrimitiveType::Long => "LongArray",
        PrimitiveType::Short => "ShortArray",
        PrimitiveType::Byte => "ByteArray",
        PrimitiveType::Float => "FloatArray",
        PrimitiveType::Double => "DoubleArray",
        PrimitiveType::Boolean => "BooleanArray",
        PrimitiveType::Char => "CharArray",
    }
}

/// The element of `class` when it is a Kotlin primitive array: `IntArray`
/// holds `Int`s.
fn primitive_array_element(class: &ClassType) -> Option<PrimitiveType> {
    if class.package.as_deref() != Some("kotlin") {
        return None;
    }
    let element = class.name.strip_suffix("Array")?;
    primitive_from_kotlin_name(element)
}

/// The boxed class of `primitive`: `Int` is a `java.lang.Integer`.
const fn boxed_class_name(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Int => "java.lang.Integer",
        PrimitiveType::Long => "java.lang.Long",
        PrimitiveType::Short => "java.lang.Short",
        PrimitiveType::Byte => "java.lang.Byte",
        PrimitiveType::Float => "java.lang.Float",
        PrimitiveType::Double => "java.lang.Double",
        PrimitiveType::Boolean => "java.lang.Boolean",
        PrimitiveType::Char => "java.lang.Character",
    }
}

/// The JVM name of `class`: a Kotlin built-in is the Java class it maps to,
/// so `kotlin.String` is `java.lang.String` and `kotlin.collections.List`
/// is `java.util.List`.
fn jvm_class_name(class: &ClassType) -> String {
    let name = class.qualified_name();
    match name.as_ref() {
        "kotlin.Any" => "java.lang.Object",
        "kotlin.String" => "java.lang.String",
        "kotlin.CharSequence" => "java.lang.CharSequence",
        "kotlin.Number" => "java.lang.Number",
        "kotlin.Comparable" => "java.lang.Comparable",
        "kotlin.Throwable" => "java.lang.Throwable",
        "kotlin.collections.Iterable" | "kotlin.collections.MutableIterable" => {
            "java.lang.Iterable"
        }
        "kotlin.collections.Collection" | "kotlin.collections.MutableCollection" => {
            "java.util.Collection"
        }
        "kotlin.collections.List" | "kotlin.collections.MutableList" => "java.util.List",
        "kotlin.collections.Set" | "kotlin.collections.MutableSet" => "java.util.Set",
        "kotlin.collections.Map" | "kotlin.collections.MutableMap" => "java.util.Map",
        "kotlin.collections.Iterator" | "kotlin.collections.MutableIterator" => {
            "java.util.Iterator"
        }
        other => other,
    }
    .to_string()
}

/// The class name and the arguments, split at top-level commas, of a generic
/// type spelling: `Map<K, List<V>>` is `Map` with `K` and `List<V>`. None when
/// the spelling has no arguments, or its brackets do not balance or close it.
fn split_type_arguments(type_expr: &str) -> Option<(&str, Vec<&str>)> {
    let open = type_expr.find('<')?;
    let inner = type_expr.strip_suffix('>')?.get(open + 1..)?;
    let mut arguments = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                arguments.push(&inner[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    arguments.push(&inner[start..]);
    Some((type_expr[..open].trim(), arguments))
}

fn extract_first_generic_arg(type_expr: &str) -> Option<&str> {
    let lt = type_expr.find('<')?;
    let gt = type_expr[lt + 1..].find('>')?;
    let inside = &type_expr[lt + 1..lt + 1 + gt];
    inside
        .split(',')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn extract_type_arg_from_collection_ctor(expr: &str) -> Option<&str> {
    let expr = expr.trim();
    let known = [
        "listOf",
        "mutableListOf",
        "setOf",
        "mutableSetOf",
        "sequenceOf",
        "arrayOf",
    ];

    for ctor in known {
        let generic_prefix = format!("{ctor}<");
        if expr.starts_with(&generic_prefix) {
            let inner = &expr[generic_prefix.len()..];
            let gt = inner.find('>')?;
            let arg = inner[..gt].split(',').next()?.trim();
            if !arg.is_empty() {
                return Some(arg);
            }
        }
    }

    None
}

fn extract_first_constructor_name(expr: &str) -> Option<&str> {
    let expr = expr.trim();
    let known = [
        "listOf(",
        "mutableListOf(",
        "setOf(",
        "mutableSetOf(",
        "sequenceOf(",
        "arrayOf(",
    ];

    if !known.iter().any(|prefix| expr.starts_with(prefix)) {
        return None;
    }

    let lparen = expr.find('(')?;
    let args = expr.get(lparen + 1..)?.trim_start();
    let ctor_end = args.find('(')?;
    let ctor_name = args[..ctor_end].trim();
    if ctor_name.is_empty() {
        return None;
    }

    // Keep simple constructor names only; fully qualified names are fine too.
    let starts_with_ident = ctor_name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if starts_with_ident {
        Some(ctor_name)
    } else {
        None
    }
}
