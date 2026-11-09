// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Checked-exception flow for the type checker's walk: which handlers
//! surround a call, constructor call, `throw` or implicit `close()`, and which
//! checked exceptions that site is certain to raise. The type-level questions
//! live in [`crate::compile_checks::exceptions`].

use crate::compile_checks::calls::possibly_applicable_methods;
use crate::compile_checks::constructors::possibly_applicable_constructors;
use crate::compile_checks::exceptions::{
    certainly_thrown, is_checked, is_handled, resolve_declared_exception, Handler,
};
use crate::compile_checks::facts::TypeFacts;
use crate::compile_checks::lexical;
use crate::resolved_name::ResolvedName;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::symbol::Type;
use hyperion_parser::modifiers::declaration_modifiers;
use hyperion_parser::{SyntaxKind, SyntaxNode};

impl super::TypeChecker {
    /// FQN a `CLASS_TYPE` node resolved to, if the resolver bound it.
    fn resolved_class_type(&self, class_type: &SyntaxNode) -> Option<String> {
        let file_semantics = self.file_semantics.as_ref()?;
        let (_, ident_range) = crate::utils::extract_class_type_simple_name(class_type)?;
        match file_semantics.resolve_at(ident_range)? {
            ResolvedName::Type { fqn, .. } => Some(fqn.clone()),
            _ => None,
        }
    }

    /// The handler frame for a method or constructor body.
    pub(super) fn method_handler(&self, member: &SyntaxNode) -> Handler {
        // Lombok's @SneakyThrows lets any checked exception escape.
        if declaration_modifiers(member).has_annotation("SneakyThrows")
            || member.ancestors().skip(1).any(|ancestor| {
                lexical::is_type_declaration(ancestor.kind())
                    && declaration_modifiers(&ancestor).has_annotation("SneakyThrows")
            })
        {
            return Handler::Opaque;
        }
        let mut declared = Vec::new();
        for clause in member
            .children()
            .filter(|child| child.kind() == SyntaxKind::THROWS_CLAUSE)
        {
            for class_type in clause
                .children()
                .filter(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            {
                match self.resolved_class_type(&class_type) {
                    Some(fqn) => declared.push(fqn),
                    // A type variable or an unresolved name: judge nothing.
                    None => return Handler::Opaque,
                }
            }
        }
        Handler::Method(declared)
    }

    /// The frame for a lambda body: the throws of its target's functional
    /// method when the lambda initializes a variable of a known interface.
    pub(super) fn lambda_handler(&self, lambda: &SyntaxNode) -> Handler {
        let Some(ctx) = self.workspace_context.as_ref() else {
            return Handler::Lambda(None);
        };
        let Some(declaration) = lambda.parent().filter(|parent| {
            matches!(
                parent.kind(),
                SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::FIELD_DECL
            )
        }) else {
            return Handler::Lambda(None);
        };
        let Some(class_type) = declaration
            .children()
            .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
        else {
            return Handler::Lambda(None);
        };
        let Some(interface) = self.resolved_class_type(&class_type) else {
            return Handler::Lambda(None);
        };
        let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
        let Some(method) = ctx
            .workspace_index
            .find_functional_interface_method(&interface)
        else {
            return Handler::Lambda(None);
        };
        let mut allowed = Vec::new();
        for written in method.throws() {
            if method
                .type_parameters
                .iter()
                .any(|parameter| parameter.name.as_ref() == &**written)
            {
                return Handler::Lambda(None);
            }
            match resolve_declared_exception(&facts, &method, written) {
                Some(fqn) => allowed.push(fqn),
                None => return Handler::Lambda(None),
            }
        }
        Handler::Lambda(Some(allowed))
    }

    /// Walk a `try`: the resources and body run under the catch clauses'
    /// handler; the catch and finally blocks do not.
    pub(super) fn walk_try(&mut self, node: &SyntaxNode) {
        let mut caught = Vec::new();
        let mut opaque = false;
        for clause in node
            .children()
            .filter(|child| child.kind() == SyntaxKind::CATCH_CLAUSE)
        {
            for class_type in clause
                .children()
                .filter(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            {
                match self.resolved_class_type(&class_type) {
                    Some(fqn) => caught.push(fqn),
                    None => opaque = true,
                }
            }
        }
        let frame = if opaque {
            Handler::Opaque
        } else {
            Handler::Caught(caught)
        };
        let mut seen_body = false;
        for child in node.children() {
            let guarded = match child.kind() {
                SyntaxKind::RESOURCE_SPEC => true,
                SyntaxKind::BLOCK_STMT if !seen_body => {
                    seen_body = true;
                    true
                }
                _ => false,
            };
            if guarded {
                self.handlers.push(frame.clone());
                if child.kind() == SyntaxKind::RESOURCE_SPEC {
                    self.check_resource_close(&child);
                }
                self.walk_node(&child);
                self.handlers.pop();
            } else {
                self.walk_node(&child);
            }
        }
    }

    /// `throw new X(..)` where `X` is checked and unhandled.
    pub(super) fn check_throw_statement(&mut self, node: &SyntaxNode) {
        let Some(thrown) = node
            .children()
            .find(|child| child.kind() == SyntaxKind::NEW_EXPR)
        else {
            return;
        };
        let Some(class_type) = thrown
            .children()
            .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
        else {
            return;
        };
        let Some(fqn) = self.resolved_class_type(&class_type) else {
            return;
        };
        self.report_unreported(node.text_range(), &[fqn]);
    }

    /// Checked exceptions declared by the method a call resolves to.
    pub(super) fn check_call_exceptions(
        &mut self,
        node: &SyntaxNode,
        method_name: &str,
        receiver_type: Option<&Type>,
        argument_types: &[Option<Type>],
        strict_arguments: &[bool],
    ) {
        if self.handlers.is_empty() || lexical::is_in_local_or_anonymous_type(node) {
            return;
        }
        let Some(ctx) = self.workspace_context.as_ref() else {
            return;
        };
        let Some(Type::Class(class_type)) = receiver_type else {
            return;
        };
        if self.call_has_explicit_receiver(node) {
            let reliable = node
                .children()
                .find(|child| Self::is_expression_kind(child.kind()))
                .is_some_and(|receiver| self.receiver_type_is_reliable(&receiver));
            if !reliable {
                return;
            }
        }
        let receiver_fqn = Self::resolve_class_fqn(class_type, ctx);
        let receiver_arguments: Vec<String> = class_type
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
        let Some(candidates) = possibly_applicable_methods(
            &facts,
            &receiver_fqn,
            &receiver_arguments,
            method_name,
            argument_types,
            strict_arguments,
        ) else {
            return;
        };
        if candidates.is_empty() {
            return;
        }
        let symbols: Vec<_> = candidates
            .iter()
            .map(|candidate| std::sync::Arc::clone(&candidate.definition.symbol))
            .collect();
        let Some(thrown) = certainly_thrown(&facts, &symbols) else {
            return;
        };
        let range = self
            .method_name_range(node)
            .unwrap_or_else(|| node.text_range());
        self.report_unreported(range, &thrown);
    }

    /// Checked exceptions declared by the constructor a `new` resolves to.
    pub(super) fn check_constructor_exceptions(
        &mut self,
        node: &SyntaxNode,
        type_fqn: &str,
        argument_types: &[Option<Type>],
        strict_arguments: &[bool],
        range: rowan::TextRange,
    ) {
        if self.handlers.is_empty() {
            return;
        }
        let Some(ctx) = self.workspace_context.as_ref() else {
            return;
        };
        let _ = node;
        let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
        let Some(constructors) =
            possibly_applicable_constructors(&facts, type_fqn, argument_types, strict_arguments)
        else {
            return;
        };
        if constructors.is_empty() {
            return;
        }
        let Some(thrown) = certainly_thrown(&facts, &constructors) else {
            return;
        };
        self.report_unreported(range, &thrown);
    }

    /// The implicit `close()` of each try-with-resources variable.
    fn check_resource_close(&mut self, resources: &SyntaxNode) {
        for declaration in resources
            .children()
            .filter(|child| child.kind() == SyntaxKind::LOCAL_VAR_DECL)
        {
            let Some(class_type) = declaration
                .children()
                .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            else {
                continue;
            };
            let Some(fqn) = self.resolved_class_type(&class_type) else {
                continue;
            };
            let Some(ctx) = self.workspace_context.as_ref() else {
                continue;
            };
            let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
            let Some(candidates) =
                possibly_applicable_methods(&facts, &fqn, &[], "close", &[], &[])
            else {
                continue;
            };
            let symbols: Vec<_> = candidates
                .iter()
                .map(|candidate| std::sync::Arc::clone(&candidate.definition.symbol))
                .collect();
            let Some(thrown) = certainly_thrown(&facts, &symbols) else {
                continue;
            };
            self.report_unreported(declaration.text_range(), &thrown);
        }
    }

    /// Report the first exception of `thrown` that no surrounding handler covers.
    fn report_unreported(&mut self, range: rowan::TextRange, thrown: &[String]) {
        let Some(ctx) = self.workspace_context.as_ref() else {
            return;
        };
        let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
        for exception in thrown {
            if is_checked(&facts, exception) != Some(true) {
                continue;
            }
            if is_handled(&facts, &self.handlers, exception) == Some(false) {
                let simple = exception.rsplit('.').next().unwrap_or(exception).to_owned();
                self.errors.push(SemanticError::new(
                    range,
                    SemanticErrorKind::UnreportedException { exception: simple },
                ));
                return;
            }
        }
    }
}
