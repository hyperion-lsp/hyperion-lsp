// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Chain resolver for type resolution
//!
//! This module provides the core logic for resolving types in method chains.

use super::types::{MemberResolutionStatus, ResolveError, ResolveResult, TypeInfo};
use super::utils::extract_package;
use crate::navigation::chain::ChainSegment;
use crate::navigation::import_resolver::resolve_class_fqn_via_imports;
use hyperion_parser::SyntaxNode;
use hyperion_semantics::call_resolution::resolve_call;
use hyperion_semantics::db::FileId;
use hyperion_semantics::resolver::ResolverBuilder;
use hyperion_semantics::symbol::Type;
use hyperion_semantics::TypeInferenceEngine;
use hyperion_workspace::{DeclarationNamespace, ResolvedDefinition, SymbolTarget, WorkspaceIndex};
use std::sync::Arc;
use tower_lsp::lsp_types::SymbolKind;
use tracing::debug;

/// Resolves types in method chains and qualified accesses
pub struct ChainResolver {
    workspace_index: Arc<WorkspaceIndex>,
    file_semantics: Option<Arc<hyperion_semantics::file_semantics::FileSemantics>>,
}

struct ResolvedMember {
    definition: ResolvedDefinition,
    projected_return_type: Option<Type>,
    parameter_declarations: Option<Vec<String>>,
    status: MemberResolutionStatus,
}

struct CallableRecovery {
    definition: Option<ResolvedDefinition>,
    owner_fqn: String,
    status: MemberResolutionStatus,
}

impl ChainResolver {
    /// Create a new ChainResolver
    pub const fn new(workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self {
            workspace_index,
            file_semantics: None,
        }
    }

    /// Set pre-computed file semantics for expression type fast paths.
    pub fn with_file_semantics(
        mut self,
        file_sem: Arc<hyperion_semantics::file_semantics::FileSemantics>,
    ) -> Self {
        self.file_semantics = Some(file_sem);
        self
    }

    /// Resolve a method chain to get the final segment's type
    ///
    /// Example: For `user.getProfile().getName()` with target_idx=2,
    /// this resolves the type of `getName()` return value.
    ///
    /// # Arguments
    ///
    /// * `file_id` - The file containing the chain
    /// * `chain` - The parsed chain segments
    /// * `target_idx` - Index of the target segment in the chain
    /// * `offset` - Byte offset in the file
    /// * `root` - The AST root node
    ///
    /// # Returns
    ///
    /// TypeInfo for the resolved type
    pub fn resolve_chain_type(
        &self,
        file_id: FileId,
        chain: &[ChainSegment],
        target_idx: usize,
        offset: u32,
        root: &SyntaxNode,
    ) -> ResolveResult<TypeInfo> {
        use tracing::info;

        if chain.is_empty() {
            return Err(ResolveError::Other("Empty chain".to_string()));
        }

        if target_idx >= chain.len() {
            return Err(ResolveError::Other(format!(
                "Target index {} out of bounds for chain of length {}",
                target_idx,
                chain.len()
            )));
        }

        let chain_str: Vec<_> = chain.iter().map(|s| &s.name).collect();
        info!(
            "TypeResolver: resolving chain {:?} -> target '{}'",
            chain_str, chain[target_idx].name
        );

        // Fast path: check cached expression types from FileSemantics
        if let Some(ref file_sem) = self.file_semantics {
            if let Some(type_info) = self.try_resolve_via_expression_types(
                file_sem,
                file_id,
                offset,
                root,
                &chain[target_idx],
            ) {
                debug!("TypeResolver fast path: resolved via FileSemantics");
                return Ok(type_info);
            }
        }

        // Step 1: Resolve the first segment (variable, field, or class)
        let mut current_type = self.resolve_first_segment(file_id, &chain[0], offset, root)?;

        debug!(
            "First segment '{}' resolved to type: {}",
            chain[0].name, current_type.fqn
        );

        // Step 2: Iterate through remaining segments up to target
        for (idx, segment) in chain.iter().enumerate().skip(1) {
            if idx > target_idx {
                break;
            }

            debug!(
                "Resolving segment {}/{}: {} (is_method_call={})",
                idx, target_idx, segment.name, segment.is_method_call
            );

            // Resolve this segment on the current type
            current_type = self.resolve_segment_on_type(
                file_id,
                &current_type,
                segment,
                idx == target_idx,
                offset,
                root,
            )?;

            debug!("Segment {} resolved to type: {:?}", idx, current_type);
        }

        Ok(current_type)
    }

    /// Resolve a simple qualified access to get the member's type
    ///
    /// Example: For `user.getName()`, resolve the return type of getName()
    pub fn resolve_qualified_type(
        &self,
        file_id: FileId,
        receiver: &str,
        member: &str,
        offset: u32,
        root: &SyntaxNode,
    ) -> ResolveResult<TypeInfo> {
        debug!(
            "Resolving qualified access: {}.{} at offset {}",
            receiver, member, offset
        );

        // Create a simple chain
        let chain = vec![
            ChainSegment {
                name: receiver.to_string(),
                is_method_call: false,
                cast_type: None,
                call_range: None,
            },
            ChainSegment {
                name: member.to_string(),
                is_method_call: true, // Assume method call for hover
                cast_type: None,
                call_range: None,
            },
        ];

        self.resolve_chain_type(file_id, &chain, 1, offset, root)
    }

    /// Try to resolve a chain segment's type via cached expression types in FileSemantics.
    ///
    /// Finds the expression node at the cursor offset, checks for a cached type, and
    /// converts it to TypeInfo. Returns None to fall through to standard chain resolution.
    fn try_resolve_via_expression_types(
        &self,
        file_sem: &hyperion_semantics::file_semantics::FileSemantics,
        file_id: FileId,
        offset: u32,
        root: &SyntaxNode,
        target_segment: &ChainSegment,
    ) -> Option<TypeInfo> {
        use hyperion_parser::SyntaxKind;

        // Find the token at cursor
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()?;

        // Walk up to find the enclosing expression
        let mut node = token.parent()?;
        loop {
            match node.kind() {
                SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::NEW_EXPR
                | SyntaxKind::CAST_EXPR => {
                    if let Some(ty) = file_sem.expression_type_at(node.text_range()) {
                        debug!(
                            "TypeResolver fast path: found cached type {:?} for '{}'",
                            ty, target_segment.name
                        );
                        return self.type_to_type_info(file_id, ty).ok();
                    }
                    break;
                }
                _ => node = node.parent()?,
            }
        }
        None
    }

    /// Resolve the first segment in a chain (variable, field, or class)
    fn resolve_first_segment(
        &self,
        file_id: FileId,
        segment: &ChainSegment,
        offset: u32,
        root: &SyntaxNode,
    ) -> ResolveResult<TypeInfo> {
        let name = &segment.name;

        // If this segment has a cast type, use it directly
        if let Some(ref cast_type) = segment.cast_type {
            debug!("Using cast type '{}' for segment '{}'", cast_type, name);
            return self.resolve_type_by_name(file_id, cast_type);
        }

        // Try to resolve as a variable or field using semantic analysis

        // Build symbol resolver from syntax tree with workspace index for method resolution
        let builder = ResolverBuilder::with_workspace(file_id, self.workspace_index.clone());
        let resolver = builder.build_from_syntax(root);

        // Find scope at position
        let scope_at_position = resolver.find_scope_at_offset(offset);

        // Try to resolve in scope
        let resolved_symbol = scope_at_position
            .and_then(|scope_id| resolver.resolve_in_scope_at_offset(name, scope_id, offset));

        if let Some(symbol) = resolved_symbol {
            debug!("Resolved first segment '{}' as variable/field", name);
            let symbol_data = resolver.symbol_table().get_symbol(symbol.symbol_id);

            if let Some(sym) = symbol_data {
                // Convert semantic Type to TypeInfo
                return self.type_to_type_info(file_id, &sym.type_info);
            }
        }

        // Not a variable - check if it's a method call (static import)
        if segment.is_method_call {
            debug!(
                "First segment '{}' is method call, trying static import",
                name
            );

            use crate::navigation::argument_inference::infer_argument_types_with_semantics;
            use crate::navigation::import_resolver::resolve_static_invocation_via_imports;
            let argument_types = segment.call_range.and_then(|call_range| {
                let call = root.descendants().find(|node| {
                    node.kind() == hyperion_parser::syntax::SyntaxKind::METHOD_CALL_EXPR
                        && node.text_range() == call_range
                })?;
                let arguments = call
                    .children()
                    .find(|node| node.kind() == hyperion_parser::syntax::SyntaxKind::ARG_LIST)?;
                Some(
                    infer_argument_types_with_semantics(
                        &arguments,
                        file_id,
                        &self.workspace_index,
                        root,
                        offset,
                        self.file_semantics.as_deref(),
                    )
                    .into_iter()
                    .map(|inferred| inferred.ty)
                    .collect::<Vec<_>>(),
                )
            });
            if let Some(method_location) = argument_types.as_deref().and_then(|arguments| {
                resolve_static_invocation_via_imports(
                    &self.workspace_index,
                    file_id,
                    name,
                    arguments,
                )
            }) {
                debug!("Resolved static import: {}", method_location.fqn);

                // Get the method symbol to find its return type
                if let Some(method_symbol) = self.workspace_index.get_symbol_target(
                    &SymbolTarget::new(method_location.fqn.clone(), DeclarationNamespace::Callable),
                ) {
                    if let Some(return_type_str) = &method_symbol.return_type {
                        debug!("Static method '{}' returns: {}", name, return_type_str);

                        // Resolve return type using the import context from where the method is defined
                        return self.resolve_type_by_name(method_location.file_id, return_type_str);
                    } else {
                        debug!("Static method '{}' has no return type", name);
                        return Err(ResolveError::Other(format!(
                            "Static method '{name}' has no return type"
                        )));
                    }
                } else {
                    debug!(
                        "Could not find method symbol for static import '{}'",
                        method_location.fqn
                    );
                    return Err(ResolveError::Other(format!(
                        "Could not find method symbol for static import '{}'",
                        method_location.fqn
                    )));
                }
            }
        }

        // Not a variable or static import - try as a class name (static access)
        debug!(
            "First segment '{}' not found as variable or static import, trying as class",
            name
        );

        self.resolve_type_by_name(file_id, name)
    }

    /// Resolve a segment on the current type
    fn resolve_segment_on_type(
        &self,
        file_id: FileId,
        current_type: &TypeInfo,
        segment: &ChainSegment,
        is_target: bool,
        offset: u32,
        root: &SyntaxNode,
    ) -> ResolveResult<TypeInfo> {
        let member_name = &segment.name;
        let class_fqn = &current_type.fqn;

        debug!(
            "Looking up member '{}' on type '{}' (is_method_call={}, is_target={})",
            member_name, class_fqn, segment.is_method_call, is_target
        );

        let owner_fqn = class_fqn.strip_suffix("[]").unwrap_or(class_fqn);
        let receiver_type = hyperion_semantics::type_parser::parse_type_string(&if current_type
            .type_arguments
            .is_empty()
        {
            owner_fqn.to_string()
        } else {
            format!("{owner_fqn}<{}>", current_type.type_arguments.join(", "))
        });
        let member_result = if segment.is_method_call {
            let applicable = self.resolve_callable_segment(
                file_id,
                owner_fqn,
                receiver_type.as_ref(),
                segment,
                offset,
                root,
            );
            if applicable.is_some() {
                applicable
            } else if is_target || segment.call_range.is_none() {
                // Hover needs enough owner/name information to explain an
                // invalid overload set, but selecting a representative
                // declaration would fabricate an identity. Recover either one
                // exact callable or an explicit identity-free method group.
                let recovery = self.recover_callable_target(
                    file_id,
                    owner_fqn,
                    &current_type.type_arguments,
                    member_name,
                );
                match recovery {
                    Some(CallableRecovery {
                        definition: Some(definition),
                        status,
                        ..
                    }) => Some(ResolvedMember {
                        definition,
                        projected_return_type: None,
                        parameter_declarations: None,
                        status,
                    }),
                    Some(CallableRecovery {
                        definition: None,
                        owner_fqn,
                        status,
                    }) => {
                        return Ok(TypeInfo {
                            simple_name: member_name.clone(),
                            fqn: format!("{owner_fqn}.{member_name}"),
                            package: extract_package(&owner_fqn),
                            kind: SymbolKind::METHOD,
                            type_arguments: Vec::new(),
                            documentation: None,
                            return_type: None,
                            file_id: None,
                            container_name: Some(owner_fqn),
                            parameters: Vec::new(),
                            member_resolution: Some(status),
                        });
                    }
                    None => None,
                }
            } else {
                None
            }
        } else {
            self.workspace_index
                .find_projected_effective_field_by_name(
                    owner_fqn,
                    &current_type.type_arguments,
                    member_name,
                )
                .map(|field| {
                    let projected_return_type = field
                        .projected_type
                        .as_deref()
                        .and_then(hyperion_semantics::type_parser::parse_type_string);
                    let status = if projected_return_type.is_some() {
                        MemberResolutionStatus::Applicable
                    } else {
                        MemberResolutionStatus::ProjectionUnavailable
                    };
                    ResolvedMember {
                        definition: field.definition,
                        projected_return_type,
                        parameter_declarations: None,
                        status,
                    }
                })
                .or_else(|| {
                    // Properties and constants are language-specific value
                    // declarations, not Java fields; retain their old lookup.
                    self.find_member_with_inheritance(
                        file_id,
                        owner_fqn,
                        member_name,
                        DeclarationNamespace::Value,
                    )
                    .map(|definition| ResolvedMember {
                        definition,
                        projected_return_type: None,
                        parameter_declarations: None,
                        status: MemberResolutionStatus::NameOnly,
                    })
                })
        };

        let member = member_result.ok_or_else(|| {
            if segment.is_method_call {
                ResolveError::MethodNotFound {
                    type_fqn: class_fqn.to_string(),
                    method_name: member_name.clone(),
                }
            } else {
                ResolveError::FieldNotFound {
                    type_fqn: class_fqn.to_string(),
                    field_name: member_name.clone(),
                }
            }
        })?;

        let sym = &member.definition.symbol;
        debug!(
            "Found member '{}' at {:?}",
            member.definition.target.fqn, member.definition.location
        );
        {
            debug!(
                "Resolved symbol '{}' kind={:?} return_type={:?}",
                member.definition.target.fqn, sym.kind, sym.return_type
            );
            // If this is a method call and not the target, we need the return type
            if segment.is_method_call && !is_target {
                if member.status == MemberResolutionStatus::ProjectionUnavailable {
                    return Err(ResolveError::Other(format!(
                        "Method '{member_name}' has no coherent receiver-relative signature"
                    )));
                }
                if let Some(return_type) = member.projected_return_type.as_ref() {
                    return self.type_to_type_info(member.definition.location.file_id, return_type);
                }
                if let Some(return_type_name) = &sym.return_type {
                    debug!(
                        "Getting return type '{}' for method '{}'",
                        return_type_name, member_name
                    );

                    // Resolve return type using the import context from where the method is defined
                    return self.resolve_type_by_name(
                        member.definition.location.file_id,
                        return_type_name,
                    );
                }
                return Err(ResolveError::Other(format!(
                    "Method '{member_name}' has no return type information"
                )));
            }
            if !segment.is_method_call && !is_target {
                if member.status == MemberResolutionStatus::ProjectionUnavailable {
                    return Err(ResolveError::Other(format!(
                        "Field '{member_name}' has no coherent receiver-relative type"
                    )));
                }
                if let Some(field_type) = member.projected_return_type.as_ref() {
                    return self.type_to_type_info(member.definition.location.file_id, field_type);
                }
            }

            // For the target or field access, return the member's type info
            Ok(TypeInfo {
                simple_name: sym.simple_name.to_string(),
                fqn: sym.fqn.to_string(),
                package: extract_package(&sym.fqn),
                kind: sym.kind,
                type_arguments: vec![],
                documentation: sym.documentation.as_ref().map(|s| s.to_string()),
                return_type: match member.status {
                    MemberResolutionStatus::ProjectionUnavailable => None,
                    MemberResolutionStatus::Applicable => member
                        .projected_return_type
                        .as_ref()
                        .filter(|ty| !matches!(ty, Type::Unknown))
                        .map(ToString::to_string),
                    MemberResolutionStatus::NameOnly => {
                        sym.return_type.as_ref().map(ToString::to_string)
                    }
                },
                parameters: match member.status {
                    MemberResolutionStatus::ProjectionUnavailable => Vec::new(),
                    MemberResolutionStatus::Applicable => {
                        member.parameter_declarations.unwrap_or_default()
                    }
                    MemberResolutionStatus::NameOnly => {
                        sym.parameters.iter().map(ToString::to_string).collect()
                    }
                },
                file_id: Some(member.definition.location.file_id),
                container_name: sym.container.as_ref().map(|s| s.to_string()),
                member_resolution: Some(member.status),
            })
        }
    }

    /// Resolve a structured call before using its return type for the next
    /// chain segment. Descriptorless lookup is not sufficient here: overloads
    /// may have unrelated return types, and `get_symbol("Owner.method")`
    /// intentionally fails when that key denotes more than one declaration.
    fn resolve_callable_segment(
        &self,
        file_id: FileId,
        owner_fqn: &str,
        receiver_type: Option<&Type>,
        segment: &ChainSegment,
        offset: u32,
        root: &SyntaxNode,
    ) -> Option<ResolvedMember> {
        use crate::navigation::argument_inference::infer_argument_types_with_semantics;

        let call_range = segment.call_range?;
        let call = root.descendants().find(|node| {
            node.kind() == hyperion_parser::syntax::SyntaxKind::METHOD_CALL_EXPR
                && node.text_range() == call_range
        })?;
        let arguments = call
            .children()
            .find(|node| node.kind() == hyperion_parser::syntax::SyntaxKind::ARG_LIST)?;
        let argument_types = infer_argument_types_with_semantics(
            &arguments,
            file_id,
            &self.workspace_index,
            root,
            offset,
            self.file_semantics.as_deref(),
        )
        .into_iter()
        .map(|inferred| inferred.ty)
        .collect::<Vec<_>>();

        for owner in self.class_lookup_candidates(file_id, owner_fqn) {
            let Some(resolved) = resolve_call(
                &self.workspace_index,
                &owner,
                receiver_type,
                &segment.name,
                &argument_types,
            ) else {
                continue;
            };
            return Some(ResolvedMember {
                definition: resolved.definition,
                projected_return_type: Some(resolved.return_type),
                parameter_declarations: Some(resolved.parameter_declarations),
                status: MemberResolutionStatus::Applicable,
            });
        }
        None
    }

    fn recover_callable_target(
        &self,
        file_id: FileId,
        owner_fqn: &str,
        receiver_arguments: &[String],
        method_name: &str,
    ) -> Option<CallableRecovery> {
        self.class_lookup_candidates(file_id, owner_fqn)
            .into_iter()
            .find_map(|owner| {
                let candidates = self
                    .workspace_index
                    .find_projected_effective_methods_by_name(
                        &owner,
                        receiver_arguments,
                        method_name,
                    );
                if candidates.is_empty() {
                    return None;
                }
                let status = if candidates
                    .iter()
                    .any(|candidate| candidate.projection.is_none())
                {
                    MemberResolutionStatus::ProjectionUnavailable
                } else {
                    MemberResolutionStatus::NameOnly
                };
                let [candidate] = candidates.as_slice() else {
                    return Some(CallableRecovery {
                        definition: None,
                        owner_fqn: owner,
                        status,
                    });
                };
                Some(CallableRecovery {
                    definition: Some(candidate.definition.clone()),
                    owner_fqn: owner,
                    status,
                })
            })
    }

    /// Find a member with inheritance support
    fn find_member_with_inheritance(
        &self,
        file_id: FileId,
        class_fqn: &str,
        member_name: &str,
        namespace: DeclarationNamespace,
    ) -> Option<ResolvedDefinition> {
        let roots = self.class_lookup_candidates(file_id, class_fqn);
        let [root] = roots.as_slice() else {
            return None;
        };
        if namespace == DeclarationNamespace::Value {
            return self
                .workspace_index
                .find_value_definition_with_inheritance(root, member_name);
        }
        if namespace == DeclarationNamespace::Callable {
            let candidates = self
                .workspace_index
                .find_projected_effective_methods_by_name(root, &[], member_name);
            let [candidate] = candidates.as_slice() else {
                return None;
            };
            return Some(candidate.definition.clone());
        }
        None
    }

    fn class_lookup_candidates(&self, file_id: FileId, class_fqn: &str) -> Vec<String> {
        match self
            .workspace_index
            .resolve_type_name(file_id, &[], class_fqn)
        {
            hyperion_workspace::TypeNameResolution::Exact(resolved) => vec![resolved],
            hyperion_workspace::TypeNameResolution::Missing
            | hyperion_workspace::TypeNameResolution::Ambiguous(_) => Vec::new(),
        }
    }

    fn normalize_class_fqn(&self, file_id: FileId, raw: &str) -> String {
        let segments: Vec<&str> = raw.split('.').collect();
        if segments.is_empty() {
            return raw.to_string();
        }

        let first_segment = segments[0];
        let starts_with_lowercase = first_segment
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase());

        if starts_with_lowercase {
            return raw.to_string();
        }

        if let Ok(resolved_outer) =
            resolve_class_fqn_via_imports(&self.workspace_index, file_id, first_segment)
        {
            let mut fqn = resolved_outer;
            for part in segments.iter().skip(1) {
                fqn.push('.');
                fqn.push_str(part);
            }
            return fqn;
        }

        raw.to_string()
    }

    /// Resolve a type by name in the context of a specific file
    ///
    /// Uses import resolution to convert simple names to FQNs
    pub fn resolve_type_by_name(
        &self,
        file_id: FileId,
        type_name: &str,
    ) -> ResolveResult<TypeInfo> {
        debug!(
            "Resolving type by name: '{}' in file {:?}",
            type_name, file_id
        );

        // Use TypeInferenceEngine to parse the type string
        // This handles primitives, arrays, generics automatically
        let engine = TypeInferenceEngine::new(self.workspace_index.clone(), file_id);
        let parsed_type = match engine.parse_type_string(type_name) {
            Some(t) => t,
            None => {
                debug!("Failed to parse type string: '{}'", type_name);
                return Err(ResolveError::TypeNotFound(type_name.to_string()));
            }
        };

        debug!(
            "TypeInferenceEngine parsed '{}' as {:?}",
            type_name, parsed_type
        );

        // For class types without packages, resolve via imports
        let resolved_type = if let Type::Class(ref class_type) = parsed_type {
            if class_type.package.is_none() {
                // Simple class name - resolve FQN via imports
                if let Ok(fqn) =
                    resolve_class_fqn_via_imports(&self.workspace_index, file_id, &class_type.name)
                {
                    debug!(
                        "Resolved simple type '{}' to FQN '{}'",
                        class_type.name, fqn
                    );
                    match engine.parse_type_string(&fqn) {
                        Some(Type::Class(mut resolved)) => {
                            resolved
                                .type_arguments
                                .clone_from(&class_type.type_arguments);
                            Type::Class(resolved)
                        }
                        _ => parsed_type,
                    }
                } else {
                    parsed_type
                }
            } else {
                parsed_type
            }
        } else {
            parsed_type
        };

        // Convert Type to TypeInfo using existing conversion
        self.type_to_type_info(file_id, &resolved_type)
            .map_err(|_| ResolveError::TypeNotFound(type_name.to_string()))
    }

    /// Convert semantic Type to TypeInfo
    fn type_to_type_info(&self, file_id: FileId, ty: &Type) -> ResolveResult<TypeInfo> {
        match ty {
            Type::Class(class_type) => {
                let raw_name = if let Some(ref package) = class_type.package {
                    format!("{}.{}", package, class_type.name)
                } else {
                    class_type.name.to_string()
                };

                let fqn = self.normalize_class_fqn(file_id, &raw_name);
                let simple_name = fqn.rsplit('.').next().unwrap_or(&fqn).to_string();

                // Get additional info from workspace
                let (kind, documentation) =
                    if let Some(symbol) = self.workspace_index.get_symbol(&fqn) {
                        (
                            symbol.kind,
                            symbol.documentation.as_ref().map(|s| s.to_string()),
                        )
                    } else {
                        (SymbolKind::CLASS, None)
                    };

                Ok(TypeInfo {
                    simple_name,
                    fqn: fqn.to_string(),
                    package: extract_package(&fqn),
                    kind,
                    type_arguments: class_type
                        .type_arguments
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    documentation,
                    return_type: None,
                    parameters: vec![],
                    file_id: Some(file_id),
                    container_name: None,
                    member_resolution: None,
                })
            }
            Type::Primitive(prim) => {
                let name = format!("{prim:?}").to_lowercase();
                Ok(TypeInfo {
                    simple_name: name.clone(),
                    fqn: name.clone(),
                    package: None,
                    kind: SymbolKind::CLASS, // Primitives are represented as CLASS
                    type_arguments: vec![],
                    documentation: Some(format!("Primitive type: {name}")),
                    return_type: None,
                    parameters: vec![],
                    file_id: None,
                    container_name: None,
                    member_resolution: None,
                })
            }
            Type::Array {
                element_type,
                dimensions,
            } => {
                // Recursively resolve element type
                let elem_info = self.type_to_type_info(file_id, element_type)?;
                let brackets = "[]".repeat(*dimensions as usize);
                Ok(TypeInfo {
                    simple_name: format!("{}{}", elem_info.simple_name, brackets),
                    fqn: format!("{}{}", elem_info.fqn, brackets),
                    package: elem_info.package,
                    kind: SymbolKind::CLASS,
                    type_arguments: vec![],
                    documentation: Some(format!(
                        "{}D array of {}",
                        dimensions, elem_info.simple_name
                    )),
                    return_type: None,
                    parameters: vec![],
                    file_id: None,
                    container_name: None,
                    member_resolution: None,
                })
            }
            Type::TypeVariable { name, bounds: _ } => {
                // For type variables (like T, E), return the name as a simple type
                Ok(TypeInfo {
                    simple_name: name.clone(),
                    fqn: name.clone(),
                    package: None,
                    kind: SymbolKind::TYPE_PARAMETER,
                    type_arguments: vec![],
                    documentation: Some(format!("Type parameter: {name}")),
                    return_type: None,
                    parameters: vec![],
                    file_id: None,
                    container_name: None,
                    member_resolution: None,
                })
            }
            Type::Wildcard {
                upper_bound,
                lower_bound: _,
            } => {
                // For wildcards, use the upper bound if present, otherwise Object
                if let Some(bound) = upper_bound {
                    self.type_to_type_info(file_id, bound)
                } else {
                    Ok(TypeInfo {
                        simple_name: "Object".to_string(),
                        fqn: "java.lang.Object".to_string(),
                        package: Some("java.lang".to_string()),
                        kind: SymbolKind::CLASS,
                        type_arguments: vec![],
                        documentation: Some("Wildcard type".to_string()),
                        return_type: None,
                        parameters: vec![],
                        file_id: None,
                        container_name: None,
                        member_resolution: None,
                    })
                }
            }
            Type::Intersection { types } => {
                // For intersection types, use the first type (common case: T & SomeInterface)
                if let Some(first) = types.first() {
                    self.type_to_type_info(file_id, first)
                } else {
                    Err(ResolveError::Other("Empty intersection type".to_string()))
                }
            }
            Type::Union { types } => {
                // For union types (multi-catch), use the first type
                if let Some(first) = types.first() {
                    self.type_to_type_info(file_id, first)
                } else {
                    Err(ResolveError::Other("Empty union type".to_string()))
                }
            }
            Type::Null => Ok(TypeInfo {
                simple_name: "null".to_string(),
                fqn: "null".to_string(),
                package: None,
                kind: SymbolKind::NULL,
                type_arguments: vec![],
                documentation: Some("null type".to_string()),
                return_type: None,
                parameters: vec![],
                file_id: None,
                container_name: None,
                member_resolution: None,
            }),
            Type::Void => Ok(TypeInfo {
                simple_name: "void".to_string(),
                fqn: "void".to_string(),
                package: None,
                kind: SymbolKind::CLASS,
                type_arguments: vec![],
                documentation: Some("void type".to_string()),
                return_type: None,
                parameters: vec![],
                file_id: None,
                container_name: None,
                member_resolution: None,
            }),
            Type::Unknown => Err(ResolveError::TypeNotFound("unknown".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn class_lookup_candidates_returns_only_the_canonical_source_identity() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let file_path = root.join("Outer.java");

        fs::write(
            &file_path,
            r"
                package com.example;

                public class Outer {
                    public static class Inner {}
                }
            ",
        )
        .unwrap();

        let workspace_index = WorkspaceIndex::new(root.clone());
        workspace_index.index_file(&file_path).await.unwrap();

        let file_id = workspace_index
            .get_file_id(&file_path)
            .expect("file should be indexed");

        let resolver = ChainResolver::new(Arc::new(workspace_index.clone()));
        let candidates = resolver.class_lookup_candidates(file_id, "Outer.Inner");

        assert_eq!(candidates, ["com.example.Outer.Inner"]);
    }

    #[tokio::test]
    async fn class_lookup_candidates_do_not_guess_an_ambiguous_nested_type_suffix() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let alpha = root.join("alpha/Outer.java");
        let beta = root.join("beta/Outer.java");
        let usage = root.join("use/Use.java");
        for path in [&alpha, &beta, &usage] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
        }
        fs::write(
            &alpha,
            "package alpha; public class Outer { public static class Inner {} }",
        )
        .unwrap();
        fs::write(
            &beta,
            "package beta; public class Outer { public static class Inner {} }",
        )
        .unwrap();
        fs::write(
            &usage,
            "package use; import alpha.*; import beta.*; class Use {}",
        )
        .unwrap();

        let workspace_index = WorkspaceIndex::new(root);
        workspace_index.index_file(&alpha).await.unwrap();
        workspace_index.index_file(&beta).await.unwrap();
        workspace_index.index_file(&usage).await.unwrap();
        let file_id = workspace_index.get_file_id(&usage).unwrap();
        let resolver = ChainResolver::new(Arc::new(workspace_index));

        assert_eq!(
            resolver.class_lookup_candidates(file_id, "Outer.Inner"),
            Vec::<String>::new(),
            "ambiguous imports must not emit textual or binary-name guesses"
        );
    }

    #[tokio::test]
    async fn inherited_member_walk_uses_each_declarations_canonical_edges() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let grandparent = root.join("Grandparent.java");
        let parent = root.join("Parent.java");
        let child = root.join("Child.java");
        fs::write(
            &grandparent,
            "package p; public class Grandparent { public String inherited; }",
        )
        .unwrap();
        fs::write(
            &parent,
            "package p; public class Parent extends Grandparent {}",
        )
        .unwrap();
        fs::write(&child, "package p; public class Child extends Parent {}").unwrap();

        let workspace_index = WorkspaceIndex::new(root);
        workspace_index.index_file(&grandparent).await.unwrap();
        workspace_index.index_file(&parent).await.unwrap();
        workspace_index.index_file(&child).await.unwrap();
        let file_id = workspace_index.get_file_id(&child).unwrap();
        let resolver = ChainResolver::new(Arc::new(workspace_index));

        let definition = resolver
            .find_member_with_inheritance(
                file_id,
                "p.Child",
                "inherited",
                DeclarationNamespace::Value,
            )
            .expect("grandparent member should remain reachable through source-spelled edges");
        assert_eq!(definition.target.fqn.as_ref(), "p.Grandparent.inherited");
        assert_eq!(
            definition.location.file_id,
            resolver.workspace_index.get_file_id(&grandparent).unwrap()
        );
    }

    #[tokio::test]
    async fn name_only_callable_recovery_refuses_inherited_overloads() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Types.java");
        fs::write(
            &path,
            concat!(
                "package p; interface Left { void choose(String value); } ",
                "interface Right { void choose(Integer value); } ",
                "class Child implements Left, Right {}"
            ),
        )
        .unwrap();

        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace_index.index_file(&path).await.unwrap();
        let file_id = workspace_index.get_file_id(&path).unwrap();
        let resolver = ChainResolver::new(Arc::new(workspace_index));

        assert!(
            resolver
                .find_member_with_inheritance(
                    file_id,
                    "p.Child",
                    "choose",
                    DeclarationNamespace::Callable,
                )
                .is_none(),
            "a name-only hover recovery must not attribute an overload to whichever interface is visited first"
        );
    }

    #[tokio::test]
    async fn inherited_value_walk_skips_private_parent_before_interface() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let named = root.join("Named.java");
        let parent = root.join("Parent.java");
        let child = root.join("Child.java");
        fs::write(
            &named,
            "package p; public interface Named { int VALUE = 1; }",
        )
        .unwrap();
        fs::write(
            &parent,
            "package p; public class Parent { private int VALUE = 2; }",
        )
        .unwrap();
        fs::write(
            &child,
            "package p; public class Child extends Parent implements Named {}",
        )
        .unwrap();

        let workspace_index = WorkspaceIndex::new(root);
        workspace_index.index_file(&named).await.unwrap();
        workspace_index.index_file(&parent).await.unwrap();
        workspace_index.index_file(&child).await.unwrap();
        let file_id = workspace_index.get_file_id(&child).unwrap();
        let resolver = ChainResolver::new(Arc::new(workspace_index));

        let definition = resolver
            .find_member_with_inheritance(file_id, "p.Child", "VALUE", DeclarationNamespace::Value)
            .expect("interface constant should remain inherited");
        assert_eq!(definition.target.fqn.as_ref(), "p.Named.VALUE");
        assert_eq!(
            definition.location.file_id,
            resolver.workspace_index.get_file_id(&named).unwrap()
        );
    }

    #[tokio::test]
    async fn package_value_does_not_reappear_after_cross_package_inheritance() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let named = root.join("Named.java");
        let base = root.join("Base.java");
        let middle = root.join("Middle.java");
        let leaf = root.join("Leaf.java");
        fs::write(
            &named,
            "package p; public interface Named { int VALUE = 1; }",
        )
        .unwrap();
        fs::write(&base, "package p; public class Base { int VALUE = 2; }").unwrap();
        fs::write(&middle, "package q; public class Middle extends p.Base {}").unwrap();
        fs::write(
            &leaf,
            "package p; public class Leaf extends q.Middle implements Named {}",
        )
        .unwrap();

        let workspace_index = WorkspaceIndex::new(root);
        workspace_index.index_file(&named).await.unwrap();
        workspace_index.index_file(&base).await.unwrap();
        workspace_index.index_file(&middle).await.unwrap();
        workspace_index.index_file(&leaf).await.unwrap();
        let file_id = workspace_index.get_file_id(&leaf).unwrap();
        let resolver = ChainResolver::new(Arc::new(workspace_index));

        let definition = resolver
            .find_member_with_inheritance(file_id, "p.Leaf", "VALUE", DeclarationNamespace::Value)
            .expect("interface constant should survive the inaccessible package field");
        assert_eq!(definition.target.fqn.as_ref(), "p.Named.VALUE");
        assert_eq!(
            definition.location.file_id,
            resolver.workspace_index.get_file_id(&named).unwrap()
        );
    }

    #[test]
    fn target_recovery_does_not_expose_raw_signature_for_conflicting_projection() {
        use hyperion_parser::{Lexer, Parser};

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("p/Types.java");
        let source = concat!(
            "package p; class Text {} class Bound {} ",
            "interface Root<T> { T conflict(T value); } ",
            "interface Left extends Root<Text> {} ",
            "interface Right extends Root<Bound> {} ",
            "interface Conflict extends Left, Right {}"
        );
        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace_index
            .index_file_with_source(&path, source)
            .expect("conflicting hierarchy should remain indexable for recovery");
        let file_id = workspace_index.get_file_id(&path).unwrap();
        let resolver = ChainResolver::new(Arc::new(workspace_index));
        let tokens = Lexer::new("class Use {}").collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty());
        let root = SyntaxNode::new_root(green);
        let receiver = TypeInfo {
            simple_name: "Conflict".to_string(),
            fqn: "p.Conflict".to_string(),
            package: Some("p".to_string()),
            kind: SymbolKind::INTERFACE,
            type_arguments: Vec::new(),
            documentation: None,
            return_type: None,
            file_id: Some(file_id),
            container_name: None,
            parameters: Vec::new(),
            member_resolution: None,
        };
        let segment = ChainSegment {
            name: "conflict".to_string(),
            is_method_call: true,
            cast_type: None,
            call_range: None,
        };

        let recovered = resolver
            .resolve_segment_on_type(file_id, &receiver, &segment, true, 0, &root)
            .expect("target name should remain recoverable");
        assert_eq!(
            recovered.member_resolution,
            Some(MemberResolutionStatus::ProjectionUnavailable)
        );
        assert!(recovered.return_type.is_none());
        assert!(recovered.parameters.is_empty());
    }
}
