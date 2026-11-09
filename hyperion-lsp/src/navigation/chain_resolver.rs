// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Chain Resolution
//!
//! Handles resolution of method chains like `user.getProfile().getName()`.
//! This was extracted from provider.rs to keep file sizes manageable.

use hyperion_parser::syntax::{SyntaxNode, SyntaxToken};
use hyperion_semantics::call_resolution::resolve_call;
use hyperion_semantics::db::FileId;
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_semantics::symbol::Type;
use hyperion_semantics::ResolvedSymbol;
use tower_lsp::lsp_types::{Location, SymbolKind};
use tracing::debug;

use super::NavigationProvider;
use crate::navigation::argument_inference::infer_argument_types;
use crate::navigation::import_resolver::{
    resolve_static_value_type_via_imports, StaticValueTypeResolution,
};
use crate::navigation::ChainSegment;

fn identifier_token_at_offset(
    root: &SyntaxNode,
    offset: u32,
    expected_name: &str,
) -> Option<SyntaxToken> {
    let at_offset = root.token_at_offset(rowan::TextSize::from(offset));
    [at_offset.clone().left_biased(), at_offset.right_biased()]
        .into_iter()
        .flatten()
        .find(|token| token.kind().is_identifier() && token.text() == expected_name)
}

impl NavigationProvider<'_> {
    /// Replace a receiver type that names an in-scope type parameter with that
    /// parameter's bound.
    ///
    /// JLS 4.4: "The members of a type variable X with bound T ... are the
    /// members of the intersection type ... appearing at the point where the
    /// type variable is declared." A type variable declares no members of its
    /// own, so a member lookup against `T` itself can only ever miss. In
    /// `class Box<T extends Comparable> { T value; ... value.compareTo(x) }`
    /// the receiver's static type is `T`, and `compareTo` is a member of it
    /// exactly because `Comparable` declares it.
    ///
    /// This is not a name-shape guess: `find_type_parameter_bound` substitutes
    /// only when an enclosing `METHOD_DECL` / `CONSTRUCTOR_DECL` / `CLASS_DECL`
    /// / `INTERFACE_DECL` / `RECORD_DECL` actually declares a `TYPE_PARAM` of
    /// that name, innermost first, so an inner unbounded `<T>` shadows rather
    /// than inherits an outer `<T extends X>` and a real class that happens to
    /// be called `T` is left alone. Semantic TypeVariable receivers carry their
    /// bounds directly; unbounded variables use java.lang.Object. The syntax
    /// fallback remains for older callers carrying a bare class spelling.
    fn substitute_type_parameter_bound(
        &self,
        receiver_type: &Type,
        file_id: FileId,
        offset: u32,
        root: &SyntaxNode,
    ) -> Option<Type> {
        if matches!(receiver_type, Type::TypeVariable { .. }) {
            let mut current = receiver_type;
            let mut variables = std::collections::HashMap::new();
            while let Type::TypeVariable { name, bounds } = current {
                if variables.insert(name.clone(), current.clone()).is_some() {
                    return None;
                }
                let Some(bound) = bounds.first() else {
                    return Some(Type::Class(hyperion_semantics::symbol::ClassType::object()));
                };
                current = bound;
            }
            // Preserve the full lexical variable in recursive bounds such as
            // Comparable<T>; the finite reference inside the bound has no bounds
            // of its own and must not replace the actual receiver's T.
            return Some(hyperion_semantics::resolver::type_parameter_mapping::apply_type_parameter_substitutions(
                current.clone(), &variables,
            ));
        }
        let Type::Class(class_type) = receiver_type else {
            return None;
        };
        if class_type.package.is_some() || class_type.name.contains('.') {
            return None;
        }
        let bound_fqn = self.find_type_parameter_bound(root, file_id, offset, &class_type.name)?;
        debug!(
            "resolve_chain: receiver '{}' is a type parameter bounded by '{}' (JLS 4.4)",
            class_type.name, bound_fqn
        );
        Some(super::fqn_to_type(&bound_fqn))
    }

    /// Return the canonical receiver identity and its concrete generic
    /// arguments. Simple source spellings are resolved once in their file
    /// context; subsequent member queries consume only this identity.
    pub(crate) fn chain_receiver_identity(
        &self,
        receiver: &Type,
        file_id: FileId,
    ) -> Option<(String, Vec<String>)> {
        let Type::Class(class) = receiver else {
            return None;
        };
        let fqn = class.package.as_ref().map_or_else(
            || {
                crate::navigation::import_resolver::resolve_class_fqn_via_imports(
                    &self.workspace_index,
                    file_id,
                    &class.name,
                )
                .ok()
            },
            |package| Some(format!("{package}.{}", class.name)),
        )?;
        let arguments = class
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect();
        Some((fqn, arguments))
    }

    /// Resolve the return type of the only effective same-named method through
    /// the workspace's canonical hierarchy/projection query. This is the
    /// conservative fallback for text-recovered calls with no argument range:
    /// overloads and unavailable generic projections deliberately return none.
    pub(crate) fn projected_name_only_method_return_type(
        &self,
        receiver: &Type,
        file_id: FileId,
        method_name: &str,
    ) -> Option<Type> {
        let (owner, arguments) = self.chain_receiver_identity(receiver, file_id)?;
        let candidates = self
            .workspace_index
            .find_projected_effective_methods_by_name(&owner, &arguments, method_name);
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        let return_type = candidate.projection.as_ref()?.return_type.as_deref()?;
        hyperion_semantics::type_parser::parse_type_string(return_type)
    }

    /// Try to resolve a method name as a same-class method and return its qualified return type.
    fn resolve_same_class_method_type(
        &self,
        segment: &ChainSegment,
        file_id: FileId,
        root: &SyntaxNode,
        offset: u32,
    ) -> Option<Type> {
        let enclosing_class_fqn = self.find_enclosing_class(root, rowan::TextSize::from(offset))?;
        if let Some(argument_types) =
            self.infer_chain_segment_argument_types(segment, root, file_id)
        {
            let receiver_type = super::fqn_to_type(&enclosing_class_fqn);
            return resolve_call(
                &self.workspace_index,
                &enclosing_class_fqn,
                Some(&receiver_type),
                &segment.name,
                &argument_types,
            )
            .map(|resolved| resolved.return_type);
        }

        let receiver = super::fqn_to_type(&enclosing_class_fqn);
        self.projected_name_only_method_return_type(&receiver, file_id, &segment.name)
    }

    /// Infer semantic argument types for a structured chain segment.
    ///
    /// The chain parser records exact ranges only when it recovered the segment from
    /// the Java AST. Legacy text fallbacks deliberately return `None`, keeping their
    /// lower-confidence behavior visible at the call site.
    fn infer_chain_segment_argument_types(
        &self,
        segment: &ChainSegment,
        root: &SyntaxNode,
        file_id: FileId,
    ) -> Option<Vec<Type>> {
        let call_range = segment.call_range?;
        let call = root.descendants().find(|node| {
            node.kind() == hyperion_parser::syntax::SyntaxKind::METHOD_CALL_EXPR
                && node.text_range() == call_range
        })?;
        let arg_list = call
            .children()
            .find(|node| node.kind() == hyperion_parser::syntax::SyntaxKind::ARG_LIST)?;
        let offset = u32::from(call_range.start());
        Some(
            infer_argument_types(&arg_list, file_id, &self.workspace_index, root, offset)
                .into_iter()
                .map(|inferred| inferred.ty)
                .collect(),
        )
    }

    /// Try to resolve a name as a class via imports, returning its Type.
    fn resolve_name_as_class(&self, name: &str, file_id: FileId) -> Option<Type> {
        use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
        let fqn = resolve_class_fqn_via_imports(&self.workspace_index, file_id, name).ok()?;
        Some(super::fqn_to_type(&fqn))
    }

    /// Resolve a bare field inherited by the enclosing type and project its
    /// declared type into that receiver's generic context.
    fn resolve_enclosing_value_type(
        &self,
        name: &str,
        file_id: FileId,
        offset: u32,
        root: &SyntaxNode,
    ) -> Option<Type> {
        let enclosing = self.find_enclosing_class(root, rowan::TextSize::from(offset))?;
        let field = self
            .workspace_index
            .find_projected_effective_field_by_name(&enclosing, &[], name)?;
        let projected = field.projected_type?;
        self.create_type_inference_engine(file_id)
            .parse_type_string(&projected)
    }

    /// Qualify variable types with the workspace's exact lexical/import resolver.
    fn qualify_variable_type(
        &self,
        ty: Type,
        file_id: FileId,
        root: &SyntaxNode,
        offset: u32,
    ) -> Type {
        let mut lexical_owners = Vec::new();
        let mut owner = self.find_enclosing_class(root, rowan::TextSize::from(offset));
        while let Some(current) = owner {
            lexical_owners.push(current.clone());
            owner = self
                .workspace_index
                .get_symbol(&current)
                .and_then(|symbol| symbol.container.as_ref().map(ToString::to_string))
                .filter(|container| {
                    self.workspace_index
                        .get_symbol(container)
                        .is_some_and(|symbol| {
                            hyperion_workspace::is_type_declaration_kind(symbol.kind)
                        })
                });
        }
        self.qualify_variable_type_with_owners(ty, file_id, &lexical_owners)
    }

    fn qualify_variable_type_with_owners(
        &self,
        ty: Type,
        file_id: FileId,
        lexical_owners: &[String],
    ) -> Type {
        match ty {
            Type::Array {
                element_type,
                dimensions,
            } => Type::Array {
                element_type: Box::new(self.qualify_variable_type_with_owners(
                    *element_type,
                    file_id,
                    lexical_owners,
                )),
                dimensions,
            },
            Type::Class(mut class_type) => {
                class_type.type_arguments = class_type
                    .type_arguments
                    .into_iter()
                    .map(|argument| {
                        self.qualify_variable_type_with_owners(argument, file_id, lexical_owners)
                    })
                    .collect();
                let current_package = self
                    .workspace_index
                    .get_imports(file_id)
                    .and_then(|imports| imports.current_package);
                let direct_exists = self
                    .workspace_index
                    .get_symbol(class_type.qualified_name().as_ref())
                    .is_some();
                let needs_contextual_resolution = class_type.package.is_none()
                    || (!direct_exists && class_type.package == current_package);
                if class_type.name.contains('.') || !needs_contextual_resolution {
                    return Type::Class(class_type);
                }
                let hyperion_workspace::TypeNameResolution::Exact(fqn) = self
                    .workspace_index
                    .resolve_type_name(file_id, lexical_owners, &class_type.name)
                else {
                    return Type::Class(class_type);
                };
                let Type::Class(mut qualified) = super::fqn_to_type(&fqn) else {
                    return Type::Class(class_type);
                };
                qualified.type_arguments = class_type.type_arguments;
                qualified.is_diamond = class_type.is_diamond;
                Type::Class(qualified)
            }
            Type::Wildcard {
                upper_bound,
                lower_bound,
            } => Type::Wildcard {
                upper_bound: upper_bound.map(|bound| {
                    Box::new(self.qualify_variable_type_with_owners(
                        *bound,
                        file_id,
                        lexical_owners,
                    ))
                }),
                lower_bound: lower_bound.map(|bound| {
                    Box::new(self.qualify_variable_type_with_owners(
                        *bound,
                        file_id,
                        lexical_owners,
                    ))
                }),
            },
            Type::Intersection { types } => Type::Intersection {
                types: types
                    .into_iter()
                    .map(|ty| self.qualify_variable_type_with_owners(ty, file_id, lexical_owners))
                    .collect(),
            },
            Type::Union { types } => Type::Union {
                types: types
                    .into_iter()
                    .map(|ty| self.qualify_variable_type_with_owners(ty, file_id, lexical_owners))
                    .collect(),
            },
            Type::TypeVariable { name, bounds } => Type::TypeVariable {
                name,
                bounds: bounds
                    .into_iter()
                    .map(|bound| {
                        self.qualify_variable_type_with_owners(bound, file_id, lexical_owners)
                    })
                    .collect(),
            },
            other => other,
        }
    }

    /// Resolve the type of the first segment in a chain.
    ///
    /// Handles: this/super keywords, cast types, variable lookup, lambda inference,
    /// same-class method calls, static imports, and class name fallback.
    fn resolve_first_segment_type(
        &self,
        segment: &ChainSegment,
        file_id: FileId,
        offset: u32,
        root: &SyntaxNode,
        resolver: &hyperion_semantics::resolver::SymbolResolver,
        scope_at_position: Option<hyperion_semantics::symbol::ScopeId>,
    ) -> Option<Type> {
        if segment.name == "this" {
            let enclosing = self.find_enclosing_class(root, rowan::TextSize::from(offset))?;
            return Some(super::fqn_to_type(&enclosing));
        }

        // Handle "super" keyword
        if segment.name == "super" {
            let enclosing = self.find_enclosing_class(root, rowan::TextSize::from(offset))?;
            let parent_fqn = self.workspace_index.get_parent_class(&enclosing)?;
            debug!(
                "resolve_first_segment_type: super -> parent class {}",
                parent_fqn
            );
            return Some(super::fqn_to_type(&parent_fqn));
        }

        // Handle cast type
        if let Some(ref cast_type) = segment.cast_type {
            debug!("resolve_first_segment_type: cast to '{}'", cast_type);
            return self
                .create_type_inference_engine(file_id)
                .parse_type_string(cast_type);
        }

        // Resolve variables, fields, and pattern bindings at this exact position.
        let first_symbol =
            Self::resolve_variable_symbol(&segment.name, resolver, scope_at_position, offset);

        if let Some(symbol) = first_symbol {
            let ty = self.qualify_variable_type(symbol.ty.clone(), file_id, root, offset);
            debug!(
                "resolve_first_segment_type: '{}' resolved as variable, type: {:?}",
                segment.name, ty
            );

            if !matches!(ty, Type::Unknown) {
                return Some(ty);
            }

            // Type is Unknown — try lambda inference first
            if let Some(lambda_type) =
                super::lambda_inference::infer_lambda_parameter_type_with_workspace_and_file(
                    root,
                    &segment.name,
                    offset,
                    Some(self.workspace_index.clone()),
                    file_id,
                )
            {
                debug!(
                    "resolve_first_segment_type: lambda inference for Unknown '{}': {:?}",
                    segment.name, lambda_type
                );
                return Some(lambda_type);
            }

            // On-demand var inference fallback:
            // symbol tables can carry Unknown for `var` declarations when initial indexing
            // lacked enough cross-file context.
            if let Some(inferred_var_type) =
                self.infer_unknown_local_var_type(&segment.name, file_id, offset, root, resolver)
            {
                debug!(
                    "resolve_first_segment_type: inferred Unknown var '{}' as {:?}",
                    segment.name, inferred_var_type
                );
                return Some(inferred_var_type);
            }

            // Try same-class method, then class name as fallbacks
            if segment.is_method_call {
                if let Some(t) = self.resolve_same_class_method_type(segment, file_id, root, offset)
                {
                    return Some(t);
                }
            }
            return self
                .resolve_name_as_class(&segment.name, file_id)
                .or(Some(ty));
        }

        if !segment.is_method_call {
            if let Some(field_type) =
                self.resolve_enclosing_value_type(&segment.name, file_id, offset, root)
            {
                debug!(
                    "resolve_first_segment_type: inherited field '{}' has projected type {:?}",
                    segment.name, field_type
                );
                return Some(field_type);
            }
            match resolve_static_value_type_via_imports(
                &self.workspace_index,
                file_id,
                &segment.name,
            ) {
                StaticValueTypeResolution::Resolved(ty) => return Some(ty),
                StaticValueTypeResolution::Unresolved => return None,
                StaticValueTypeResolution::Missing => {}
            }
        }

        // Not a variable — try lambda inference
        if let Some(lambda_type) =
            super::lambda_inference::infer_lambda_parameter_type_with_workspace_and_file(
                root,
                &segment.name,
                offset,
                Some(self.workspace_index.clone()),
                file_id,
            )
        {
            debug!(
                "resolve_first_segment_type: lambda inference for '{}': {:?}",
                segment.name, lambda_type
            );
            return Some(lambda_type);
        }

        // Try method call resolution
        if segment.is_method_call {
            // Same-class method
            if let Some(t) = self.resolve_same_class_method_type(segment, file_id, root, offset) {
                return Some(t);
            }

            // Static import
            if let Some(t) = self.resolve_static_import_return_type(segment, file_id, root, offset)
            {
                return Some(t);
            }
        }

        // Last resort: class name for static access
        self.resolve_name_as_class(&segment.name, file_id)
    }

    fn infer_unknown_local_var_type(
        &self,
        name: &str,
        file_id: FileId,
        offset: u32,
        root: &SyntaxNode,
        resolver: &hyperion_semantics::resolver::SymbolResolver,
    ) -> Option<Type> {
        use hyperion_parser::SyntaxKind;

        let enclosing_executable = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()
            .and_then(|tok| tok.parent())
            .and_then(|node| {
                node.ancestors().find(|n| {
                    matches!(
                        n.kind(),
                        SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
                    )
                })
            });

        let mut best_decl: Option<SyntaxNode> = None;
        let mut best_start = 0u32;

        for node in root.descendants() {
            if node.kind() != SyntaxKind::LOCAL_VAR_DECL {
                continue;
            }

            let start = u32::from(node.text_range().start());
            if start >= offset {
                continue;
            }

            // Only infer for `var` declarations here; explicit typed locals should not be Unknown.
            if !node.children().any(|c| c.kind() == SyntaxKind::VAR_TYPE) {
                continue;
            }

            let declares_name = node.children_with_tokens().any(|child| {
                child
                    .as_token()
                    .is_some_and(|tok| tok.kind() == SyntaxKind::IDENT && tok.text() == name)
            });
            if !declares_name {
                continue;
            }

            if let Some(ref exec) = enclosing_executable {
                let same_executable = node.ancestors().any(|ancestor| {
                    ancestor.kind() == exec.kind() && ancestor.text_range() == exec.text_range()
                });
                if !same_executable {
                    continue;
                }
            }

            if best_decl.is_none() || start > best_start {
                best_start = start;
                best_decl = Some(node);
            }
        }

        let decl = best_decl?;
        let builder = ResolverBuilder::for_var_inference(
            file_id,
            resolver.symbol_table().clone(),
            self.workspace_index.clone(),
        );
        let inferred = builder.infer_var_type(&decl);
        if matches!(inferred, Type::Unknown) {
            None
        } else {
            Some(self.qualify_variable_type(inferred, file_id, root, offset))
        }
    }

    /// Resolve a variable or pattern binding visible at the exact source position.
    fn resolve_variable_symbol(
        name: &str,
        resolver: &hyperion_semantics::resolver::SymbolResolver,
        scope_at_position: Option<hyperion_semantics::symbol::ScopeId>,
        offset: u32,
    ) -> Option<ResolvedSymbol> {
        scope_at_position
            .and_then(|scope_id| resolver.resolve_in_scope_at_offset(name, scope_id, offset))
    }

    /// Resolve a static import's return type.
    fn resolve_static_import_return_type(
        &self,
        segment: &ChainSegment,
        file_id: FileId,
        root: &SyntaxNode,
        offset: u32,
    ) -> Option<Type> {
        use crate::navigation::import_resolver::{
            enclosing_callable_shadows_static_import, resolve_static_invocation_via_imports,
        };

        let argument_types = self.infer_chain_segment_argument_types(segment, root, file_id)?;
        if self
            .find_enclosing_class(root, rowan::TextSize::from(offset))
            .is_some_and(|owner| {
                enclosing_callable_shadows_static_import(
                    &self.workspace_index,
                    file_id,
                    &owner,
                    &segment.name,
                    &argument_types,
                )
            })
        {
            return None;
        }
        let method_location = resolve_static_invocation_via_imports(
            &self.workspace_index,
            file_id,
            &segment.name,
            &argument_types,
        )?;
        debug!(
            "resolve_static_import_return_type: resolved to {}",
            method_location.fqn
        );

        let method_symbol =
            self.workspace_index
                .get_symbol_target(&hyperion_workspace::SymbolTarget::new(
                    method_location.fqn.clone(),
                    hyperion_workspace::DeclarationNamespace::Callable,
                ))?;
        let owner = method_symbol.container.as_deref()?;
        let candidates = self
            .workspace_index
            .find_projected_effective_methods_by_name(owner, &[], &segment.name);
        let mut selected = candidates.into_iter().filter(|candidate| {
            candidate.definition.target.fqn.as_ref() == method_location.fqn.as_ref()
        });
        let candidate = selected.next()?;
        if selected.next().is_some() {
            return None;
        }
        let return_type = candidate.projection?.return_type?;
        hyperion_semantics::type_parser::parse_type_string(&return_type)
    }

    /// Resolve a method chain to find the definition of a specific segment
    ///
    /// This walks through a chain like `user.getProfile().getName()` step by step:
    /// 1. Resolve `user` to get its type (e.g., `User`)
    /// 2. Look up `getProfile()` on `User`, get return type (e.g., `Profile`)
    /// 3. Look up `getName()` on `Profile` - this is our target
    pub(crate) fn resolve_chain(
        &self,
        file_id: FileId,
        chain: &[ChainSegment],
        target_idx: usize,
        offset: u32,
        root: &SyntaxNode,
        source_text: &str,
    ) -> Result<Option<Location>, String> {
        debug!(
            "resolve_chain: {} segments, target at {}",
            chain.len(),
            target_idx
        );

        // Fast path: use cached expression types from FileSemantics
        if target_idx > 0 {
            if let Some(result) =
                self.try_resolve_chain_via_file_semantics(file_id, chain, target_idx, offset, root)
            {
                return Ok(Some(result));
            }
        }

        // Build symbol resolver from parse tree
        let builder = ResolverBuilder::with_workspace(file_id, self.workspace_index.clone());
        let resolver = builder.build_from_syntax(root);
        let scope_at_position = resolver.find_scope_at_offset(offset);

        // Resolve the first segment's type
        let first_segment = &chain[0];
        let qualified_type_prefix = first_segment
            .name
            .chars()
            .next()
            .is_some_and(char::is_lowercase)
            .then(|| {
                let first_is_variable = Self::resolve_variable_symbol(
                    &first_segment.name,
                    &resolver,
                    scope_at_position,
                    offset,
                )
                .is_some();
                if first_is_variable {
                    return None;
                }

                let mut prefix = String::new();
                let mut resolved = None;
                for (index, segment) in chain.iter().enumerate().take(target_idx) {
                    if segment.is_method_call {
                        break;
                    }
                    if !prefix.is_empty() {
                        prefix.push('.');
                    }
                    prefix.push_str(&segment.name);
                    if self
                        .workspace_index
                        .get_symbol_target(&hyperion_workspace::SymbolTarget::new(
                            prefix.as_str(),
                            hyperion_workspace::DeclarationNamespace::Type,
                        ))
                        .is_some()
                    {
                        resolved = Some((index, super::fqn_to_type(&prefix)));
                    }
                }
                resolved
            })
            .flatten();

        let (mut current_type, first_unresolved_segment) =
            if let Some((prefix_end, prefix_type)) = qualified_type_prefix {
                (prefix_type, prefix_end + 1)
            } else {
                let Some(first_type) = self.resolve_first_segment_type(
                    first_segment,
                    file_id,
                    offset,
                    root,
                    &resolver,
                    scope_at_position,
                ) else {
                    return Ok(None);
                };
                let first_type = self
                    .substitute_type_parameter_bound(&first_type, file_id, offset, root)
                    .unwrap_or(first_type);
                (first_type, 1)
            };

        debug!("resolve_chain: initial type: {:?}", current_type);

        // Now walk through the chain, resolving each method call
        for (i, segment) in chain.iter().enumerate().skip(first_unresolved_segment) {
            debug!(
                "resolve_chain: step {}: resolving '{}' on type {:?}",
                i, segment.name, current_type
            );

            // If this segment has a cast, use it directly.
            //
            // Not when the cast belongs to the *target* segment. `(T) Proxy.
            // newProxyInstance(..)` attaches the cast to the last segment, but
            // the cast describes the call's result, not the receiver the callee
            // is declared on -- casting the result of `Proxy.newProxyInstance`
            // to `T` does not move `newProxyInstance`'s declaration. Taking
            // this branch first replaced the receiver and `continue`d past the
            // navigation below, so a call whose result is cast resolved to
            // nothing at all.
            if i != target_idx {
                if let Some(ref cast_type) = segment.cast_type {
                    debug!("resolve_chain: cast type '{}'", cast_type);
                    let Some(cast_type) = self
                        .create_type_inference_engine(file_id)
                        .parse_type_string(cast_type)
                    else {
                        return Ok(None);
                    };
                    current_type = cast_type;
                    continue;
                }
            }

            // If this is our target segment, navigate to it
            if i == target_idx {
                debug!(
                    "resolve_chain: target segment '{}' on type {:?}",
                    segment.name, current_type
                );

                // Prefer type-based overload resolution when possible.
                // This handles inherited overloads like:
                //   Derived.method(String)
                //   Base.method(int)
                // and correctly picks `method(int)` for call-site `method(42)`.
                let type_fqn_for_matching = match &current_type {
                    Type::Class(c) if c.package.is_some() => Some(format!(
                        "{}.{}",
                        c.package.as_deref().unwrap_or_default(),
                        c.name
                    )),
                    Type::Class(c) => {
                        use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
                        resolve_class_fqn_via_imports(&self.workspace_index, file_id, &c.name).ok()
                    }
                    _ => None,
                };

                if let Some(type_fqn) = type_fqn_for_matching {
                    if !segment.is_method_call {
                        let Some(symbol_loc) =
                            self.find_value_with_inheritance(&type_fqn, &segment.name)
                        else {
                            return Ok(None);
                        };
                        return self.symbol_location_to_lsp_location(&symbol_loc);
                    }
                    if let Some(cursor_token) =
                        identifier_token_at_offset(root, offset, &segment.name)
                    {
                        if let Some(enum_method) =
                            self.find_enum_method_target(&type_fqn, &segment.name)
                        {
                            return self.symbol_location_to_lsp_location(&enum_method);
                        }
                        if let Some(symbol_loc) = self.resolve_method_with_type_matching(
                            file_id,
                            &type_fqn,
                            &segment.name,
                            &cursor_token,
                            root,
                            offset as usize,
                        ) {
                            debug!(
                                "resolve_chain: type-matching resolved '{}' on {}",
                                segment.name, type_fqn
                            );
                            return self.symbol_location_to_lsp_location(&symbol_loc);
                        }
                        // A structured receiver and argument list supplied all
                        // facts required by canonical resolution. A miss is not
                        // permission to select the only same-named declaration.
                        return Ok(None);
                    }
                }

                return self.resolve_method_on_type(&current_type, &segment.name, file_id);
            }

            // Not our target - resolve to get the return type for next iteration
            let member_type = if segment.is_method_call {
                let argument_types =
                    self.infer_chain_segment_argument_types(segment, root, file_id);
                self.get_method_return_type(
                    &current_type,
                    &segment.name,
                    file_id,
                    argument_types.as_deref(),
                )
            } else {
                self.get_value_type(&current_type, &segment.name, file_id)
            };

            if let Some(ty) = member_type {
                debug!("resolve_chain: '{}' has type {:?}", segment.name, ty);
                current_type = ty;
            } else {
                debug!(
                    "resolve_chain: could not resolve '{}' on {:?}",
                    segment.name, current_type
                );
                return Ok(None);
            }
        }

        // If we get here, the target_idx was the first segment
        if target_idx == 0 {
            // Navigate to the variable/class definition
            debug!("resolve_chain: target is first segment, looking up definition");

            // Try the exact lexical binding first.
            let symbol = Self::resolve_variable_symbol(
                &first_segment.name,
                &resolver,
                scope_at_position,
                offset,
            );

            if let Some(symbol) = symbol {
                // Found as a variable - return its location
                if symbol.file_id != file_id {
                    return Ok(None);
                }
                let Some(definition) = resolver.symbol_table().get_symbol(symbol.symbol_id) else {
                    return Ok(None);
                };
                if let Some(path) = self.workspace_index.get_file_path(symbol.file_id) {
                    if let Ok(uri) = tower_lsp::lsp_types::Url::from_file_path(&path) {
                        let Some(range) = crate::navigation_util::byte_range_to_lsp_range(
                            source_text,
                            definition.location.range.start,
                            definition.location.range.end,
                        ) else {
                            return Ok(None);
                        };
                        return Ok(Some(Location { uri, range }));
                    }
                }
            } else {
                // Try as a class name
                use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
                if let Ok(fqn) = resolve_class_fqn_via_imports(
                    &self.workspace_index,
                    file_id,
                    &first_segment.name,
                ) {
                    if let Some(location) = self.workspace_index.find_symbol(&fqn) {
                        return self.symbol_location_to_lsp_location(&location);
                    }
                }
            }
        }

        Ok(None)
    }

    /// Try to resolve a chain via cached expression types in FileSemantics.
    ///
    /// Finds the receiver expression node (the expression before the target segment)
    /// and checks if its type is cached. If so, skips chain walking and resolves
    /// the target segment directly on the cached type.
    fn try_resolve_chain_via_file_semantics(
        &self,
        file_id: FileId,
        chain: &[ChainSegment],
        target_idx: usize,
        offset: u32,
        root: &SyntaxNode,
    ) -> Option<Location> {
        use hyperion_parser::SyntaxKind;
        use hyperion_semantics::db::SemanticDatabase;

        // Find the token at the cursor position
        let token = identifier_token_at_offset(root, offset, &chain[target_idx].name)?;

        // Walk up to find the parent expression (METHOD_CALL_EXPR or FIELD_ACCESS_EXPR)
        let mut node = token.parent()?;
        loop {
            match node.kind() {
                SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::FIELD_ACCESS_EXPR => break,
                _ => node = node.parent()?,
            }
        }

        // The receiver is the first child expression node (before the DOT)
        let receiver_node = node.children().find(|child| {
            matches!(
                child.kind(),
                SyntaxKind::NAME_EXPR
                    | SyntaxKind::METHOD_CALL_EXPR
                    | SyntaxKind::FIELD_ACCESS_EXPR
                    | SyntaxKind::NEW_EXPR
                    | SyntaxKind::PAREN_EXPR
                    | SyntaxKind::THIS_EXPR
            )
        })?;

        let file_sem = self.txn.file_semantics(file_id);

        let receiver_type = file_sem.expression_type_at(receiver_node.text_range())?;
        let bound = self.substitute_type_parameter_bound(receiver_type, file_id, offset, root);
        let receiver_type = bound.as_ref().unwrap_or(receiver_type);

        let target_name = &chain[target_idx].name;
        debug!(
            "resolve_chain: fast path receiver type {:?}, looking up '{}'",
            receiver_type, target_name
        );

        // Prefer type-based overload resolution so calls like `d.method(42)` with
        // `d : Derived extends Base` pick the inherited `Base.method(int)` rather
        // than landing on whichever Derived overload `find_symbol`'s prefix scan
        // happens to return first. Mirrors the equivalent branch in the slow path
        // at the `i == target_idx` step below.
        let type_fqn_for_matching = match receiver_type {
            Type::Class(c) if c.package.is_some() => Some(format!(
                "{}.{}",
                c.package.as_deref().unwrap_or_default(),
                c.name
            )),
            Type::Class(c) => {
                use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
                resolve_class_fqn_via_imports(&self.workspace_index, file_id, &c.name).ok()
            }
            _ => None,
        };

        if let Some(type_fqn) = type_fqn_for_matching {
            if !chain[target_idx].is_method_call {
                let symbol_loc = self.find_value_with_inheritance(&type_fqn, target_name)?;
                return self
                    .symbol_location_to_lsp_location(&symbol_loc)
                    .ok()
                    .flatten();
            }
            if let Some(enum_method) = self.find_enum_method_target(&type_fqn, target_name) {
                return self
                    .symbol_location_to_lsp_location(&enum_method)
                    .ok()
                    .flatten();
            }
            if let Some(symbol_loc) = self.resolve_method_with_type_matching(
                file_id,
                &type_fqn,
                target_name,
                &token,
                root,
                offset as usize,
            ) {
                debug!(
                    "resolve_chain: fast path type-matching resolved on {}",
                    type_fqn
                );
                return self
                    .symbol_location_to_lsp_location(&symbol_loc)
                    .ok()
                    .flatten();
            }
            // The receiver and call syntax were both available, so a canonical
            // miss means no declaration is applicable. Do not guess by name.
            return None;
        }

        match self.resolve_method_on_type(receiver_type, target_name, file_id) {
            Ok(Some(location)) => {
                debug!("resolve_chain: fast path resolved via FileSemantics");
                Some(location)
            }
            _ => None,
        }
    }

    /// Resolve a method on a given type and return its location
    ///
    /// Uses inheritance to find methods defined in parent classes/interfaces.
    pub(crate) fn resolve_method_on_inferred_type(
        &self,
        current_type: &Type,
        method_name: &str,
        file_id: FileId,
    ) -> Result<Option<Location>, String> {
        self.resolve_method_on_type(current_type, method_name, file_id)
    }

    fn resolve_method_on_type(
        &self,
        current_type: &Type,
        method_name: &str,
        file_id: FileId,
    ) -> Result<Option<Location>, String> {
        debug!(
            "resolve_method_on_type: Looking for method '{}' on type {:?}",
            method_name, current_type
        );

        // Handle array types by extracting element type
        let resolved_type = match current_type {
            Type::Array { element_type, .. } => {
                debug!("resolve_method_on_type: Array type detected, extracting element type");
                // Array access - resolve method on element type
                return self.resolve_method_on_type(element_type, method_name, file_id);
            }
            _ => current_type,
        };

        // Get the FQN of the type
        let type_fqn = if let Type::Class(c) = resolved_type {
            if let Some(ref package) = c.package {
                format!("{}.{}", package, c.name)
            } else {
                use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
                let Ok(resolved) =
                    resolve_class_fqn_via_imports(&self.workspace_index, file_id, &c.name)
                else {
                    return Ok(None);
                };
                resolved
            }
        } else {
            debug!("resolve_method_on_type: Type is not a class, cannot resolve method");
            return Ok(None);
        };

        debug!("resolve_method_on_type: Type FQN: {}", type_fqn);

        if let Some(symbol_loc) = self.find_enum_method_target(&type_fqn, method_name) {
            return self.symbol_location_to_lsp_location(&symbol_loc);
        }

        // Use inheritance-aware lookup to find the method
        // This checks the class itself, then parent class, then interfaces
        if let Some(symbol_loc) = self.find_callable_with_inheritance(&type_fqn, method_name) {
            debug!(
                "resolve_method_on_type: Found method '{}' via inheritance on {}",
                method_name, type_fqn
            );
            return self.symbol_location_to_lsp_location(&symbol_loc);
        }

        debug!(
            "resolve_method_on_type: Method '{}' not found on {} (checked inheritance)",
            method_name, type_fqn
        );
        Ok(None)
    }

    fn is_java_lang_enum_base_method(method_name: &str) -> bool {
        matches!(
            method_name,
            "ordinal" | "name" | "compareTo" | "getDeclaringClass"
        )
    }

    /// Prefer a real workspace declaration of a `java.lang.Enum` base method
    /// over the synthetic copy indexed on each enum. When only bytecode/JDK
    /// metadata is available, keep the synthetic declaration so navigation
    /// remains local and source-backed.
    fn find_enum_method_target(
        &self,
        receiver_fqn: &str,
        method_name: &str,
    ) -> Option<hyperion_workspace::SymbolLocation> {
        let receiver = self.workspace_index.get_symbol(receiver_fqn)?;
        if receiver.kind != SymbolKind::ENUM {
            return None;
        }
        if Self::is_java_lang_enum_base_method(method_name) {
            if let Some(location) =
                self.find_callable_with_inheritance("java.lang.Enum", method_name)
            {
                if self.workspace_index.is_workspace_file(location.file_id) {
                    return Some(location);
                }
            }
        }
        if !matches!(method_name, "values" | "valueOf" | "name" | "ordinal") {
            return None;
        }
        let target = hyperion_workspace::SymbolTarget::new(
            format!("{receiver_fqn}.{method_name}"),
            hyperion_workspace::DeclarationNamespace::Callable,
        );
        self.workspace_index
            .find_symbol_definition(&target)
            .map(|definition| definition.location)
    }

    /// Resolve an intermediate field or enum constant to its declared type.
    /// Chain segments are not all calls (`Color.RED.name()` is the canonical
    /// counterexample), so feeding every link to method resolution loses the
    /// receiver before the target is reached.
    fn get_value_type(
        &self,
        current_type: &Type,
        member_name: &str,
        file_id: FileId,
    ) -> Option<Type> {
        let (receiver_fqn, receiver_arguments) =
            self.chain_receiver_identity(current_type, file_id)?;
        if let Some(field) = self.workspace_index.find_projected_effective_field_by_name(
            &receiver_fqn,
            &receiver_arguments,
            member_name,
        ) {
            return hyperion_semantics::type_parser::parse_type_string(
                field.projected_type.as_deref()?,
            );
        }

        // A dotted chain may cross a member type before reaching a value or
        // callable (`Outer.Inner.create()`). Member types occupy a separate
        // namespace from fields, so preserve that type as the next receiver.
        let member_types = self.workspace_index.find_member_definitions_by_name(
            &receiver_fqn,
            member_name,
            hyperion_workspace::DeclarationNamespace::Type,
        );
        let [member_type] = member_types.as_slice() else {
            // Static member types are inherited and may therefore be selected
            // through a subclass qualifier (`Child.Nested`). Reuse the exact
            // static-import election rules so hiding, accessibility, and
            // interface ambiguity stay consistent with ordinary resolution.
            let mut imports = hyperion_workspace::ImportMap::new();
            imports.current_package = self
                .workspace_index
                .get_imports(file_id)
                .and_then(|file_imports| file_imports.current_package);
            imports.add_static_wildcard(receiver_fqn);
            let resolver = hyperion_workspace::import_resolver::ImportResolver::new(
                &imports,
                &self.workspace_index,
            );
            let inherited = resolver.resolve_static_types(member_name);
            let [member_type] = inherited.as_slice() else {
                return None;
            };
            return Some(super::fqn_to_type(&member_type.fqn));
        };
        Some(super::fqn_to_type(&member_type.symbol.fqn))
    }

    /// Get the return type of a method on a given type
    ///
    /// This method now uses TypeInferenceEngine for improved type parsing and resolution.
    /// Uses inheritance to find methods defined in parent classes/interfaces.
    fn get_method_return_type(
        &self,
        current_type: &Type,
        method_name: &str,
        file_id: FileId,
        argument_types: Option<&[Type]>,
    ) -> Option<Type> {
        // Handle array types by extracting element type
        let resolved_type = match current_type {
            Type::Array { element_type, .. } => {
                debug!("get_method_return_type: Array type detected, extracting element type");
                // Array access - resolve method on element type
                return self.get_method_return_type(
                    element_type,
                    method_name,
                    file_id,
                    argument_types,
                );
            }
            _ => current_type,
        };

        let (type_fqn, _) = self.chain_receiver_identity(resolved_type, file_id)?;

        if let Some(argument_types) = argument_types {
            return resolve_call(
                &self.workspace_index,
                &type_fqn,
                Some(resolved_type),
                method_name,
                argument_types,
            )
            .map(|resolved| resolved.return_type);
        }

        self.projected_name_only_method_return_type(resolved_type, file_id, method_name)
    }
}
