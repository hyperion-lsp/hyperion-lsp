// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::symbol::Type;
use hyperion_parser::{SyntaxKind, SyntaxNode};

/// Method call validation helpers for TypeChecker
///
/// These methods validate method signatures, argument counts, and argument types.
impl super::TypeChecker {
    /// Extract method name from METHOD_CALL_EXPR node
    ///
    /// With nested expression trees, METHOD_CALL_EXPR contains its receiver,
    /// DOT, method name IDENT, and ARG_LIST as direct children:
    /// ```text
    /// METHOD_CALL_EXPR
    ///   NAME_EXPR("user")     ← receiver
    ///   DOT
    ///   IDENT("setName")      ← method name
    ///   ARG_LIST(...)
    /// ```
    /// For simple calls like `method()`, the NAME_EXPR child contains the name.
    #[allow(clippy::unused_self)]
    pub(super) fn extract_method_name(&self, node: &SyntaxNode) -> Option<String> {
        // Look for IDENT token directly in METHOD_CALL_EXPR children
        // Take the last IDENT before ARG_LIST (to skip any receiver IDENTs inside child nodes)
        let mut last_ident = None;
        for elem in node.children_with_tokens() {
            match elem {
                // `with`, `record`, `yield`, ... are contextual keywords: legal
                // method names that the lexer does not produce as IDENT.
                rowan::NodeOrToken::Token(token) if token.kind().is_identifier() => {
                    last_ident = Some(token.text().to_string());
                }
                rowan::NodeOrToken::Node(child) if child.kind() == SyntaxKind::ARG_LIST => {
                    break;
                }
                _ => {}
            }
        }

        if last_ident.is_some() {
            return last_ident;
        }

        // For simple method() calls: NAME_EXPR is first child, get IDENT from it
        for child in node.children() {
            if child.kind() == SyntaxKind::NAME_EXPR {
                for elem in child.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(token) = elem {
                        if token.kind() == SyntaxKind::IDENT {
                            return Some(token.text().to_string());
                        }
                    }
                }
            }
        }

        None
    }

    /// Check `new T(args)`: instantiating an abstract type, constructor
    /// applicability (see [`crate::compile_checks::constructors`]) and the
    /// checked exceptions the constructor declares.
    pub(super) fn check_new_expr(&mut self, node: &SyntaxNode) {
        use crate::compile_checks::constructors::{
            check_constructor, simplify_qualified_names, ConstructorOutcome,
        };
        use crate::compile_checks::facts::TypeFacts;
        use crate::semantic_error::{SemanticError, SemanticErrorKind};

        if let Some(site) = self.new_expression_site(node) {
            let Some(ctx) = self.workspace_context.as_ref() else {
                return;
            };
            let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
            let outcome = check_constructor(
                &facts,
                &site.type_fqn,
                &site.arguments,
                &site.strict,
                site.has_body,
            );
            let simple = simplify_qualified_names(
                site.type_fqn
                    .rsplit_once('.')
                    .map_or(site.type_fqn.as_str(), |(_, name)| name),
            );
            let kind = match outcome {
                ConstructorOutcome::Fine => {
                    self.check_constructor_exceptions(
                        node,
                        &site.type_fqn,
                        &site.arguments,
                        &site.strict,
                        site.range,
                    );
                    None
                }
                ConstructorOutcome::Abstract => {
                    Some(SemanticErrorKind::AbstractInstantiation { type_name: simple })
                }
                ConstructorOutcome::Inapplicable(rejected) => {
                    Some(SemanticErrorKind::ConstructorNotApplicable {
                        class_name: simple,
                        arguments: site
                            .arguments
                            .iter()
                            .map(|argument| argument.as_ref().map(simple_type_display))
                            .collect(),
                        candidates: rejected.into_iter().map(|(params, _)| params).collect(),
                    })
                }
            };
            if let Some(kind) = kind {
                self.errors.push(SemanticError::new(site.range, kind));
            }
        }
        for child in node.children() {
            self.walk_node(&child);
        }
    }

    /// The created class, argument types and source range of a `new` that
    /// constructs an object (not an array, not `outer.new Inner()`).
    fn new_expression_site(&self, node: &SyntaxNode) -> Option<NewSite> {
        let file_semantics = self.file_semantics.as_ref()?;
        self.workspace_context.as_ref()?;
        // `outer.new Inner()` names a member type of `outer`'s type.
        if node
            .children_with_tokens()
            .take_while(|element| element.kind() != SyntaxKind::NEW_KW)
            .any(|element| element.kind() == SyntaxKind::DOT)
        {
            return None;
        }
        // Array creation (`new Integer[]{1}`, `new int[3]`) has no argument
        // list and constructs nothing.
        if !node
            .children()
            .any(|child| child.kind() == SyntaxKind::ARG_LIST)
            || node
                .children()
                .any(|child| child.kind() == SyntaxKind::ARRAY_TYPE)
        {
            return None;
        }
        let class_type = node
            .children()
            .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)?;
        let (_, ident_range) = crate::utils::extract_class_type_simple_name(&class_type)?;
        let crate::resolved_name::ResolvedName::Type { fqn, .. } =
            file_semantics.resolve_at(ident_range)?
        else {
            return None;
        };
        let argument_nodes = self.extract_method_arguments(node);
        // javac points at `new`; stop before an anonymous class body.
        let start = node
            .children_with_tokens()
            .find(|element| element.kind() == SyntaxKind::NEW_KW)
            .map_or_else(
                || node.text_range().start(),
                |element| element.text_range().start(),
            );
        let end = node
            .children()
            .find(|child| child.kind() == SyntaxKind::ARG_LIST)
            .or_else(|| {
                node.children()
                    .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            })
            .map_or_else(|| node.text_range().end(), |child| child.text_range().end());
        Some(NewSite {
            type_fqn: fqn.clone(),
            has_body: node
                .children_with_tokens()
                .any(|element| element.kind() == SyntaxKind::L_BRACE),
            arguments: argument_nodes
                .iter()
                .map(|argument| self.infer_expression_type(argument))
                .collect(),
            strict: argument_nodes
                .iter()
                .map(|argument| self.type_arguments_are_declared(argument))
                .collect(),
            range: rowan::TextRange::new(start, end),
        })
    }

    /// The declared return type of a method, or `None` when it cannot be read.
    pub(super) fn declared_return_expectation(
        &self,
        method: &SyntaxNode,
    ) -> Option<super::ReturnExpectation> {
        let type_node = method.children().find(|child| {
            matches!(
                child.kind(),
                SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::CLASS_TYPE
            )
        })?;
        if type_node.kind() == SyntaxKind::PRIMITIVE_TYPE
            && type_node
                .children_with_tokens()
                .any(|element| element.kind() == SyntaxKind::VOID_KW)
        {
            return Some(super::ReturnExpectation::Nothing);
        }
        let base = if type_node.kind() == SyntaxKind::PRIMITIVE_TYPE {
            self.extract_primitive_type(&type_node)?
        } else {
            self.extract_contextual_class_type(&type_node)
        };
        Some(super::ReturnExpectation::Value(Self::wrap_with_array_dims(
            base, method,
        )))
    }

    /// `return` against the enclosing method's declared return type.
    pub(super) fn check_return(&mut self, node: &SyntaxNode) {
        use crate::semantic_error::{SemanticError, SemanticErrorKind};
        let Some(Some(expectation)) = self.return_expectations.last().cloned() else {
            return;
        };
        let value = node
            .children()
            .find(|child| Self::is_expression_kind(child.kind()));
        match (expectation, value) {
            (super::ReturnExpectation::Nothing, Some(value)) => {
                self.errors.push(SemanticError::new(
                    value.text_range(),
                    SemanticErrorKind::ReturnValueMismatch {
                        detail: "unexpected return value",
                    },
                ));
            }
            (super::ReturnExpectation::Value(_), None) => {
                self.errors.push(SemanticError::new(
                    node.text_range(),
                    SemanticErrorKind::ReturnValueMismatch {
                        detail: "missing return value",
                    },
                ));
            }
            (super::ReturnExpectation::Value(expected), Some(value)) => {
                let Some(actual) = self.infer_expression_type(&value) else {
                    return;
                };
                if self.is_definitely_not_assignable(&expected, &actual, Some(&value)) {
                    self.errors.push(SemanticError::new(
                        value.text_range(),
                        SemanticErrorKind::ReturnTypeMismatch {
                            method_name: String::new(),
                            expected_type: expected,
                            actual_type: actual,
                        },
                    ));
                }
            }
            (super::ReturnExpectation::Nothing, None) => {}
        }
    }

    /// `if (s)` / `while (n)`: the condition must be boolean.
    pub(super) fn check_condition(&mut self, node: &SyntaxNode) {
        use crate::semantic_error::{SemanticError, SemanticErrorKind};
        let condition = match node.kind() {
            SyntaxKind::FOR_STMT => {
                let mut semicolons = 0;
                let mut found = None;
                for element in node.children_with_tokens() {
                    match element {
                        rowan::NodeOrToken::Token(token)
                            if token.kind() == SyntaxKind::SEMICOLON =>
                        {
                            semicolons += 1;
                        }
                        rowan::NodeOrToken::Node(child)
                            if semicolons == 1 && Self::is_expression_kind(child.kind()) =>
                        {
                            found = Some(child);
                        }
                        _ => {}
                    }
                }
                found
            }
            SyntaxKind::DO_WHILE_STMT => node
                .children()
                .filter(|child| Self::is_expression_kind(child.kind()))
                .last(),
            _ => node
                .children()
                .find(|child| Self::is_expression_kind(child.kind())),
        };
        let Some(condition) = condition else { return };
        let Some(actual) = self.infer_expression_type(&condition) else {
            return;
        };
        let boolean = Type::Primitive(crate::symbol::PrimitiveType::Boolean);
        if self.is_definitely_not_assignable(&boolean, &actual, None) {
            self.errors.push(SemanticError::new(
                condition.text_range(),
                SemanticErrorKind::ConditionTypeMismatch {
                    actual_type: actual,
                },
            ));
        }
    }

    /// `for (x : expr)`: `expr` must be an array or an `Iterable`.
    pub(super) fn check_foreach_iterable(&mut self, node: &SyntaxNode) {
        use crate::compile_checks::facts::TypeFacts;
        use crate::semantic_error::{SemanticError, SemanticErrorKind};
        let mut past_colon = false;
        let mut iterable = None;
        for element in node.children_with_tokens() {
            match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::COLON => {
                    past_colon = true;
                }
                rowan::NodeOrToken::Node(child)
                    if past_colon
                        && iterable.is_none()
                        && Self::is_expression_kind(child.kind()) =>
                {
                    iterable = Some(child);
                }
                _ => {}
            }
        }
        let Some(iterable) = iterable else { return };
        let Some(actual) = self.infer_expression_type(&iterable) else {
            return;
        };
        let not_applicable = match &actual {
            Type::Primitive(_) => true,
            Type::Class(class) => {
                let Some(ctx) = self.workspace_context.as_ref() else {
                    return;
                };
                let fqn = class.raw_qualified_name();
                let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
                fqn != "java.lang.Object"
                    && facts.is_known_type(&fqn)
                    && !facts.is_foreign_language_type(&fqn)
                    && facts
                        .is_subtype(&fqn, "java.lang.Iterable")
                        .is_some_and(|is_iterable| !is_iterable)
            }
            _ => false,
        };
        if not_applicable {
            self.errors.push(SemanticError::new(
                iterable.text_range(),
                SemanticErrorKind::ForEachNotApplicable {
                    actual_type: actual,
                },
            ));
        }
    }

    /// Range of the method-name identifier of a call (`foo` in `a.foo(x)`).
    #[allow(clippy::unused_self)]
    pub(super) fn method_name_range(&self, node: &SyntaxNode) -> Option<rowan::TextRange> {
        let mut last = None;
        for elem in node.children_with_tokens() {
            match elem {
                rowan::NodeOrToken::Token(token) if token.kind().is_identifier() => {
                    last = Some(token.text_range());
                }
                rowan::NodeOrToken::Node(child) if child.kind() == SyntaxKind::ARG_LIST => break,
                _ => {}
            }
        }
        last
    }

    /// True when the call is written `receiver.name(...)` rather than `name(...)`.
    #[allow(clippy::unused_self)]
    pub(super) fn call_has_explicit_receiver(&self, node: &SyntaxNode) -> bool {
        node.children_with_tokens().any(|elem| {
            elem.into_token()
                .is_some_and(|token| token.kind() == SyntaxKind::DOT)
        })
    }

    /// Extract receiver type from METHOD_CALL_EXPR node
    ///
    /// With nested expression trees, the receiver is a direct child:
    /// ```text
    /// METHOD_CALL_EXPR
    ///   NAME_EXPR("user")     ← receiver (first expression child)
    ///   DOT
    ///   IDENT("setName")
    ///   ARG_LIST(...)
    /// ```
    /// For `setName()`, returns None (could be `this` or static).
    pub(super) fn extract_receiver_type(&self, node: &SyntaxNode) -> Option<Type> {
        // Check if this method call has a DOT token (indicating a receiver)
        let mut found_dot = false;
        let mut receiver_node: Option<SyntaxNode> = None;

        for elem in node.children_with_tokens() {
            match elem {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::DOT => {
                    found_dot = true;
                    break;
                }
                rowan::NodeOrToken::Node(child) => {
                    // The first expression child before DOT is the receiver
                    if matches!(
                        child.kind(),
                        SyntaxKind::NAME_EXPR
                            | SyntaxKind::FIELD_ACCESS_EXPR
                            | SyntaxKind::METHOD_CALL_EXPR
                            | SyntaxKind::ARRAY_ACCESS_EXPR
                            | SyntaxKind::PAREN_EXPR
                            | SyntaxKind::THIS_EXPR
                            | SyntaxKind::SUPER_EXPR
                            | SyntaxKind::NEW_EXPR
                            | SyntaxKind::TERNARY_EXPR
                            | SyntaxKind::CAST_EXPR
                    ) {
                        receiver_node = Some(child);
                    }
                }
                _ => {}
            }
        }

        // An explicit receiver (`a.b()`, `Foo.class.getName()`) that cannot be
        // typed stays untyped. Falling through to the implicit-`this` case below
        // attributed such calls to the *enclosing* class and reported every one
        // of them as a call to a missing method.
        if found_dot {
            return receiver_node
                .and_then(|receiver| self.infer_receiver_expression_type(&receiver));
        }

        // No explicit receiver: treat as implicit `this`
        if let Some(ctx) = self.workspace_context.as_ref() {
            if let Some(class_name) = enclosing_class_name(node) {
                let fqn = if let Some(package) = ctx.imports.current_package.as_deref() {
                    format!("{package}.{class_name}")
                } else {
                    class_name
                };

                if ctx.workspace_index.has_class(&fqn) {
                    return Some(Type::Class(Self::parse_class_type(&fqn)));
                }
            }
        }

        None
    }

    /// Infer type of a receiver expression (NAME_EXPR, THIS_EXPR, etc.)
    pub(super) fn infer_receiver_expression_type(&self, node: &SyntaxNode) -> Option<Type> {
        match node.kind() {
            SyntaxKind::NAME_EXPR => {
                let ident = node.children_with_tokens().find_map(|elem| {
                    elem.into_token()
                        .filter(|token| token.kind() == SyntaxKind::IDENT)
                })?;
                if let Some(file_semantics) = &self.file_semantics {
                    // Scope-aware resolution is authoritative: a name the
                    // resolver cannot type (lambda parameter, unresolved
                    // symbol) must stay untyped rather than borrow the type of
                    // a same-named field or local elsewhere in the file.
                    if let Some(crate::resolved_name::ResolvedName::Type { fqn, .. }) =
                        file_semantics.resolve_at(ident.text_range())
                    {
                        // `Math.max(..)`: the qualifier names the type itself.
                        return Some(Type::Class(Self::parse_class_type(fqn)));
                    }
                    return file_semantics
                        .expression_type_at(node.text_range())
                        .cloned();
                }
                // Without FileSemantics (standalone use) fall back to the
                // checker's own variable table.
                self.get_variable(ident.text())
            }
            _ => {
                // For more complex expressions, use general type inference
                self.infer_expression_type(node)
            }
        }
    }

    /// Extract argument expressions from METHOD_CALL_EXPR
    ///
    /// Returns a vector of expression nodes representing arguments.
    /// Uses comma-based boundaries to correctly count arguments even when
    /// expressions contain nested sub-expressions (e.g., `method(a.b())` is 1 arg, not 2).
    #[allow(clippy::unused_self)]
    pub(super) fn extract_method_arguments(&self, node: &SyntaxNode) -> Vec<SyntaxNode> {
        let mut arguments = Vec::new();

        // Find ARG_LIST child
        for child in node.children() {
            if child.kind() == SyntaxKind::ARG_LIST {
                // Use comma-based argument extraction (same approach as inlay_hints.rs)
                // This correctly handles nested expressions like config.getHost()
                let mut current_arg_expr: Option<SyntaxNode> = None;

                for arg_child in child.children_with_tokens() {
                    match arg_child {
                        rowan::NodeOrToken::Node(expr_node)
                            if Self::is_expression_kind(expr_node.kind()) =>
                        {
                            // Record the LAST expression node for this argument.
                            // This fixes a bug where flat AST structures like "str".length()
                            // have LITERAL_EXPR and METHOD_CALL_EXPR as siblings within ARG_LIST.
                            // Taking the first would incorrectly get the literal's type (String)
                            // instead of the method call's return type (int).
                            current_arg_expr = Some(expr_node.clone());
                        }
                        rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => {
                            // Comma marks end of current argument
                            if let Some(expr) = current_arg_expr.take() {
                                arguments.push(expr);
                            }
                            // Reset for next argument
                            current_arg_expr = None;
                        }
                        _ => {
                            // Skip other tokens (parens, whitespace, etc.)
                        }
                    }
                }

                // Don't forget the last argument (no trailing comma)
                if let Some(expr) = current_arg_expr.take() {
                    arguments.push(expr);
                }

                break;
            }
        }

        arguments
    }

    /// Check if a SyntaxKind represents an expression node
    /// Every expression node kind, because any expression can be an argument.
    ///
    /// A kind omitted here is invisible to argument extraction, so the call is
    /// judged to have fewer arguments than it does and a correct call is
    /// reported as having the wrong argument count. `CONSTRUCTOR_REF_EXPR` was
    /// missing, which made `forEach(Foo::new)` look like a zero-argument call.
    pub(super) const fn is_expression_kind(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::LITERAL_EXPR
                | SyntaxKind::NAME_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::NEW_EXPR
                | SyntaxKind::BINARY_EXPR
                | SyntaxKind::UNARY_EXPR
                | SyntaxKind::PAREN_EXPR
                | SyntaxKind::CAST_EXPR
                | SyntaxKind::TERNARY_EXPR
                | SyntaxKind::THIS_EXPR
                | SyntaxKind::SUPER_EXPR
                | SyntaxKind::LAMBDA_EXPR
                | SyntaxKind::ARRAY_ACCESS_EXPR
                | SyntaxKind::ARRAY_INIT_EXPR
                | SyntaxKind::CLASS_LITERAL_EXPR
                | SyntaxKind::METHOD_REF_EXPR
                | SyntaxKind::CONSTRUCTOR_REF_EXPR
                | SyntaxKind::ASSIGNMENT_EXPR
                | SyntaxKind::INSTANCEOF_EXPR
                | SyntaxKind::PATTERN_INSTANCEOF_EXPR
                | SyntaxKind::SWITCH_EXPR
        )
    }

    /// Report a call only when every candidate declaration provably rejects
    /// it (see [`crate::compile_checks::calls`]).
    pub(super) fn report_inapplicable_call(
        &mut self,
        node: &SyntaxNode,
        method_name: &str,
        receiver_type: Option<&Type>,
        argument_types: &[Option<Type>],
        strict_arguments: &[bool],
    ) {
        let explicit_receiver = self.call_has_explicit_receiver(node);
        // Accuse only through a receiver whose type is spelled or provably
        // derived, never through overload/generic inference (see
        // `receiver_type_is_reliable`).
        if explicit_receiver {
            let receiver_is_reliable = node
                .children()
                .find(|child| Self::is_expression_kind(child.kind()))
                .is_some_and(|receiver| self.receiver_type_is_reliable(&receiver));
            if !receiver_is_reliable {
                return;
            }
        }
        use crate::compile_checks::calls::{
            check_call, check_method_access, AccessLevel, CallOutcome, Inapplicable,
        };
        use crate::compile_checks::facts::TypeFacts;
        use crate::compile_checks::lexical;
        use crate::semantic_error::{SemanticError, SemanticErrorKind};

        let Some(ctx) = self.workspace_context.as_ref() else {
            return;
        };
        let Some(Type::Class(class_type)) = receiver_type else {
            return;
        };
        let receiver_fqn = Self::resolve_class_fqn(class_type, ctx);
        let receiver_arguments: Vec<String> = class_type
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
        if explicit_receiver && !lexical::is_in_local_or_anonymous_type(node) {
            let owners = lexical::lexical_type_fqns(node, ctx.imports.current_package.as_deref());
            let qualifier = if node
                .children()
                .next()
                .is_some_and(|receiver| receiver.kind() == SyntaxKind::SUPER_EXPR)
            {
                hyperion_workspace::FieldAccessQualifier::Super
            } else {
                hyperion_workspace::FieldAccessQualifier::Expression
            };
            if let Some(violation) = check_method_access(
                &facts,
                &receiver_fqn,
                &receiver_arguments,
                method_name,
                &owners,
                qualifier,
            ) {
                let range = self
                    .method_name_range(node)
                    .unwrap_or_else(|| node.text_range());
                self.errors.push(SemanticError::new(
                    range,
                    SemanticErrorKind::InaccessibleMember {
                        member_kind: "method",
                        member: format!("{method_name}({})", violation.parameters.join(",")),
                        owner: simple_owner(&violation.owner),
                        access: match violation.level {
                            AccessLevel::Private => "private",
                            AccessLevel::Protected => "protected",
                            AccessLevel::PackagePrivate => "package-private",
                        },
                    },
                ));
                return;
            }
        }

        let outcome = check_call(
            &facts,
            &receiver_fqn,
            &receiver_arguments,
            method_name,
            argument_types,
            strict_arguments,
        );
        let rejected = match outcome {
            CallOutcome::Fine => return,
            CallOutcome::NoSuchMethod => {
                // Only an explicit receiver names the type to search. For a
                // bare `foo()` the member may live in an enclosing class or a
                // static import; the name resolver owns that diagnostic.
                if !explicit_receiver
                    // `Object` as a receiver is what an inference fallback
                    // looks like, not something the user wrote.
                    || receiver_fqn == "java.lang.Object"
                    || crate::compile_checks::lombok::lombok_may_generate(
                        &facts,
                        &receiver_fqn,
                        method_name,
                    )
                {
                    return;
                }
                let range = self
                    .method_name_range(node)
                    .unwrap_or_else(|| node.text_range());
                let rendered_arguments = argument_types
                    .iter()
                    .map(|argument| argument.as_ref().map(simple_type_display))
                    .collect();
                self.errors.push(SemanticError::new(
                    range,
                    SemanticErrorKind::MethodNotFound {
                        method_name: method_name.to_owned(),
                        receiver_type: Some(Type::Class(Self::parse_class_type(&receiver_fqn))),
                        argument_types: Some(rendered_arguments),
                    },
                ));
                return;
            }
            CallOutcome::Inapplicable(rejected) => rejected,
        };

        // Explain the closest candidate: one whose arity fits and fails on a
        // specific argument, else the nearest arity.
        let actual_count = argument_types.len();
        let best = rejected
            .iter()
            .find(|candidate| matches!(candidate.reason, Inapplicable::Argument { .. }))
            .or_else(|| {
                rejected
                    .iter()
                    .min_by_key(|candidate| match candidate.reason {
                        Inapplicable::Arity { expected, .. } => expected.abs_diff(actual_count),
                        Inapplicable::Argument { .. } => 0,
                    })
            });
        let Some(best) = best else {
            return;
        };
        let kind = match &best.reason {
            Inapplicable::Arity { expected, actual } => SemanticErrorKind::WrongArgumentCount {
                method_name: method_name.to_owned(),
                expected: *expected,
                actual: *actual,
            },
            Inapplicable::Argument {
                index,
                expected,
                actual,
            } => SemanticErrorKind::MethodArgumentTypeMismatch {
                method_name: method_name.to_owned(),
                param_index: *index,
                expected_type: expected.clone(),
                actual_type: actual.clone(),
            },
        };
        let range = match &best.reason {
            Inapplicable::Argument { index, .. } => self
                .extract_method_arguments(node)
                .get(*index)
                .map_or_else(|| node.text_range(), SyntaxNode::text_range),
            Inapplicable::Arity { .. } => node.text_range(),
        };
        self.errors.push(SemanticError::new(range, kind));
    }

    /// Parse a workspace type string (which may include a trailing parameter
    /// name or `...` varargs marker) into a `Type`.
    ///
    /// Thin facade over [`crate::type_parser`]: strips the parameter name
    /// first, then delegates to the canonical parser.
    #[cfg(test)]
    #[allow(clippy::unused_self)]
    pub(super) fn parse_type_from_string(&self, type_str: &str) -> Option<Type> {
        let type_only = crate::type_parser::strip_parameter_name(type_str);
        crate::type_parser::parse_type_string(type_only)
    }
}

fn enclosing_class_name(node: &SyntaxNode) -> Option<String> {
    for ancestor in node.ancestors() {
        if matches!(
            ancestor.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
        ) {
            for elem in ancestor.children_with_tokens() {
                if let rowan::NodeOrToken::Token(token) = elem {
                    if token.kind() == SyntaxKind::IDENT {
                        return Some(token.text().to_string());
                    }
                }
            }
        }
    }
    None
}

/// Type name as a user would write it: no package, no type arguments.
fn simple_type_display(ty: &Type) -> String {
    match ty {
        Type::Class(class) => class
            .raw_name()
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_owned(),
        Type::Array {
            element_type,
            dimensions,
        } => format!(
            "{}{}",
            simple_type_display(element_type),
            "[]".repeat(usize::from(*dimensions))
        ),
        other => other.to_string(),
    }
}

/// Owner as javac prints it in access errors: the type's simple (dotted
/// member) name without the package.
fn simple_owner(fqn: &str) -> String {
    // Keep nested spelling (`Outer.Inner`) but drop lowercase package segments.
    let segments: Vec<&str> = fqn.split('.').collect();
    let first_type = segments
        .iter()
        .position(|segment| segment.chars().next().is_some_and(char::is_uppercase))
        .unwrap_or(segments.len().saturating_sub(1));
    segments[first_type..].join(".")
}

/// What a `new T(..)` expression constructs and with which arguments.
struct NewSite {
    type_fqn: String,
    has_body: bool,
    arguments: Vec<Option<Type>>,
    strict: Vec<bool>,
    range: rowan::TextRange,
}
