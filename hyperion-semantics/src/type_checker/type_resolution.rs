// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::symbol::{ClassType, Type};
use hyperion_parser::{SyntaxKind, SyntaxNode};
use hyperion_workspace::import_resolver::ImportResolver;
use hyperion_workspace::{Language, TypeNameResolution};

use super::WorkspaceContext;

/// Import-aware type resolution helpers
impl super::TypeChecker {
    /// True only when `actual` provably cannot be assigned to `declared`.
    ///
    /// With a workspace this is the three-valued
    /// [`crate::compile_checks::conversion`] relation, which treats every
    /// unresolved or inference-dependent case as "maybe" so that a wrong
    /// diagnostic can never come from a guess. Without one the structural
    /// fallback is all there is.
    ///
    /// `source` is the expression that produced `actual`. Type arguments are
    /// compared only when that expression's type arguments are written or
    /// declared rather than inferred (see [`Self::type_arguments_are_declared`]).
    pub(super) fn is_definitely_not_assignable(
        &self,
        declared: &Type,
        actual: &Type,
        source: Option<&SyntaxNode>,
    ) -> bool {
        use crate::compile_checks::conversion::{convertible, Context, Convertibility};
        use crate::compile_checks::facts::TypeFacts;

        let Some(ctx) = &self.workspace_context else {
            return !declared.is_assignable_from(actual);
        };
        let declared = Self::resolve_assignability_type(declared, ctx);
        let actual = Self::resolve_assignability_type(actual, ctx);
        let context =
            if source.is_some_and(|expression| self.type_arguments_are_declared(expression)) {
                Context::AssignmentWithTypeArguments
            } else {
                Context::Assignment
            };
        let facts = TypeFacts::new(&ctx.workspace_index, &self.facts_cache);
        convertible(&facts, &actual, &declared, context) == Convertibility::No
    }

    /// Are the type arguments of this expression's type written in the source
    /// (a variable's declared type, `new X<A>()`, a cast) or projected from
    /// such a type through a non-generic method -- as opposed to produced by
    /// generic-method inference, which is where this engine guesses?
    pub(super) fn type_arguments_are_declared(&self, expression: &SyntaxNode) -> bool {
        match expression.kind() {
            SyntaxKind::NEW_EXPR | SyntaxKind::CAST_EXPR | SyntaxKind::THIS_EXPR => true,
            SyntaxKind::PAREN_EXPR => expression
                .children()
                .next()
                .is_some_and(|inner| self.type_arguments_are_declared(&inner)),
            SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR => {
                self.name_has_declared_type(expression)
            }
            SyntaxKind::METHOD_CALL_EXPR => self.call_is_non_generic(expression),
            _ => false,
        }
    }

    /// Is the static type this engine infers for `expression` trustworthy as
    /// the *receiver* of a call it is about to accuse?
    ///
    /// Types spelled in declarations (variables, fields, parameters, `new X()`,
    /// casts) are. The result of a method call is only as good as overload
    /// selection and generic inference, which pick the wrong overload when a
    /// lambda argument is involved (`http.csrf(c -> ..)` returned the type of
    /// the zero-argument `csrf()`); such a receiver is trusted only when every
    /// candidate agrees on one non-generic return type.
    pub(super) fn receiver_type_is_reliable(&self, expression: &SyntaxNode) -> bool {
        match expression.kind() {
            SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR => true,
            SyntaxKind::PAREN_EXPR => expression
                .children()
                .next()
                .is_some_and(|inner| self.receiver_type_is_reliable(&inner)),
            SyntaxKind::NAME_EXPR => {
                self.names_a_type(expression) || self.name_has_declared_type(expression)
            }
            SyntaxKind::FIELD_ACCESS_EXPR => {
                expression
                    .children()
                    .next()
                    .is_some_and(|base| self.receiver_type_is_reliable(&base))
                    && self.name_has_declared_type(expression)
            }
            SyntaxKind::METHOD_CALL_EXPR => self.call_result_is_reliable(expression),
            _ => false,
        }
    }

    /// A NAME_EXPR that the resolver bound to a type (`Math` in `Math.max`).
    fn names_a_type(&self, expression: &SyntaxNode) -> bool {
        let Some(file_semantics) = &self.file_semantics else {
            return false;
        };
        expression
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| token.kind().is_identifier())
            .is_some_and(|ident| {
                matches!(
                    file_semantics.resolve_at(ident.text_range()),
                    Some(crate::resolved_name::ResolvedName::Type { .. })
                )
            })
    }

    /// The type of a call expression is reliable when its receiver is and all
    /// same-named candidates declare one identical, non-generic return type.
    fn call_result_is_reliable(&self, call: &SyntaxNode) -> bool {
        let Some(ctx) = self.workspace_context.as_ref() else {
            return false;
        };
        let Some(method_name) = self.extract_method_name(call) else {
            return false;
        };
        if self.call_has_explicit_receiver(call) {
            let Some(receiver) = call.children().find(|child| {
                !matches!(child.kind(), SyntaxKind::ARG_LIST)
                    && Self::is_expression_kind(child.kind())
            }) else {
                return false;
            };
            if !self.receiver_type_is_reliable(&receiver) {
                return false;
            }
        }
        let Some(Type::Class(receiver_class)) = self.extract_receiver_type(call) else {
            return false;
        };
        let receiver_fqn = Self::resolve_class_fqn(&receiver_class, ctx);
        let arguments: Vec<String> = receiver_class
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        let candidates = ctx
            .workspace_index
            .find_projected_effective_methods_by_name(&receiver_fqn, &arguments, &method_name);
        let mut return_types = candidates.iter().map(|candidate| {
            candidate.projection.as_ref().and_then(|projection| {
                projection
                    .type_parameters
                    .is_empty()
                    .then(|| projection.return_type.clone())
                    .flatten()
            })
        });
        let Some(Some(first)) = return_types.next() else {
            return false;
        };
        return_types.all(|other| other.as_deref() == Some(first.as_str()))
    }

    /// A variable or field whose declaration spells its type (no `var`, no
    /// inferred lambda parameter).
    pub(super) fn name_has_declared_type(&self, expression: &SyntaxNode) -> bool {
        let Some(file_semantics) = &self.file_semantics else {
            return false;
        };
        let Some(ident) = expression
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier())
            .last()
        else {
            return false;
        };
        let location = match file_semantics.resolve_at(ident.text_range()) {
            Some(
                crate::resolved_name::ResolvedName::LocalVariable {
                    location: Some(location),
                    name,
                    ..
                }
                | crate::resolved_name::ResolvedName::Field {
                    location: Some(location),
                    name,
                    ..
                },
            ) => (location, name),
            // Fields of other files carry no location here but always have a
            // spelled type.
            Some(crate::resolved_name::ResolvedName::Field { .. }) => return true,
            _ => return false,
        };
        let Some(root) = expression.ancestors().last() else {
            return false;
        };
        let Some(token) = root
            .token_at_offset(rowan::TextSize::from(location.0.range.start))
            .right_biased()
        else {
            return false;
        };
        if token.text() != location.1 {
            return false;
        }
        let Some(declaration) = token.parent_ancestors().find(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::LOCAL_VAR_DECL
                    | SyntaxKind::FIELD_DECL
                    | SyntaxKind::PARAM
                    | SyntaxKind::FOREACH_STMT
            )
        }) else {
            return false;
        };
        // `var x = ...` and untyped lambda parameters (`x -> ...`) take their
        // type from inference; only `var x = new T(..)` spells it out.
        let is_var = declaration
            .children()
            .any(|child| child.kind() == SyntaxKind::VAR_TYPE);
        if is_var {
            return declaration.kind() == SyntaxKind::LOCAL_VAR_DECL
                && declaration
                    .children()
                    .find(|child| Self::is_expression_kind(child.kind()))
                    .is_some_and(|initializer| {
                        initializer.kind() == SyntaxKind::NEW_EXPR
                            && !initializer
                                .descendants_with_tokens()
                                .filter_map(rowan::NodeOrToken::into_token)
                                .any(|token| token.kind() == SyntaxKind::L_BRACE)
                            && !initializer.text().to_string().contains("<>")
                    });
        }
        declaration.kind() != SyntaxKind::PARAM
            || declaration.children().any(|child| {
                matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                )
            })
    }

    /// A call whose every candidate declaration has no method-level type
    /// parameters and whose receiver's own type arguments are declared.
    fn call_is_non_generic(&self, call: &SyntaxNode) -> bool {
        let Some(ctx) = self.workspace_context.as_ref() else {
            return false;
        };
        let Some(method_name) = self.extract_method_name(call) else {
            return false;
        };
        if self.call_has_explicit_receiver(call) {
            let Some(receiver) = call.children().find(|child| {
                !matches!(child.kind(), SyntaxKind::ARG_LIST)
                    && Self::is_expression_kind(child.kind())
            }) else {
                return false;
            };
            if !self.type_arguments_are_declared(&receiver) {
                // A type name (`Foo.bar()`) has no type arguments to be wrong.
                let names_a_type = matches!(receiver.kind(), SyntaxKind::NAME_EXPR)
                    && self.file_semantics.as_ref().is_some_and(|semantics| {
                        receiver
                            .children_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .find(|token| token.kind().is_identifier())
                            .is_some_and(|ident| {
                                matches!(
                                    semantics.resolve_at(ident.text_range()),
                                    Some(crate::resolved_name::ResolvedName::Type { .. })
                                )
                            })
                    });
                if !names_a_type {
                    return false;
                }
            }
        }
        let Some(Type::Class(receiver_class)) = self.extract_receiver_type(call) else {
            return false;
        };
        let receiver_fqn = Self::resolve_class_fqn(&receiver_class, ctx);
        let arguments: Vec<String> = receiver_class
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        let candidates = ctx
            .workspace_index
            .find_projected_effective_methods_by_name(&receiver_fqn, &arguments, &method_name);
        !candidates.is_empty()
            && candidates.iter().all(|candidate| {
                candidate
                    .projection
                    .as_ref()
                    .is_some_and(|projection| projection.type_parameters.is_empty())
            })
    }

    /// Canonicalize every nominal component before the shared compatibility
    /// check, including generic arguments, array components and variable bounds.
    /// Missing/ambiguous names remain written; they cannot invent a hierarchy.
    fn resolve_assignability_type(ty: &Type, ctx: &WorkspaceContext) -> Type {
        match ty {
            Type::Class(class) => {
                let mut resolved = Self::resolve_class_identity(class, ctx).map_or_else(
                    || class.clone(),
                    |identity| Self::parse_class_type(&identity),
                );
                resolved.type_arguments = class
                    .type_arguments
                    .iter()
                    .map(|argument| Self::resolve_assignability_type(argument, ctx))
                    .collect();
                resolved.is_diamond = class.is_diamond;
                Type::Class(resolved)
            }
            Type::Array {
                element_type,
                dimensions,
            } => Type::Array {
                element_type: Box::new(Self::resolve_assignability_type(element_type, ctx)),
                dimensions: *dimensions,
            },
            Type::TypeVariable { name, bounds } => Type::TypeVariable {
                name: name.clone(),
                bounds: bounds
                    .iter()
                    .map(|bound| Self::resolve_assignability_type(bound, ctx))
                    .collect(),
            },
            Type::Wildcard {
                upper_bound,
                lower_bound,
            } => Type::Wildcard {
                upper_bound: upper_bound
                    .as_ref()
                    .map(|bound| Box::new(Self::resolve_assignability_type(bound, ctx))),
                lower_bound: lower_bound
                    .as_ref()
                    .map(|bound| Box::new(Self::resolve_assignability_type(bound, ctx))),
            },
            Type::Intersection { types } => Type::Intersection {
                types: types
                    .iter()
                    .map(|ty| Self::resolve_assignability_type(ty, ctx))
                    .collect(),
            },
            Type::Union { types } => Type::Union {
                types: types
                    .iter()
                    .map(|ty| Self::resolve_assignability_type(ty, ctx))
                    .collect(),
            },
            other => other.clone(),
        }
    }

    /// Resolve the exact nominal identity used for assignability.
    ///
    /// Unlike the legacy display/navigation helper below, this function never
    /// chooses the first wildcard candidate. Missing and ambiguous names remain
    /// unresolved so they cannot manufacture subtype relationships.
    fn resolve_class_identity(class_type: &ClassType, ctx: &WorkspaceContext) -> Option<String> {
        let written = class_type.raw_qualified_name();
        if class_type.package.is_some() {
            return Some(written);
        }

        match ctx.workspace_index.resolve_type_name_in_context(
            &ctx.imports,
            &[],
            &written,
            Language::Java,
        ) {
            TypeNameResolution::Exact(resolved) => Some(resolved),
            TypeNameResolution::Missing => Self::resolve_unique_file_local_type(&written, ctx),
            TypeNameResolution::Ambiguous(_) => None,
        }
    }

    /// Resolve an otherwise-unbound type spelling to one exact declaration in
    /// the current file.
    ///
    /// The contextual resolver normally owns name binding. Its caller cannot
    /// yet provide the enclosing-type stack, though, so an unqualified member
    /// type such as `Inner` is otherwise invisible. Restricting this fallback
    /// to the current file and requiring a unique exact type suffix avoids the
    /// old global/simple-name first-candidate bug. Multiple possible lexical
    /// owners remain unresolved rather than being chosen arbitrarily.
    fn resolve_unique_file_local_type(written: &str, ctx: &WorkspaceContext) -> Option<String> {
        let suffix = format!(".{written}");
        let mut matches = ctx
            .workspace_index
            .get_file_symbols(ctx.file_id)
            .into_iter()
            .filter(|candidate| {
                (candidate == written || candidate.ends_with(&suffix))
                    && ctx.workspace_index.has_class(candidate)
            })
            .collect::<Vec<_>>();
        matches.sort_unstable();
        matches.dedup();

        (matches.len() == 1).then(|| matches.remove(0))
    }

    /// Resolve a ClassType to its FQN using imports
    pub(super) fn resolve_class_fqn(class_type: &ClassType, ctx: &WorkspaceContext) -> String {
        // Already has package?
        if class_type.package.is_some() {
            return class_type.qualified_name().into_owned();
        }

        // Handle nested classes (e.g., "User.Builder")
        if class_type.name.contains('.') {
            let parts: Vec<&str> = class_type.name.split('.').collect();
            if parts.len() >= 2 {
                // Resolve outer class first
                let outer_class = ClassType::new(parts[0].to_string(), None);
                let outer_fqn = Self::resolve_class_fqn(&outer_class, ctx);

                // Append inner class parts
                let inner_parts = parts[1..].join(".");
                let nested_fqn = format!("{outer_fqn}.{inner_parts}");

                // Verify nested class exists in workspace before returning
                if ctx.workspace_index.has_class(&nested_fqn) {
                    return nested_fqn;
                }
            }
        }

        // Try to resolve using imports
        let resolver = ImportResolver::new(&ctx.imports, &ctx.workspace_index);
        let candidates = resolver.resolve(&class_type.name);

        // Find first matching class in workspace
        for candidate in &candidates {
            if ctx.workspace_index.has_class(candidate) {
                return candidate.clone();
            }
        }

        // Candidate-vector resolution lacks the enclosing lexical owner. If
        // ordinary imports/package lookup found nothing, use only a unique
        // type declaration owned by this exact file.
        if let Some(local) = Self::resolve_unique_file_local_type(&class_type.name, ctx) {
            return local;
        }

        // Return first candidate or qualified name
        candidates
            .into_iter()
            .next()
            .unwrap_or_else(|| class_type.qualified_name().into_owned())
    }
}
