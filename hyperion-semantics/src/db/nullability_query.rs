// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Salsa implementation and source-level helpers for Java nullability analysis.

use super::{FileId, Language, SemanticDatabase};
use crate::nullability::VariableResolver;
use hyperion_parser::{SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

fn name_expr_ident(node: &SyntaxNode) -> Option<String> {
    for elem in node.children_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = elem {
            if token.kind().is_identifier() {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

const fn is_expression(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::IDENT
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::PATTERN_INSTANCEOF_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::CONSTRUCTOR_REF_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::SWITCH_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
    )
}

fn declaration_type_name(node: &SyntaxNode) -> Option<String> {
    crate::nullability::signature_cache::extract_declared_type(node)
}

fn expression_owner(
    expression: &SyntaxNode,
    field_types: &HashMap<String, String>,
    call_results: &HashMap<u32, String>,
    current_class: &str,
) -> Option<String> {
    match expression.kind() {
        SyntaxKind::NAME_EXPR => {
            let name = name_expr_ident(expression)?;
            crate::nullability::source_methods::visible_variable_type(
                expression,
                &name,
                field_types,
            )
            // This is only a source spelling carried into the import/type
            // resolver below, not proof that the expression denotes a type.
            .or(Some(name))
        }
        SyntaxKind::METHOD_CALL_EXPR => {
            crate::nullability::analysis::method_call_name_offset(expression)
                .and_then(|name_offset| call_results.get(&name_offset))
                .cloned()
        }
        SyntaxKind::NEW_EXPR => expression
            .descendants()
            .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            .and_then(|child| {
                declaration_type_name(&child).or_else(|| Some(child.text().to_string()))
            }),
        SyntaxKind::CLASS_LITERAL_EXPR => Some("Class".to_string()),
        SyntaxKind::THIS_EXPR | SyntaxKind::SUPER_EXPR => Some(current_class.to_string()),
        SyntaxKind::PAREN_EXPR | SyntaxKind::CAST_EXPR => expression
            .children()
            .find(|child| is_expression(child.kind()))
            .and_then(|child| expression_owner(&child, field_types, call_results, current_class)),
        SyntaxKind::FIELD_ACCESS_EXPR => {
            // A direct access to one of the current class's fields is a value,
            // not a progressively qualified type name. Keeping that distinction
            // here is important: call dispatch uses the returned owner to decide
            // whether a receiver such as `Client.value` names a type. Returning
            // the source path would classify `Client.value.toString()` as a
            // static call and skip the required dereference of `value`.
            if let Some(field_type) =
                direct_current_class_field_type(expression, field_types, current_class)
            {
                return Some(field_type.to_owned());
            }
            let compact: String = expression
                .text()
                .to_string()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            let root = compact.split('.').next().unwrap_or(&compact);
            expression
                .descendants()
                .find(|node| {
                    node.kind() == SyntaxKind::NAME_EXPR
                        && name_expr_ident(node).as_deref() == Some(root)
                })
                .and_then(|root_expression| {
                    crate::nullability::source_methods::visible_variable_type(
                        &root_expression,
                        root,
                        field_types,
                    )
                })
                // Preserve a qualified source spelling for resolver-backed
                // lookup without treating capitalization as semantics.
                .or_else(|| Some(compact.trim_end_matches(".class").to_string()))
        }
        _ => None,
    }
}

fn direct_current_class_field_type<'a>(
    expression: &SyntaxNode,
    field_types: &'a HashMap<String, String>,
    current_class: &str,
) -> Option<&'a str> {
    let receiver = expression.children().next()?;
    let receiver_path = crate::nullability::stable_access_path(&receiver)?;
    if receiver_path != "this" && !source_owner_matches(&receiver_path, current_class) {
        return None;
    }
    let field_name = expression
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier())
        .last()
        .map(|token| token.text().to_string())
        .or_else(|| {
            crate::nullability::stable_access_path(expression)
                .and_then(|path| path.rsplit('.').next().map(str::to_owned))
        })?;
    field_types.get(&field_name).map(String::as_str)
}

pub(super) struct JavaMethodOwnerResolver {
    imports: hyperion_workspace::ImportMap,
}

impl JavaMethodOwnerResolver {
    pub(super) fn from_root(root: &SyntaxNode) -> Self {
        Self {
            imports: crate::utils::build_imports_from_syntax(root),
        }
    }

    pub(super) fn resolve_call_owner(
        &self,
        owner: &str,
        method_name: &str,
        arity: usize,
        signatures: &crate::nullability::MethodSignatureSnapshot,
    ) -> String {
        let key_for = |class_name: String| crate::nullability::MethodCallKey {
            class_name,
            method_name: method_name.to_string(),
            arity,
        };

        if let Some((first, suffix)) = owner.split_once('.') {
            if let Some(imported) = self.imports.explicit.get(first) {
                return format!("{imported}.{suffix}");
            }
            if signatures.contains_call(&key_for(owner.to_string())) {
                return owner.to_string();
            }
        }

        if let Some(imported) = self.imports.explicit.get(owner) {
            return imported.clone();
        }

        if let Some(package) = &self.imports.current_package {
            let candidate = format!("{package}.{owner}");
            if signatures.contains_declaring_type(&candidate)
                || signatures.contains_call(&key_for(candidate.clone()))
            {
                // A type in the current package wins over every on-demand
                // import, even when that selected type has no matching member.
                return candidate;
            }
        } else if signatures.contains_declaring_type(owner)
            || signatures.contains_call(&key_for(owner.to_string()))
        {
            return owner.to_string();
        }

        let mut candidates: Vec<_> = self
            .imports
            .wildcards
            .iter()
            .map(|package| format!("{package}.{owner}"))
            .collect();
        candidates.push(format!("java.lang.{owner}"));
        candidates.retain(|candidate| {
            signatures.contains_declaring_type(candidate)
                || signatures.contains_call(&key_for(candidate.clone()))
        });
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.len() == 1 {
            return candidates.pop().expect("one checked candidate");
        }

        owner.to_string()
    }

    pub(super) fn resolve_field(
        &self,
        owner: &str,
        field_name: &str,
        access_owner: &str,
        signatures: &crate::nullability::MethodSignatureSnapshot,
    ) -> Option<(Arc<str>, crate::nullability::FieldSignature)> {
        let access_owner = self.canonical_source_owner(access_owner);
        let resolve = |candidate: &str| {
            signatures.resolve_field_signature_from(
                candidate,
                field_name,
                self.imports.current_package.as_deref(),
                &access_owner,
            )
        };
        if let Some((first, suffix)) = owner.split_once('.') {
            if let Some(imported) = self.imports.explicit.get(first) {
                return resolve(&format!("{imported}.{suffix}"));
            }
            if signatures.contains_declaring_type(owner)
                || signatures.contains_field(owner, field_name)
            {
                return resolve(owner);
            }
        }

        if let Some(imported) = self.imports.explicit.get(owner) {
            return resolve(imported);
        }

        if let Some(package) = &self.imports.current_package {
            let candidate = format!("{package}.{owner}");
            if signatures.contains_declaring_type(&candidate)
                || signatures.contains_field(&candidate, field_name)
            {
                return resolve(&candidate);
            }
        } else if signatures.contains_declaring_type(owner)
            || signatures.contains_field(owner, field_name)
        {
            return resolve(owner);
        }

        let mut candidates: Vec<_> = self
            .imports
            .wildcards
            .iter()
            .map(|package| format!("{package}.{owner}"))
            .collect();
        candidates.push(format!("java.lang.{owner}"));
        candidates.retain(|candidate| {
            signatures.contains_declaring_type(candidate)
                || signatures.contains_field(candidate, field_name)
        });
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.len() == 1 {
            let candidate = candidates.pop().expect("one checked candidate");
            return resolve(&candidate);
        }

        None
    }

    fn resolve_super_field(
        &self,
        access_owner: &str,
        field_name: &str,
        signatures: &crate::nullability::MethodSignatureSnapshot,
    ) -> Option<(Arc<str>, crate::nullability::FieldSignature)> {
        signatures.resolve_super_field_signature_from(
            &self.canonical_source_owner(access_owner),
            field_name,
            self.imports.current_package.as_deref(),
        )
    }

    fn canonical_source_owner(&self, owner: &str) -> String {
        self.imports.current_package.as_ref().map_or_else(
            || owner.to_owned(),
            |package| {
                if owner
                    .strip_prefix(package)
                    .is_some_and(|suffix| suffix.starts_with('.'))
                {
                    owner.to_owned()
                } else {
                    format!("{package}.{owner}")
                }
            },
        )
    }

    fn resolves_type_name(
        &self,
        source_name: &str,
        signatures: &crate::nullability::MethodSignatureSnapshot,
    ) -> bool {
        if let Some((first, _)) = source_name.split_once('.') {
            if self.imports.explicit.contains_key(first) {
                return true;
            }
        } else if self.imports.explicit.contains_key(source_name) {
            return true;
        }
        if signatures.contains_declaring_type(source_name) {
            return true;
        }

        self.imports
            .current_package
            .iter()
            .chain(self.imports.wildcards.iter())
            .map(|package| format!("{package}.{source_name}"))
            .chain(std::iter::once(format!("java.lang.{source_name}")))
            .any(|candidate| signatures.contains_declaring_type(&candidate))
    }
}

fn known_call_result_owner(owner: &str, method_name: &str) -> Option<String> {
    let owner = owner.rsplit('.').next().unwrap_or(owner);
    match owner {
        "Optional"
            if matches!(
                method_name,
                "of" | "ofNullable" | "empty" | "filter" | "map" | "flatMap"
            ) =>
        {
            Some("Optional".to_string())
        }
        "Optional" if method_name == "stream" => Some("Stream".to_string()),
        "Stream"
            if matches!(
                method_name,
                "of" | "ofNullable"
                    | "empty"
                    | "builder"
                    | "filter"
                    | "map"
                    | "flatMap"
                    | "peek"
                    | "distinct"
                    | "sorted"
                    | "limit"
                    | "skip"
                    | "takeWhile"
                    | "dropWhile"
                    | "parallel"
                    | "sequential"
                    | "unordered"
            ) =>
        {
            Some("Stream".to_string())
        }
        "List" if matches!(method_name, "of" | "copyOf") => Some("List".to_string()),
        "Set" if matches!(method_name, "of" | "copyOf") => Some("Set".to_string()),
        "Map" if matches!(method_name, "of" | "copyOf") => Some("Map".to_string()),
        "List" | "Set" | "Collection" if method_name == "stream" => Some("Stream".to_string()),
        "Arrays" if method_name == "asList" => Some("List".to_string()),
        "Collections" if method_name.ends_with("List") => Some("List".to_string()),
        "Collections" if method_name.ends_with("Set") || method_name == "singleton" => {
            Some("Set".to_string())
        }
        "Collections" if method_name.ends_with("Map") => Some("Map".to_string()),
        _ if method_name == "getClass" => Some("Class".to_string()),
        _ => None,
    }
}

fn infer_call_owners(
    file: FileId,
    scope: &SyntaxNode,
    field_types: &HashMap<String, String>,
    current_class: &str,
    owner_resolver: &JavaMethodOwnerResolver,
    method_signatures: &crate::nullability::MethodSignatureSnapshot,
) -> HashMap<u32, String> {
    let mut calls: Vec<SyntaxNode> = scope
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
        .collect();
    calls.sort_by_key(|call| u32::from(call.text_range().len()));

    let mut owners = HashMap::new();
    let mut results = HashMap::new();
    for call in calls {
        let (method_name, receiver, arguments) = crate::nullability::method_call_descriptor(&call);
        let Some(method_name) = method_name else {
            continue;
        };
        let owner = crate::nullability::analysis::method_call_name_offset(&call)
            .and_then(|name_offset| method_signatures.indexed_call_owner(file, name_offset))
            .map(str::to_owned)
            .or_else(|| {
                receiver.as_ref().map_or_else(
                    || Some(current_class.to_string()),
                    |receiver| expression_owner(receiver, field_types, &results, current_class),
                )
            });
        let Some(owner) = owner else {
            continue;
        };
        let owner = owner_resolver.resolve_call_owner(
            &owner,
            &method_name,
            arguments.len(),
            method_signatures,
        );
        let Some(offset) = crate::nullability::analysis::method_call_name_offset(&call) else {
            continue;
        };
        if let Some(result_owner) = known_call_result_owner(&owner, &method_name) {
            results.insert(offset, result_owner);
        }
        owners.insert(offset, owner);
    }
    owners
}

#[derive(Debug, Clone)]
pub(super) struct SourceFieldInfo {
    declared_type: String,
    value_kind: crate::nullability::patterns::ValueKind,
    nullability: crate::nullability::Nullability,
    is_static: bool,
}

fn visit_source_field_paths(
    class_name: &str,
    field_name: &str,
    mut visit: impl FnMut(usize, &str),
) {
    visit(0, field_name);
    let explicit = format!("this.{field_name}");
    visit(1, &explicit);
    let qualified = format!("{class_name}.{field_name}");
    visit(2, &qualified);
}

fn declare_source_field_paths(
    resolver: &mut crate::nullability::NullabilityResolver,
    class_name: &str,
    field_name: &str,
) {
    visit_source_field_paths(class_name, field_name, |_, path| {
        resolver.declare(path);
    });
}

fn set_source_field_state(
    env: &mut crate::nullability::NullabilityEnv,
    resolver: &crate::nullability::NullabilityResolver,
    class_name: &str,
    field_name: &str,
    state: crate::nullability::Nullability,
) {
    visit_source_field_paths(class_name, field_name, |_, path| {
        if let Some(id) = resolver.resolve_variable(path) {
            env.set(id, state);
        }
    });
}

fn source_field_is_nonnull(
    env: &crate::nullability::NullabilityEnv,
    resolver: &crate::nullability::NullabilityResolver,
    class_name: &str,
    field_name: &str,
    allow_bare: bool,
) -> bool {
    let mut is_nonnull = false;
    visit_source_field_paths(class_name, field_name, |index, path| {
        is_nonnull |= (allow_bare || index != 0)
            && resolver
                .resolve_variable(path)
                .is_some_and(|id| env.get(id) == crate::nullability::Nullability::NonNull);
    });
    is_nonnull
}

fn java_default_field_state(field: Option<&SourceFieldInfo>) -> crate::nullability::Nullability {
    if field.is_some_and(|field| {
        matches!(
            field.value_kind,
            crate::nullability::patterns::ValueKind::PrimitiveBoolean
                | crate::nullability::patterns::ValueKind::PrimitiveNumeric
                | crate::nullability::patterns::ValueKind::PrimitiveFloating
        )
    }) {
        crate::nullability::Nullability::NonNull
    } else {
        crate::nullability::Nullability::Null
    }
}

pub(super) fn resolve_source_member_uses(
    file: FileId,
    scope: &SyntaxNode,
    lexical_resolver: &crate::nullability::NullabilityResolver,
    field_types: &HashMap<String, String>,
    source_fields: &HashMap<(String, String), SourceFieldInfo>,
    source_methods: &crate::nullability::source_methods::SourceMethodTable,
    call_declarations: &HashMap<u32, crate::nullability::source_methods::SourceMethodId>,
    call_owners: &HashMap<u32, String>,
    owner_resolver: &JavaMethodOwnerResolver,
    method_signatures: &crate::nullability::MethodSignatureSnapshot,
) -> (
    HashMap<u32, crate::nullability::analysis::ResolvedMemberUse>,
    Arc<[Box<[crate::nullability::patterns::ValueKind]>]>,
) {
    use crate::nullability::analysis::{
        DispatchKind, InvocationEffect, QualifierKind, ResolvedMemberUse,
    };

    let mut uses = HashMap::new();
    let mut conversion_plans: Vec<Box<[crate::nullability::patterns::ValueKind]>> = Vec::new();
    let mut conversion_plan_ids: HashMap<Vec<crate::nullability::patterns::ValueKind>, u16> =
        HashMap::new();
    for call in scope
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
    {
        let Some(offset) = crate::nullability::analysis::method_call_name_offset(&call) else {
            continue;
        };
        let (method_name, receiver, arguments) = crate::nullability::method_call_descriptor(&call);
        // Workspace overload resolution is authoritative for an indexed
        // source revision. The source table remains the live/same-file
        // fallback when no exact forward binding was published.
        let resolved = (|| {
            Some((
                method_signatures
                    .indexed_call_owner(file, offset)?
                    .to_string(),
                method_signatures.indexed_call_is_static(file, offset)?,
                method_signatures.indexed_call_value_kind(file, offset)?,
                method_signatures.indexed_call_effect(file, offset)?,
                method_signatures.indexed_call_parameter_value_kinds(
                    file,
                    offset,
                    arguments.len(),
                )?,
            ))
        })()
        .or_else(|| {
            call_declarations.get(&offset).and_then(|declaration| {
                source_methods.binding_for_declaration(declaration).map(
                    |(owner, is_static, value_kind, invocation_effect, parameter_types)| {
                        (
                            owner.to_string(),
                            is_static,
                            value_kind,
                            invocation_effect,
                            crate::nullability::analysis::invocation_parameter_value_kinds(
                                parameter_types.iter().map(String::as_str),
                                arguments.len(),
                            ),
                        )
                    },
                )
            })
        })
        .or_else(|| {
            let owner = call_owners.get(&offset)?;
            let key = crate::nullability::MethodCallKey {
                class_name: owner.clone(),
                method_name: method_name.clone()?,
                arity: arguments.len(),
            };
            method_signatures.conservative_call_member_facts(&key).map(
                |(is_static, value_kind, effect, parameter_kinds)| {
                    (
                        owner.clone(),
                        is_static,
                        value_kind,
                        effect,
                        parameter_kinds,
                    )
                },
            )
        })
        .or_else(|| {
            let receiver = receiver.as_ref()?;
            let owner = expression_declared_type_at(receiver, field_types, source_fields)?;
            let (is_static, value_kind, invocation_effect, parameter_types) =
                source_methods.resolve_member(&call, field_types, &owner)?;
            Some((
                owner,
                is_static,
                value_kind,
                invocation_effect,
                crate::nullability::analysis::invocation_parameter_value_kinds(
                    parameter_types.iter().map(String::as_str),
                    arguments.len(),
                ),
            ))
        });
        let Some((owner, is_static, value_kind, invocation_effect, parameter_kinds)) = resolved
        else {
            continue;
        };
        let argument_plan = if parameter_kinds.iter().any(|kind| {
            matches!(
                kind,
                crate::nullability::patterns::ValueKind::PrimitiveBoolean
                    | crate::nullability::patterns::ValueKind::PrimitiveNumeric
                    | crate::nullability::patterns::ValueKind::PrimitiveFloating
            )
        }) {
            if let Some(plan) = conversion_plan_ids.get(&parameter_kinds) {
                *plan
            } else if let Ok(plan) = u16::try_from(conversion_plans.len() + 1) {
                conversion_plans.push(parameter_kinds.clone().into_boxed_slice());
                conversion_plan_ids.insert(parameter_kinds, plan);
                plan
            } else {
                // Preserve dispatch/effect facts even in a pathological method
                // with more distinct plans than the compact ID can encode.
                0
            }
        } else {
            0
        };
        uses.insert(
            offset,
            ResolvedMemberUse::new(
                if is_static {
                    DispatchKind::Static
                } else {
                    DispatchKind::Instance
                },
                source_qualifier_kind(
                    receiver.as_ref(),
                    field_types,
                    source_fields,
                    Some(&owner),
                    owner_resolver,
                    method_signatures,
                ),
                value_kind,
                invocation_effect,
                crate::nullability::Nullability::Unknown,
                argument_plan,
            ),
        );
    }

    for access in scope
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
    {
        let Some(offset) = crate::nullability::analysis::field_access_name_offset(&access) else {
            continue;
        };
        let Some(member_name) = access
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier())
            .last()
            .map(|token| token.text().to_string())
        else {
            continue;
        };
        let Some(receiver) = access.children().next() else {
            continue;
        };
        let access_owner = crate::nullability::find_enclosing_class_name(&access)
            .unwrap_or_else(|| "<anonymous>".to_owned());
        let runtime_owner = expression_declared_type_at(&receiver, field_types, source_fields);
        let is_super_access = receiver.kind() == SyntaxKind::SUPER_EXPR;
        let lexical_owner = if matches!(
            receiver.kind(),
            SyntaxKind::THIS_EXPR | SyntaxKind::SUPER_EXPR
        ) {
            // `this` and `super` are values rooted in the lexical class. Their
            // literal spellings are not type names; `super` is handled below
            // by starting lookup at the direct superclass.
            Some(access_owner.clone())
        } else {
            crate::nullability::stable_access_path(&receiver)
        };
        let owner = runtime_owner.as_deref().or(lexical_owner.as_deref());
        let Some(owner) = owner else {
            continue;
        };
        // A current-class declaration must not participate in `super.field`:
        // Java starts that lookup at the direct superclass specifically to
        // bypass shadowing members in the current class.
        let source_fact = (!is_super_access)
            .then(|| {
                let mut candidates = source_fields
                    .iter()
                    .filter(|((candidate_owner, candidate_name), _)| {
                        candidate_name == &member_name
                            && source_owner_matches(owner, candidate_owner)
                    })
                    .map(|(_, field)| (field.is_static, field.value_kind, field.nullability));
                candidates
                    .next()
                    .and_then(|(is_static, mut value_kind, mut nullability)| {
                        for (candidate_static, candidate_kind, candidate_nullability) in candidates
                        {
                            if candidate_static != is_static {
                                return None;
                            }
                            if candidate_kind != value_kind {
                                value_kind = crate::nullability::patterns::ValueKind::Unknown;
                            }
                            nullability = nullability.join(candidate_nullability);
                        }
                        Some((is_static, value_kind, nullability, owner.to_owned()))
                    })
            })
            .flatten();
        let resolved = source_fact.or_else(|| {
            let (resolved_owner, field) = if is_super_access {
                owner_resolver.resolve_super_field(
                    &access_owner,
                    &member_name,
                    method_signatures,
                )?
            } else {
                owner_resolver.resolve_field(
                    owner,
                    &member_name,
                    &access_owner,
                    method_signatures,
                )?
            };
            Some((
                field.is_static(),
                field.value_kind(),
                field.nullability(),
                resolved_owner.to_string(),
            ))
        });
        let Some((is_static, value_kind, result_nullability, selected_owner)) = resolved else {
            continue;
        };
        uses.insert(
            offset,
            ResolvedMemberUse::new(
                if is_static {
                    DispatchKind::Static
                } else {
                    DispatchKind::Instance
                },
                if runtime_owner.is_some() {
                    QualifierKind::Value
                } else {
                    source_qualifier_kind(
                        Some(&receiver),
                        field_types,
                        source_fields,
                        Some(&selected_owner),
                        owner_resolver,
                        method_signatures,
                    )
                },
                value_kind,
                // A getstatic/putstatic is an active use and may trigger class
                // initialization. Constant-variable metadata is not published
                // yet, so every known static field remains conservative.
                if is_static {
                    InvocationEffect::ClassInitialization
                } else {
                    InvocationEffect::NonThrowing
                },
                result_nullability,
                0,
            ),
        );
    }

    // A bare inherited field (`value`) is a NAME_EXPR, not a
    // FIELD_ACCESS_EXPR. Bind it through the same canonical hierarchy as
    // `this.value`; otherwise cross-file field contracts silently degrade to
    // Unknown. Lexical parameters and locals win before member lookup, and a
    // field declared in the current source remains owned by `source_fields`.
    for name_expression in scope
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::NAME_EXPR)
    {
        let Some(member_name) = name_expr_ident(&name_expression) else {
            continue;
        };
        if crate::nullability::source_methods::visible_variable_binding(
            &name_expression,
            &member_name,
        ) || lexical_resolver
            .resolve_variable_node(&member_name, &name_expression)
            .is_some_and(|binding| lexical_resolver.is_scoped_binding(binding))
        {
            continue;
        }
        let access_owner = crate::nullability::find_enclosing_class_name(&name_expression)
            .unwrap_or_else(|| "<anonymous>".to_owned());
        if source_fields
            .keys()
            .any(|(owner, name)| name == &member_name && source_owner_matches(&access_owner, owner))
        {
            continue;
        }
        let Some((_, field)) = owner_resolver.resolve_field(
            &access_owner,
            &member_name,
            &access_owner,
            method_signatures,
        ) else {
            continue;
        };
        let offset = u32::from(name_expression.text_range().start());
        uses.entry(offset).or_insert_with(|| {
            ResolvedMemberUse::new(
                if field.is_static() {
                    DispatchKind::Static
                } else {
                    DispatchKind::Instance
                },
                QualifierKind::Implicit,
                field.value_kind(),
                if field.is_static() {
                    InvocationEffect::ClassInitialization
                } else {
                    InvocationEffect::NonThrowing
                },
                field.nullability(),
                0,
            )
        });
    }
    (uses, conversion_plans.into())
}

fn source_qualifier_kind(
    receiver: Option<&SyntaxNode>,
    field_types: &HashMap<String, String>,
    source_fields: &HashMap<(String, String), SourceFieldInfo>,
    selected_owner: Option<&str>,
    owner_resolver: &JavaMethodOwnerResolver,
    method_signatures: &crate::nullability::MethodSignatureSnapshot,
) -> crate::nullability::analysis::QualifierKind {
    use crate::nullability::analysis::QualifierKind;
    let Some(receiver) = receiver else {
        return QualifierKind::Implicit;
    };
    if expression_declared_type_at(receiver, field_types, source_fields).is_some()
        || expression_has_lexically_bound_root(receiver, field_types)
    {
        return QualifierKind::Value;
    }
    let Some(path) = crate::nullability::stable_access_path(receiver) else {
        return QualifierKind::Value;
    };
    if selected_owner.is_some_and(|owner| source_owner_matches(&path, owner))
        || owner_resolver.resolves_type_name(&path, method_signatures)
    {
        QualifierKind::TypeName
    } else {
        QualifierKind::Value
    }
}

fn expression_has_lexically_bound_root(
    expression: &SyntaxNode,
    field_types: &HashMap<String, String>,
) -> bool {
    let Some(path) = crate::nullability::stable_access_path(expression) else {
        return false;
    };
    let root = path.split('.').next().unwrap_or(path.as_str());
    if field_types.contains_key(root) {
        return true;
    }
    std::iter::once(expression.clone())
        .chain(expression.descendants())
        .filter(|candidate| candidate.kind() == SyntaxKind::NAME_EXPR)
        .find(|candidate| name_expr_ident(candidate).as_deref() == Some(root))
        .is_some_and(|root_expression| {
            crate::nullability::source_methods::visible_variable_binding(&root_expression, root)
        })
}

fn expression_declared_type_at(
    expression: &SyntaxNode,
    field_types: &HashMap<String, String>,
    source_fields: &HashMap<(String, String), SourceFieldInfo>,
) -> Option<String> {
    match expression.kind() {
        SyntaxKind::NAME_EXPR => {
            let name = expression.text().to_string();
            crate::nullability::source_methods::visible_variable_type(
                expression,
                name.trim(),
                field_types,
            )
        }
        SyntaxKind::PAREN_EXPR => expression
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
            .and_then(|value| expression_declared_type_at(&value, field_types, source_fields)),
        SyntaxKind::ASSIGNMENT_EXPR => {
            crate::nullability::patterns::continuation_left_operand(expression)
                .and_then(|target| expression_declared_type_at(&target, field_types, source_fields))
        }
        SyntaxKind::FIELD_ACCESS_EXPR => {
            let receiver = expression.children().next()?;
            let owner = expression_declared_type_at(&receiver, field_types, source_fields)
                .or_else(|| crate::nullability::stable_access_path(&receiver))?;
            let member_name = expression
                .descendants_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|token| token.kind().is_identifier())
                .last()?;
            let mut candidates = source_fields.iter().filter(|((candidate_owner, name), _)| {
                name == member_name.text() && source_owner_matches(&owner, candidate_owner)
            });
            let (_, first) = candidates.next()?;
            candidates
                .all(|(_, candidate)| candidate.declared_type == first.declared_type)
                .then(|| first.declared_type.clone())
        }
        SyntaxKind::CAST_EXPR => expression
            .children()
            .find(|child| {
                matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE
                )
            })
            .map(|ty| ty.text().to_string().trim().to_string()),
        SyntaxKind::NEW_EXPR => expression
            .descendants()
            .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            .map(|ty| ty.text().to_string().trim().to_string()),
        _ => None,
    }
}

fn source_owner_matches(reference: &str, declaration: &str) -> bool {
    reference == declaration
        || reference
            .strip_suffix(declaration)
            .is_some_and(|prefix| prefix.ends_with('.'))
        || declaration
            .strip_suffix(reference)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// Collect field annotations organized by class name.
///
/// Returns a map from class name to a map of field annotations
/// (field_name -> list of annotation simple names).
fn collect_field_annotations_by_class(
    root: &SyntaxNode,
) -> std::collections::HashMap<String, std::collections::HashMap<String, Vec<String>>> {
    use crate::nullability::extraction::collect_field_annotations;
    let mut class_field_annotations = std::collections::HashMap::new();

    for class_node in root
        .descendants()
        .filter(|node| node.kind().is_type_declaration())
    {
        let class_name = crate::nullability::qualified_type_name(&class_node)
            .unwrap_or_else(|| "<anonymous>".to_string());
        let field_annotations = collect_field_annotations(&class_node);
        class_field_annotations.insert(class_name, field_annotations);
    }

    class_field_annotations
}

fn field_decl_name(node: &SyntaxNode) -> Option<String> {
    let mut found_type = false;
    for element in node.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Node(child)
                if matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                ) =>
            {
                found_type = true;
            }
            rowan::NodeOrToken::Token(token) if found_type && token.kind().is_identifier() => {
                return Some(token.text().to_string());
            }
            _ => {}
        }
    }
    None
}

fn collect_node_annotations(node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(prev) = node.prev_sibling() {
        if prev.kind() == SyntaxKind::MODIFIER_LIST {
            collect_annotation_names(&prev, &mut names);
        }
    }
    collect_annotation_names(node, &mut names);
    names
}

fn collect_annotation_names(node: &SyntaxNode, output: &mut Vec<String>) {
    if node.kind() == SyntaxKind::ANNOTATION {
        if let Some(name) = crate::nullability::annotation_name(node) {
            output.push(name);
        }
    }
    for child in node.children() {
        collect_annotation_names(&child, output);
    }
}

/// Implementation of the `file_nullability` query.
///
/// This function performs dataflow-based nullability analysis on all methods
/// in a Java file. The analysis builds a control flow graph for each method
/// and tracks nullability state through all code paths.
///
/// For non-Java files, returns an empty analysis (Kotlin has built-in nullability).
pub(super) fn file_nullability(
    db: &dyn SemanticDatabase,
    file: FileId,
) -> Arc<crate::nullability::FileNullabilityAnalysis> {
    use crate::nullability::source_methods::{
        SourceMethodDeclaration, SourceMethodId, SourceMethodTable,
    };
    use crate::nullability::{
        accepted_return_nullability, analyze, collect_local_variables, expression_nullability,
        extract_explicit_nonnull_parameters, extract_method_info, extract_parameter_nullability,
        find_enclosing_class_name, qualified_type_name, ControlFlowGraph, FileNullabilityAnalysis,
        MethodCallKey, MethodContext, Nullability, NullabilityEnv, NullabilityResolver, Terminator,
    };

    let language = db.file_language(file);

    // Only analyze Java files for now
    if !matches!(language, Language::Java) {
        tracing::debug!(
            "Skipping nullability analysis for {:?} file {:?}",
            language,
            file
        );
        return Arc::new(FileNullabilityAnalysis::empty());
    }

    // The immutable input both invalidates this query and pins it to the exact
    // cross-file signature revision visible to this Salsa snapshot.
    let method_signatures = db.method_signatures();
    let parse_tree = db.parse_file(file);

    tracing::debug!("Computing nullability for file {:?}", file);

    let root = parse_tree.syntax();
    let owner_resolver = JavaMethodOwnerResolver::from_root(&root);
    // Reuse the immutable signature revision rather than reading the mutable
    // workspace index behind a broad global epoch. This catches same-package
    // type shadowing without turning every index edit into O(workspace)
    // nullability invalidation.
    let exception_type_shadows: Arc<HashSet<String>> = Arc::new(
        owner_resolver
            .imports
            .current_package
            .iter()
            .chain(owner_resolver.imports.wildcards.iter())
            .flat_map(|package| {
                let signatures = &method_signatures;
                crate::nullability::cfg::IMPLICIT_JAVA_LANG_TYPE_NAMES
                    .iter()
                    .copied()
                    .filter(move |simple| {
                        signatures.contains_declaring_type(&format!("{package}.{simple}"))
                    })
                    .map(str::to_owned)
            })
            .collect(),
    );
    let exception_types =
        crate::nullability::ExceptionTypeRevision::from_scope_with_external_shadows(
            &root,
            &exception_type_shadows,
        );
    let method_signatures = Arc::new(method_signatures);
    let class_field_annotations = collect_field_annotations_by_class(&root);
    let mut class_declared_types: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut source_fields: HashMap<(String, String), SourceFieldInfo> = HashMap::new();
    for field in root
        .descendants()
        .filter(|candidate| candidate.kind() == SyntaxKind::FIELD_DECL)
    {
        let Some(base_type_name) = declaration_type_name(&field) else {
            continue;
        };
        let class_name =
            find_enclosing_class_name(&field).unwrap_or_else(|| "<anonymous>".to_string());
        let implicit_interface_static = field
            .ancestors()
            .skip(1)
            .find(|ancestor| ancestor.kind().is_type_declaration())
            .is_some_and(|owner| {
                matches!(
                    owner.kind(),
                    SyntaxKind::INTERFACE_DECL | SyntaxKind::ANNOTATION_DECL
                )
            });
        let is_static = implicit_interface_static
            || crate::nullability::extraction::has_attached_modifier(&field, SyntaxKind::STATIC_KW);
        let annotations = collect_node_annotations(&field);
        for declarator in hyperion_types::field_declarators(&field) {
            let mut type_name = base_type_name.clone();
            for _ in 0..declarator.extra_array_dimensions {
                type_name.push_str("[]");
            }
            let value_kind =
                crate::nullability::signature_cache::declaration_value_kind(&field, &type_name);
            source_fields.insert(
                (class_name.clone(), declarator.name.clone()),
                SourceFieldInfo {
                    declared_type: type_name.clone(),
                    value_kind,
                    nullability: if matches!(
                        value_kind,
                        crate::nullability::patterns::ValueKind::PrimitiveBoolean
                            | crate::nullability::patterns::ValueKind::PrimitiveNumeric
                            | crate::nullability::patterns::ValueKind::PrimitiveFloating
                    ) {
                        Nullability::NonNull
                    } else if crate::nullability::has_nullable_annotation(&annotations) {
                        Nullability::Nullable
                    } else if crate::nullability::has_nonnull_annotation(&annotations) {
                        Nullability::NonNull
                    } else {
                        Nullability::Unknown
                    },
                    is_static,
                },
            );
            class_declared_types
                .entry(class_name.clone())
                .or_default()
                .insert(declarator.name, type_name);
        }
    }
    let mut record_component_fields: HashMap<String, std::collections::HashSet<String>> =
        HashMap::new();
    let mut nullable_record_component_fields: HashMap<String, std::collections::HashSet<String>> =
        HashMap::new();
    for record in root
        .descendants()
        .filter(|candidate| candidate.kind() == SyntaxKind::RECORD_DECL)
    {
        let class_name = qualified_type_name(&record).unwrap_or_else(|| "<anonymous>".to_string());
        let Some(parameters) = record
            .children()
            .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
        else {
            continue;
        };
        for component in parameters
            .children()
            .filter(|child| child.kind() == SyntaxKind::PARAM)
        {
            let (Some(name), Some(type_name)) = (
                field_decl_name(&component),
                declaration_type_name(&component),
            ) else {
                continue;
            };
            class_declared_types
                .entry(class_name.clone())
                .or_default()
                .insert(name.clone(), type_name);
            if crate::nullability::has_nullable_annotation(&collect_node_annotations(&component)) {
                nullable_record_component_fields
                    .entry(class_name.clone())
                    .or_default()
                    .insert(name);
            } else {
                record_component_fields
                    .entry(class_name.clone())
                    .or_default()
                    .insert(name);
            }
        }
    }

    struct MethodEntry {
        declaration: SourceMethodDeclaration,
        class_name: String,
        node: SyntaxNode,
        info: crate::nullability::MethodNullabilityInfo,
        cfg: ControlFlowGraph,
        resolver: NullabilityResolver,
        initial_env: NullabilityEnv,
        call_owners: Arc<HashMap<u32, String>>,
        call_declarations: Arc<HashMap<u32, SourceMethodId>>,
        member_uses: Arc<HashMap<u32, crate::nullability::analysis::ResolvedMemberUse>>,
        argument_conversions: Arc<[Box<[crate::nullability::patterns::ValueKind]>]>,
        field_types: HashMap<String, String>,
        local_names: std::collections::HashSet<String>,
        param_names: std::collections::HashSet<String>,
    }

    struct ConstructorEntry {
        node: SyntaxNode,
        id: Arc<str>,
        delegates_to: Option<Arc<[Arc<str>]>>,
        has_explicit_invocation: bool,
        class_name: String,
        cfg: ControlFlowGraph,
        resolver: NullabilityResolver,
        parameter_states: Vec<(String, Nullability)>,
        explicit_nonnull_params: std::collections::HashSet<String>,
        call_owners: Arc<HashMap<u32, String>>,
        call_declarations: Arc<HashMap<u32, SourceMethodId>>,
        member_uses: Arc<HashMap<u32, crate::nullability::analysis::ResolvedMemberUse>>,
        argument_conversions: Arc<[Box<[crate::nullability::patterns::ValueKind]>]>,
    }

    struct InitializerEntry {
        class_name: String,
        is_static: bool,
        source_offset: u32,
        cfg: ControlFlowGraph,
        resolver: NullabilityResolver,
        call_owners: Arc<HashMap<u32, String>>,
        call_declarations: Arc<HashMap<u32, SourceMethodId>>,
        member_uses: Arc<HashMap<u32, crate::nullability::analysis::ResolvedMemberUse>>,
        argument_conversions: Arc<[Box<[crate::nullability::patterns::ValueKind]>]>,
        local_names: std::collections::HashSet<String>,
    }

    struct FieldEntry {
        class_name: String,
        name: String,
        is_static: bool,
        source_offset: u32,
        initializer: Option<SyntaxNode>,
        explicitly_nullable: bool,
        cfg: Option<ControlFlowGraph>,
        resolver: NullabilityResolver,
        call_owners: Arc<HashMap<u32, String>>,
        call_declarations: Arc<HashMap<u32, SourceMethodId>>,
        member_uses: Arc<HashMap<u32, crate::nullability::analysis::ResolvedMemberUse>>,
        argument_conversions: Arc<[Box<[crate::nullability::patterns::ValueKind]>]>,
    }

    let mut method_entries = Vec::new();

    for node in root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::METHOD_DECL)
    {
        if let Some(body) = node.children().find(|c| c.kind() == SyntaxKind::BLOCK_STMT) {
            let Some(method_info) = extract_method_info(&node) else {
                continue;
            };
            let class_name =
                find_enclosing_class_name(&node).unwrap_or_else(|| "<anonymous>".to_string());
            let declaration =
                SourceMethodDeclaration::from_method(&node, &class_name, &method_info);
            let cfg = ControlFlowGraph::build_with_exception_types(&body, &exception_types);
            let mut resolver = NullabilityResolver::new();
            let mut initial_env = NullabilityEnv::new();
            for (param_name, declared_nullability) in &method_info.param_nullability {
                let id = resolver.declare(param_name);
                initial_env.set(id, *declared_nullability);
            }

            let local_names: std::collections::HashSet<String> =
                collect_local_variables(&body).into_iter().collect();
            for local_name in &local_names {
                resolver.declare(local_name);
            }
            for name_expr in body
                .descendants()
                .filter(|candidate| candidate.kind() == SyntaxKind::NAME_EXPR)
            {
                let Some(name) = name_expr_ident(&name_expr) else {
                    continue;
                };
                if !name.chars().next().is_some_and(char::is_uppercase) {
                    resolver.declare(&name);
                }
            }
            for access in body.descendants().filter(|candidate| {
                matches!(
                    candidate.kind(),
                    SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::ARRAY_ACCESS_EXPR
                )
            }) {
                if let Some(path) = crate::nullability::stable_access_path(&access) {
                    resolver.declare(&path);
                }
            }
            resolver.register_scoped_body_bindings(&body);
            resolver.register_declared_value_kinds(&node);
            let param_names = method_info
                .param_nullability
                .iter()
                .map(|(name, _)| name.clone())
                .collect();
            let field_types = class_declared_types
                .get(&class_name)
                .cloned()
                .unwrap_or_default();
            resolver.register_named_declared_types(&field_types);
            let call_owners = Arc::new(infer_call_owners(
                file,
                &body,
                &field_types,
                &class_name,
                &owner_resolver,
                &method_signatures,
            ));
            method_entries.push(MethodEntry {
                declaration,
                class_name,
                node,
                info: method_info,
                cfg,
                resolver,
                initial_env,
                call_owners,
                call_declarations: Arc::new(HashMap::new()),
                member_uses: Arc::new(HashMap::new()),
                argument_conversions: Arc::from([]),
                field_types,
                local_names,
                param_names,
            });
        }
    }

    let source_methods = SourceMethodTable::new(
        method_entries.iter().map(|entry| entry.declaration.clone()),
        owner_resolver.imports.current_package.as_deref(),
    );
    for entry in &mut method_entries {
        entry.call_declarations = Arc::new(source_methods.resolve_calls(
            file,
            &entry.node,
            &entry.field_types,
            &entry.call_owners,
            &method_signatures,
        ));
        let (member_uses, argument_conversions) = resolve_source_member_uses(
            file,
            &entry.node,
            &entry.resolver,
            &entry.field_types,
            &source_fields,
            &source_methods,
            &entry.call_declarations,
            &entry.call_owners,
            &owner_resolver,
            &method_signatures,
        );
        entry.member_uses = Arc::new(member_uses);
        entry.argument_conversions = argument_conversions;
    }

    let mut field_entries = Vec::new();
    let mut field_names: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    for (class_name, components) in &record_component_fields {
        field_names.insert(class_name.clone(), components.clone());
    }
    for field in root
        .descendants()
        .filter(|candidate| candidate.kind() == SyntaxKind::FIELD_DECL)
    {
        let class_name =
            find_enclosing_class_name(&field).unwrap_or_else(|| "<anonymous>".to_string());
        field_names.entry(class_name).or_default().extend(
            hyperion_types::field_declarators(&field)
                .into_iter()
                .map(|declarator| declarator.name),
        );
    }
    for field in root
        .descendants()
        .filter(|candidate| candidate.kind() == SyntaxKind::FIELD_DECL)
    {
        let class_name =
            find_enclosing_class_name(&field).unwrap_or_else(|| "<anonymous>".to_string());
        let annotations = collect_node_annotations(&field);
        let is_static = field
            .ancestors()
            .skip(1)
            .find(|ancestor| ancestor.kind().is_type_declaration())
            .is_some_and(|owner| {
                matches!(
                    owner.kind(),
                    SyntaxKind::INTERFACE_DECL | SyntaxKind::ANNOTATION_DECL
                )
            })
            || crate::nullability::extraction::has_attached_modifier(&field, SyntaxKind::STATIC_KW);
        let declared_types = class_declared_types
            .get(&class_name)
            .cloned()
            .unwrap_or_default();
        let call_owners = Arc::new(infer_call_owners(
            file,
            &field,
            &declared_types,
            &class_name,
            &owner_resolver,
            &method_signatures,
        ));
        let call_declarations = Arc::new(source_methods.resolve_calls(
            file,
            &field,
            &declared_types,
            &call_owners,
            &method_signatures,
        ));
        let mut resolver = NullabilityResolver::new();
        if let Some(names) = field_names.get(&class_name) {
            for name in names {
                declare_source_field_paths(&mut resolver, &class_name, name);
            }
        }
        // Lambdas and switch expressions are legal field initializers. Build
        // their lexical identities before binding bare member uses so a
        // scoped value can never be replaced by an inherited field contract.
        resolver.register_scoped_body_bindings(&field);
        resolver.register_declared_value_kinds(&field);
        resolver.register_named_declared_types(&declared_types);
        let (member_uses, argument_conversions) = resolve_source_member_uses(
            file,
            &field,
            &resolver,
            &declared_types,
            &source_fields,
            &source_methods,
            &call_declarations,
            &call_owners,
            &owner_resolver,
            &method_signatures,
        );
        let member_uses = Arc::new(member_uses);
        for declarator in hyperion_types::field_declarators(&field) {
            let cfg = declarator.initializer.as_ref().map(|initializer| {
                ControlFlowGraph::build_expression_with_exception_types(
                    initializer,
                    &exception_types,
                )
            });
            field_entries.push(FieldEntry {
                call_owners: Arc::clone(&call_owners),
                call_declarations: Arc::clone(&call_declarations),
                class_name: class_name.clone(),
                name: declarator.name,
                is_static,
                source_offset: declarator.initializer.as_ref().map_or_else(
                    || u32::from(field.text_range().start()),
                    |initializer| u32::from(initializer.text_range().start()),
                ),
                initializer: declarator.initializer,
                explicitly_nullable: crate::nullability::has_nullable_annotation(&annotations),
                cfg,
                resolver: resolver.clone(),
                member_uses: Arc::clone(&member_uses),
                argument_conversions: Arc::clone(&argument_conversions),
            });
        }
    }
    let mut nullable_fields_by_class = nullable_record_component_fields;
    for field in &field_entries {
        if field.explicitly_nullable {
            nullable_fields_by_class
                .entry(field.class_name.clone())
                .or_default()
                .insert(field.name.clone());
        }
    }

    let mut constructor_entries = Vec::new();
    let mut initializer_entries = Vec::new();
    for initializer in root.descendants().filter(|candidate| {
        matches!(
            candidate.kind(),
            SyntaxKind::INSTANCE_INIT | SyntaxKind::STATIC_INIT
        )
    }) {
        let Some(body) = initializer
            .children()
            .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
        else {
            continue;
        };
        let class_name =
            find_enclosing_class_name(&initializer).unwrap_or_else(|| "<anonymous>".to_string());
        let declared_types = class_declared_types
            .get(&class_name)
            .cloned()
            .unwrap_or_default();
        let local_names: std::collections::HashSet<String> =
            collect_local_variables(&body).into_iter().collect();
        let mut resolver = NullabilityResolver::new();
        if let Some(names) = field_names.get(&class_name) {
            for name in names {
                declare_source_field_paths(&mut resolver, &class_name, name);
            }
        }
        for local in &local_names {
            resolver.declare(local);
        }
        for access in body.descendants().filter(|candidate| {
            matches!(
                candidate.kind(),
                SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::ARRAY_ACCESS_EXPR
            )
        }) {
            if let Some(path) = crate::nullability::stable_access_path(&access) {
                resolver.declare(&path);
            }
        }
        resolver.register_scoped_body_bindings(&body);
        resolver.register_declared_value_kinds(&initializer);
        resolver.register_named_declared_types(&declared_types);
        let call_owners = Arc::new(infer_call_owners(
            file,
            &body,
            &declared_types,
            &class_name,
            &owner_resolver,
            &method_signatures,
        ));
        let call_declarations = Arc::new(source_methods.resolve_calls(
            file,
            &initializer,
            &declared_types,
            &call_owners,
            &method_signatures,
        ));
        let (member_uses, argument_conversions) = resolve_source_member_uses(
            file,
            &body,
            &resolver,
            &declared_types,
            &source_fields,
            &source_methods,
            &call_declarations,
            &call_owners,
            &owner_resolver,
            &method_signatures,
        );
        let is_static = initializer.kind() == SyntaxKind::STATIC_INIT;
        let source_offset = u32::from(initializer.text_range().start());
        initializer_entries.push(InitializerEntry {
            class_name: class_name.clone(),
            is_static,
            source_offset,
            cfg: ControlFlowGraph::build_with_exception_types(&body, &exception_types),
            resolver,
            call_owners,
            call_declarations,
            member_uses: Arc::new(member_uses),
            argument_conversions,
            local_names,
        });
    }

    let constructor_nodes: Vec<_> = root
        .descendants()
        .filter(|candidate| candidate.kind() == SyntaxKind::CONSTRUCTOR_DECL)
        .collect();
    let mut constructors_by_class_and_arity: HashMap<
        (String, usize),
        Vec<(Arc<str>, Arc<[String]>)>,
    > = HashMap::new();
    for constructor in &constructor_nodes {
        let class_name =
            find_enclosing_class_name(constructor).unwrap_or_else(|| "<anonymous>".to_string());
        let parameter_types = crate::nullability::signature_cache::extract_param_types(constructor);
        let id: Arc<str> = Arc::from(format!(
            "{class_name}.<init>({})",
            parameter_types.join(",")
        ));
        constructors_by_class_and_arity
            .entry((class_name, parameter_types.len()))
            .or_default()
            .push((id, parameter_types.into()));
    }
    for constructor in constructor_nodes {
        let Some(body) = constructor
            .children()
            .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
        else {
            continue;
        };
        let class_name =
            find_enclosing_class_name(&constructor).unwrap_or_else(|| "<anonymous>".to_string());
        let parameter_states = extract_parameter_nullability(&constructor);
        let explicit_nonnull_params = extract_explicit_nonnull_parameters(&constructor);
        let parameter_types =
            crate::nullability::signature_cache::extract_param_types(&constructor);
        let id: Arc<str> = Arc::from(format!(
            "{class_name}.<init>({})",
            parameter_types.join(",")
        ));
        let field_types = class_declared_types
            .get(&class_name)
            .cloned()
            .unwrap_or_default();
        let mut declared_types = field_types.clone();
        for parameter in constructor
            .children()
            .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
            .into_iter()
            .flat_map(|list| list.descendants())
            .filter(|candidate| candidate.kind() == SyntaxKind::PARAM)
        {
            if let (Some(name), Some(type_name)) = (
                field_decl_name(&parameter),
                declaration_type_name(&parameter),
            ) {
                declared_types.insert(name, type_name);
            }
        }
        let delegates_to = body
            .children()
            .find(|child| child.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION)
            .filter(|invocation| {
                invocation
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .any(|token| token.kind() == SyntaxKind::THIS_KW)
            })
            .map(|invocation| {
                let argument_list = invocation
                    .children()
                    .find(|child| child.kind() == SyntaxKind::ARG_LIST);
                let arguments = argument_list
                    .as_ref()
                    .map_or_else(Vec::new, |arguments| {
                        hyperion_workspace::usage_extractor::signature_inference::argument_expressions(
                            arguments,
                        )
                    });
                let arity = arguments.len();
                let candidates = constructors_by_class_and_arity
                    .get(&(class_name.clone(), arity))
                    .into_iter()
                    .flatten()
                    .cloned()
                    .collect::<Vec<_>>();
                let inferred_types = argument_list.as_ref().map(|arguments| {
                    hyperion_workspace::usage_extractor::signature_inference::build_type_signature_from_arg_list_with_resolver(
                        arguments,
                        |name_expression| {
                            let name = name_expr_ident(name_expression)?;
                            crate::nullability::source_methods::visible_variable_type(
                                name_expression,
                                &name,
                                &declared_types,
                            )
                        },
                    )
                });
                let exact = inferred_types.as_deref().and_then(|descriptor| {
                    let inferred: Vec<_> = if descriptor.is_empty() {
                        Vec::new()
                    } else {
                        descriptor.split(',').collect()
                    };
                    (!inferred.iter().any(|ty| matches!(*ty, "_" | "null")))
                        .then(|| {
                            candidates
                                .iter()
                                .filter(|(_, parameters)| {
                                    parameters.len() == inferred.len()
                                        && parameters
                                            .iter()
                                            .zip(&inferred)
                                            .all(|(parameter, argument)| {
                                                parameter.as_str() == *argument
                                            })
                                })
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .filter(|matches| matches.len() == 1)
                });
                exact
                    .unwrap_or(candidates)
                    .into_iter()
                    .map(|(candidate, _)| candidate)
                    .filter(|candidate| candidate.as_ref() != id.as_ref())
                    .collect::<Vec<_>>()
                    .into()
            });
        let mut resolver = NullabilityResolver::new();
        if let Some(names) = field_names.get(&class_name) {
            for name in names {
                declare_source_field_paths(&mut resolver, &class_name, name);
            }
        }
        for (name, _) in &parameter_states {
            resolver.declare(name);
        }
        for name_expr in body
            .descendants()
            .filter(|candidate| candidate.kind() == SyntaxKind::NAME_EXPR)
        {
            if let Some(name) = name_expr_ident(&name_expr) {
                resolver.declare(&name);
            }
        }
        for access in body.descendants().filter(|candidate| {
            matches!(
                candidate.kind(),
                SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::ARRAY_ACCESS_EXPR
            )
        }) {
            if let Some(path) = crate::nullability::stable_access_path(&access) {
                resolver.declare(&path);
            }
        }
        resolver.register_scoped_body_bindings(&body);
        resolver.register_declared_value_kinds(&constructor);
        resolver.register_named_declared_types(&declared_types);
        let call_owners = Arc::new(infer_call_owners(
            file,
            &body,
            &field_types,
            &class_name,
            &owner_resolver,
            &method_signatures,
        ));
        let call_declarations = Arc::new(source_methods.resolve_calls(
            file,
            &constructor,
            &field_types,
            &call_owners,
            &method_signatures,
        ));
        let (member_uses, argument_conversions) = resolve_source_member_uses(
            file,
            &body,
            &resolver,
            &field_types,
            &source_fields,
            &source_methods,
            &call_declarations,
            &call_owners,
            &owner_resolver,
            &method_signatures,
        );
        constructor_entries.push(ConstructorEntry {
            node: constructor,
            id,
            delegates_to,
            has_explicit_invocation: body
                .children()
                .any(|child| child.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION),
            call_owners,
            call_declarations,
            member_uses: Arc::new(member_uses),
            argument_conversions,
            class_name,
            cfg: ControlFlowGraph::build_with_exception_types(&body, &exception_types),
            resolver,
            parameter_states,
            explicit_nonnull_params,
        });
    }

    // Explicit declarations seed the summary lattice. Unannotated source
    // methods begin at Bottom so a non-null body can be discovered without an
    // Unknown state absorbing every delegating return. Any unresolved Bottom
    // is converted to Unknown before user-facing analysis.
    let mut method_returns_by_declaration = source_methods.initial_returns();
    let method_params_by_declaration = Arc::new(source_methods.exact_parameter_contracts());
    let method_params = Arc::new(source_methods.conservative_parameter_contracts());
    let mut method_returns = source_methods.summarize_returns(&method_returns_by_declaration);
    let known_method_keys: std::collections::HashSet<MethodCallKey> = method_entries
        .iter()
        .map(|entry| entry.declaration.call_key.clone())
        .collect();
    let mut method_dependents: HashMap<SourceMethodId, Vec<usize>> = HashMap::new();
    for (caller_index, entry) in method_entries.iter().enumerate() {
        for call in entry
            .node
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
            .filter(|call| {
                call.ancestors()
                    .skip(1)
                    .find(|ancestor| ancestor.kind() == SyntaxKind::METHOD_DECL)
                    .is_some_and(|method| method == entry.node)
            })
        {
            let name_offset = crate::nullability::analysis::method_call_name_offset(&call);
            if let Some(declaration) =
                name_offset.and_then(|offset| entry.call_declarations.get(&offset))
            {
                method_dependents
                    .entry(Arc::clone(declaration))
                    .or_default()
                    .push(caller_index);
                continue;
            }
            let (Some(method_name), receiver, arguments) =
                crate::nullability::method_call_descriptor(&call)
            else {
                continue;
            };
            let receiver_is_this = receiver
                .as_ref()
                .is_some_and(|receiver| receiver.kind() == SyntaxKind::THIS_EXPR);
            let owner = name_offset
                .and_then(|name_offset| entry.call_owners.get(&name_offset))
                .map(|owner| owner.rsplit('.').next().unwrap_or(owner))
                .or_else(|| {
                    (receiver.is_none() || receiver_is_this).then_some(entry.class_name.as_str())
                });
            let Some(owner) = owner else {
                continue;
            };
            let dependency = MethodCallKey {
                class_name: owner.to_string(),
                method_name,
                arity: arguments.len(),
            };
            if known_method_keys.contains(&dependency) {
                for declaration in source_methods.declarations_for(&dependency) {
                    method_dependents
                        .entry(Arc::clone(&declaration.id))
                        .or_default()
                        .push(caller_index);
                }
            }
        }
    }
    let mut guaranteed_fields: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    let mut constructor_guarantees: HashMap<Arc<str>, std::collections::HashSet<String>> =
        HashMap::new();
    #[derive(Clone, Copy)]
    enum InitializationStep {
        Field(usize),
        Block(usize),
    }
    let mut initialization_programs: HashMap<(String, bool), Vec<(u32, InitializationStep)>> =
        HashMap::new();
    let mut phase_field_names: HashMap<(String, bool), std::collections::HashSet<String>> =
        HashMap::new();
    for (index, field) in field_entries.iter().enumerate() {
        phase_field_names
            .entry((field.class_name.clone(), field.is_static))
            .or_default()
            .insert(field.name.clone());
        if field.initializer.is_some() {
            initialization_programs
                .entry((field.class_name.clone(), field.is_static))
                .or_default()
                .push((field.source_offset, InitializationStep::Field(index)));
        }
    }
    for (index, initializer) in initializer_entries.iter().enumerate() {
        initialization_programs
            .entry((initializer.class_name.clone(), initializer.is_static))
            .or_default()
            .push((initializer.source_offset, InitializationStep::Block(index)));
    }
    for program in initialization_programs.values_mut() {
        program.sort_unstable_by_key(|(offset, _)| *offset);
    }
    let mut initialization_classes: Vec<_> = field_names.keys().cloned().collect();
    initialization_classes.sort_unstable();
    initialization_classes.dedup();
    let executable_count = method_entries
        .len()
        .saturating_add(constructor_entries.len())
        .saturating_add(initializer_entries.len())
        .saturating_add(field_entries.len());
    let max_iterations = executable_count.saturating_mul(2).saturating_add(4);

    for iteration in 0..max_iterations {
        let previous_returns_by_declaration = method_returns_by_declaration.clone();
        let mut shared_returns_by_declaration = Arc::new(previous_returns_by_declaration.clone());
        let previous_returns = method_returns.clone();
        let mut shared_returns = Arc::new(previous_returns.clone());
        let previous_fields = guaranteed_fields.clone();
        let previous_constructor_guarantees = constructor_guarantees.clone();

        // Java executes static and instance initialization as two independent,
        // textual programs. A later declaration must never feed an earlier
        // initializer through the outer summary fixed point.
        let mut initialized_fields: HashMap<String, std::collections::HashSet<String>> =
            HashMap::new();
        for class_name in &initialization_classes {
            let mut completed_phase_fields = std::collections::HashSet::new();
            for is_static in [true, false] {
                let Some(program) = initialization_programs.get(&(class_name.clone(), is_static))
                else {
                    continue;
                };
                let names = phase_field_names
                    .get(&(class_name.clone(), is_static))
                    .cloned()
                    .unwrap_or_default();
                let mut phase_fields = std::collections::HashSet::new();
                for (_, step) in program {
                    let visible_fields: std::collections::HashSet<String> = completed_phase_fields
                        .iter()
                        .chain(phase_fields.iter())
                        .cloned()
                        .collect();
                    match *step {
                        InitializationStep::Field(index) => {
                            let field = &field_entries[index];
                            let Some(initializer) = &field.initializer else {
                                continue;
                            };
                            let mut env = NullabilityEnv::new();
                            if let Some(all_names) = field_names.get(class_name) {
                                for name in all_names {
                                    let state = if visible_fields.contains(name) {
                                        Nullability::NonNull
                                    } else {
                                        Nullability::Unknown
                                    };
                                    set_source_field_state(
                                        &mut env,
                                        &field.resolver,
                                        class_name,
                                        name,
                                        state,
                                    );
                                }
                            }
                            let context = MethodContext {
                                source_file: Some(file),
                                current_class: field.class_name.clone(),
                                call_owners: Arc::clone(&field.call_owners),
                                call_declarations: Arc::clone(&field.call_declarations),
                                member_uses: Arc::clone(&field.member_uses),
                                argument_conversions: Arc::clone(&field.argument_conversions),
                                method_returns: Arc::clone(&shared_returns),
                                method_returns_by_declaration: Arc::clone(
                                    &shared_returns_by_declaration,
                                ),
                                method_params: Arc::clone(&method_params),
                                method_params_by_declaration: Arc::clone(
                                    &method_params_by_declaration,
                                ),
                                method_signatures: Arc::clone(&method_signatures),
                                exception_type_shadows: Arc::clone(&exception_type_shadows),
                                exception_types: exception_types.clone(),
                                constructor_assigned_fields: Arc::new(visible_fields),
                                nullable_fields: Arc::new(
                                    nullable_fields_by_class
                                        .get(class_name)
                                        .cloned()
                                        .unwrap_or_default(),
                                ),
                                field_annotations: Arc::new(
                                    class_field_annotations
                                        .get(class_name)
                                        .cloned()
                                        .unwrap_or_default(),
                                ),
                                ..MethodContext::default()
                            };
                            if !field.explicitly_nullable
                                && expression_nullability(
                                    initializer,
                                    &env,
                                    &field.resolver,
                                    &context,
                                ) == Nullability::NonNull
                            {
                                phase_fields.insert(field.name.clone());
                            } else {
                                phase_fields.remove(&field.name);
                            }
                        }
                        InitializationStep::Block(index) => {
                            let initializer = &initializer_entries[index];
                            let mut env = NullabilityEnv::new();
                            if let Some(all_names) = field_names.get(class_name) {
                                for name in all_names {
                                    let state = if visible_fields.contains(name) {
                                        Nullability::NonNull
                                    } else {
                                        Nullability::Unknown
                                    };
                                    set_source_field_state(
                                        &mut env,
                                        &initializer.resolver,
                                        class_name,
                                        name,
                                        state,
                                    );
                                }
                            }
                            let context = MethodContext {
                                source_file: Some(file),
                                current_class: initializer.class_name.clone(),
                                call_owners: Arc::clone(&initializer.call_owners),
                                call_declarations: Arc::clone(&initializer.call_declarations),
                                member_uses: Arc::clone(&initializer.member_uses),
                                argument_conversions: Arc::clone(&initializer.argument_conversions),
                                method_returns: Arc::clone(&shared_returns),
                                method_returns_by_declaration: Arc::clone(
                                    &shared_returns_by_declaration,
                                ),
                                method_params: Arc::clone(&method_params),
                                method_params_by_declaration: Arc::clone(
                                    &method_params_by_declaration,
                                ),
                                method_signatures: Arc::clone(&method_signatures),
                                exception_type_shadows: Arc::clone(&exception_type_shadows),
                                exception_types: exception_types.clone(),
                                constructor_assigned_fields: Arc::new(visible_fields),
                                nullable_fields: Arc::new(
                                    nullable_fields_by_class
                                        .get(class_name)
                                        .cloned()
                                        .unwrap_or_default(),
                                ),
                                local_names: initializer.local_names.clone(),
                                field_annotations: Arc::new(
                                    class_field_annotations
                                        .get(class_name)
                                        .cloned()
                                        .unwrap_or_default(),
                                ),
                                ..MethodContext::default()
                            };
                            let analysis =
                                analyze(&initializer.cfg, env, &initializer.resolver, &context);
                            let Some(exit_env) = analysis.exit_states.get(&initializer.cfg.exit)
                            else {
                                phase_fields.clear();
                                break;
                            };
                            for name in &names {
                                let explicitly_nullable = field_entries.iter().any(|field| {
                                    field.class_name == *class_name
                                        && field.name == *name
                                        && field.explicitly_nullable
                                });
                                let assigned = source_field_is_nonnull(
                                    exit_env,
                                    &initializer.resolver,
                                    class_name,
                                    name,
                                    !initializer.local_names.contains(name),
                                );
                                if !explicitly_nullable && assigned {
                                    phase_fields.insert(name.clone());
                                } else {
                                    phase_fields.remove(name);
                                }
                            }
                        }
                    }
                }
                completed_phase_fields.extend(phase_fields);
            }
            initialized_fields.insert(class_name.clone(), completed_phase_fields);
        }
        for (class_name, components) in &record_component_fields {
            initialized_fields
                .entry(class_name.clone())
                .or_default()
                .extend(components.iter().cloned());
        }

        let mut next_fields = initialized_fields.clone();
        let mut next_constructor_guarantees = previous_constructor_guarantees.clone();
        for (class_name, all_names) in &field_names {
            let constructors: Vec<&ConstructorEntry> = constructor_entries
                .iter()
                .filter(|constructor| constructor.class_name == *class_name)
                .collect();
            if constructors.is_empty() {
                continue;
            }
            let mut instance_names = phase_field_names
                .get(&(class_name.clone(), false))
                .cloned()
                .unwrap_or_default();
            if let Some(components) = record_component_fields.get(class_name) {
                instance_names.extend(components.iter().cloned());
            }

            let nullable: std::collections::HashSet<&str> = field_entries
                .iter()
                .filter(|field| {
                    field.class_name == *class_name && !field.is_static && field.explicitly_nullable
                })
                .map(|field| field.name.as_str())
                .collect();
            let mut definitely_assigned: std::collections::HashSet<String> = instance_names
                .iter()
                .filter(|name| !nullable.contains(name.as_str()))
                .cloned()
                .collect();
            let mut saw_normal_completion = false;

            for constructor in constructors {
                let mut this_constructor_fields: std::collections::HashSet<String> = instance_names
                    .iter()
                    .filter(|name| !nullable.contains(name.as_str()))
                    .cloned()
                    .collect();
                let mut saw_this_normal_completion = false;
                let mut post_invocation_fields: std::collections::HashSet<String> =
                    initialized_fields
                        .get(class_name)
                        .into_iter()
                        .flat_map(|fields| fields.iter())
                        .filter(|name| !instance_names.contains(*name))
                        .cloned()
                        .collect();
                if let Some(targets) = &constructor.delegates_to {
                    for name in &instance_names {
                        if !targets.is_empty()
                            && targets.iter().all(|target| {
                                previous_constructor_guarantees
                                    .get(target)
                                    .is_some_and(|fields| fields.contains(name))
                            })
                        {
                            post_invocation_fields.insert(name.clone());
                        }
                    }
                } else {
                    post_invocation_fields.extend(
                        initialized_fields
                            .get(class_name)
                            .into_iter()
                            .flat_map(|fields| fields.iter())
                            .filter(|name| instance_names.contains(*name))
                            .cloned(),
                    );
                }
                let initially_visible_fields = if constructor.has_explicit_invocation {
                    post_invocation_fields
                        .iter()
                        .filter(|name| !instance_names.contains(*name))
                        .cloned()
                        .collect()
                } else {
                    post_invocation_fields.clone()
                };
                let mut initial_env = NullabilityEnv::new();
                for name in all_names {
                    let state = if initially_visible_fields.contains(name) {
                        Nullability::NonNull
                    } else {
                        Nullability::Unknown
                    };
                    set_source_field_state(
                        &mut initial_env,
                        &constructor.resolver,
                        class_name,
                        name,
                        state,
                    );
                }
                for (name, state) in &constructor.parameter_states {
                    if let Some(id) = constructor.resolver.resolve_variable(name) {
                        initial_env.set(id, *state);
                    }
                }
                let context = MethodContext {
                    source_file: Some(file),
                    current_class: class_name.clone(),
                    call_owners: Arc::clone(&constructor.call_owners),
                    call_declarations: Arc::clone(&constructor.call_declarations),
                    member_uses: Arc::clone(&constructor.member_uses),
                    argument_conversions: Arc::clone(&constructor.argument_conversions),
                    method_returns: Arc::clone(&shared_returns),
                    method_returns_by_declaration: Arc::clone(&shared_returns_by_declaration),
                    method_params: Arc::clone(&method_params),
                    method_params_by_declaration: Arc::clone(&method_params_by_declaration),
                    method_signatures: Arc::clone(&method_signatures),
                    exception_type_shadows: Arc::clone(&exception_type_shadows),
                    exception_types: exception_types.clone(),
                    constructor_assigned_fields: Arc::new(post_invocation_fields),
                    constructor_fields_after_explicit_invocation: constructor
                        .has_explicit_invocation,
                    nullable_fields: Arc::new(
                        nullable_fields_by_class
                            .get(class_name)
                            .cloned()
                            .unwrap_or_default(),
                    ),
                    param_names: constructor
                        .parameter_states
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect(),
                    explicit_nonnull_params: constructor.explicit_nonnull_params.clone(),
                    field_annotations: Arc::new(
                        class_field_annotations
                            .get(class_name)
                            .cloned()
                            .unwrap_or_default(),
                    ),
                    ..MethodContext::default()
                };
                let analysis = analyze(
                    &constructor.cfg,
                    initial_env,
                    &constructor.resolver,
                    &context,
                );

                for (block_id, block) in &constructor.cfg.blocks {
                    let is_normal_exit = *block_id == constructor.cfg.exit
                        || matches!(block.terminator, Terminator::Return(None));
                    if !is_normal_exit {
                        continue;
                    }
                    let Some(exit_env) = analysis.exit_states.get(block_id) else {
                        continue;
                    };
                    saw_normal_completion = true;
                    saw_this_normal_completion = true;
                    let is_nonnull = |name: &String| {
                        let is_shadowed = constructor
                            .parameter_states
                            .iter()
                            .any(|(parameter, _)| parameter == name);
                        source_field_is_nonnull(
                            exit_env,
                            &constructor.resolver,
                            class_name,
                            name,
                            !is_shadowed,
                        )
                    };
                    definitely_assigned.retain(&is_nonnull);
                    this_constructor_fields.retain(is_nonnull);
                }
                if saw_this_normal_completion {
                    next_constructor_guarantees
                        .insert(Arc::clone(&constructor.id), this_constructor_fields);
                }
            }

            if saw_normal_completion {
                let mut completed_fields: std::collections::HashSet<String> = initialized_fields
                    .get(class_name)
                    .into_iter()
                    .flat_map(|fields| fields.iter())
                    .filter(|name| !instance_names.contains(*name))
                    .cloned()
                    .collect();
                completed_fields.extend(definitely_assigned);
                next_fields.insert(class_name.clone(), completed_fields);
            }
        }
        guaranteed_fields = next_fields;
        constructor_guarantees = next_constructor_guarantees;

        let mut worklist: std::collections::VecDeque<usize> = method_entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (entry.info.return_nullability == Nullability::Unknown).then_some(index)
            })
            .collect();
        let mut in_worklist: std::collections::HashSet<usize> = worklist.iter().copied().collect();

        while let Some(entry_index) = worklist.pop_front() {
            in_worklist.remove(&entry_index);
            let entry = &method_entries[entry_index];
            let context = MethodContext {
                source_file: Some(file),
                return_nullability: entry.info.return_nullability,
                return_is_primitive: entry.info.return_is_primitive,
                current_class: entry.class_name.clone(),
                call_owners: Arc::clone(&entry.call_owners),
                call_declarations: Arc::clone(&entry.call_declarations),
                member_uses: Arc::clone(&entry.member_uses),
                argument_conversions: Arc::clone(&entry.argument_conversions),
                method_returns: Arc::clone(&shared_returns),
                method_returns_by_declaration: Arc::clone(&shared_returns_by_declaration),
                method_params: Arc::clone(&method_params),
                method_params_by_declaration: Arc::clone(&method_params_by_declaration),
                method_signatures: Arc::clone(&method_signatures),
                exception_type_shadows: Arc::clone(&exception_type_shadows),
                exception_types: exception_types.clone(),
                // Summaries are reused while the object is still being
                // initialized. Inferring them from the post-construction field
                // intersection would let a getter carry later assignments
                // backward into an earlier initializer.
                constructor_assigned_fields: Arc::new(std::collections::HashSet::new()),
                constructor_fields_after_explicit_invocation: false,
                nullable_fields: Arc::new(
                    nullable_fields_by_class
                        .get(&entry.class_name)
                        .cloned()
                        .unwrap_or_default(),
                ),
                local_names: entry.local_names.clone(),
                param_names: entry.param_names.clone(),
                explicit_nonnull_params: entry.info.explicit_nonnull_params.clone(),
                field_annotations: Arc::new(
                    class_field_annotations
                        .get(&entry.class_name)
                        .cloned()
                        .unwrap_or_default(),
                ),
            };
            let analysis = analyze(
                &entry.cfg,
                entry.initial_env.clone(),
                &entry.resolver,
                &context,
            );
            let inferred = accepted_return_nullability(&analysis).unwrap_or(Nullability::Unknown);
            drop(context);
            if shared_returns_by_declaration
                .get(&entry.declaration.id)
                .copied()
                == Some(inferred)
            {
                continue;
            }
            Arc::make_mut(&mut shared_returns_by_declaration)
                .insert(Arc::clone(&entry.declaration.id), inferred);
            let summary = source_methods
                .summarize_return(&entry.declaration.call_key, &shared_returns_by_declaration);
            Arc::make_mut(&mut shared_returns).insert(entry.declaration.call_key.clone(), summary);
            if let Some(dependents) = method_dependents.get(&entry.declaration.id) {
                for dependent in dependents {
                    if in_worklist.insert(*dependent) {
                        worklist.push_back(*dependent);
                    }
                }
            }
        }
        method_returns_by_declaration = Arc::try_unwrap(shared_returns_by_declaration)
            .unwrap_or_else(|shared| (*shared).clone());
        method_returns = Arc::try_unwrap(shared_returns).unwrap_or_else(|shared| (*shared).clone());

        tracing::trace!("Nullability summaries converged through iteration {iteration}");
        if method_returns_by_declaration == previous_returns_by_declaration
            && guaranteed_fields == previous_fields
            && constructor_guarantees == previous_constructor_guarantees
        {
            break;
        }
    }

    let mut ordinary_returns_by_declaration = method_returns_by_declaration.clone();
    let mut ordinary_returns = source_methods.summarize_returns(&ordinary_returns_by_declaration);
    let mut ordinary_shared_by_declaration = Arc::new(ordinary_returns_by_declaration.clone());
    let mut ordinary_shared = Arc::new(ordinary_returns.clone());
    let mut ordinary_worklist: std::collections::VecDeque<usize> = method_entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            (entry.info.return_nullability == Nullability::Unknown).then_some(index)
        })
        .collect();
    let mut ordinary_in_worklist: std::collections::HashSet<usize> =
        ordinary_worklist.iter().copied().collect();
    while let Some(entry_index) = ordinary_worklist.pop_front() {
        ordinary_in_worklist.remove(&entry_index);
        let entry = &method_entries[entry_index];
        let context = MethodContext {
            source_file: Some(file),
            return_nullability: entry.info.return_nullability,
            return_is_primitive: entry.info.return_is_primitive,
            current_class: entry.class_name.clone(),
            call_owners: Arc::clone(&entry.call_owners),
            call_declarations: Arc::clone(&entry.call_declarations),
            member_uses: Arc::clone(&entry.member_uses),
            argument_conversions: Arc::clone(&entry.argument_conversions),
            method_returns: Arc::clone(&ordinary_shared),
            method_returns_by_declaration: Arc::clone(&ordinary_shared_by_declaration),
            method_params: Arc::clone(&method_params),
            method_params_by_declaration: Arc::clone(&method_params_by_declaration),
            method_signatures: Arc::clone(&method_signatures),
            exception_type_shadows: Arc::clone(&exception_type_shadows),
            exception_types: exception_types.clone(),
            constructor_assigned_fields: Arc::new(
                guaranteed_fields
                    .get(&entry.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
            constructor_fields_after_explicit_invocation: false,
            nullable_fields: Arc::new(
                nullable_fields_by_class
                    .get(&entry.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
            local_names: entry.local_names.clone(),
            param_names: entry.param_names.clone(),
            explicit_nonnull_params: entry.info.explicit_nonnull_params.clone(),
            field_annotations: Arc::new(
                class_field_annotations
                    .get(&entry.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
        };
        let analysis = analyze(
            &entry.cfg,
            entry.initial_env.clone(),
            &entry.resolver,
            &context,
        );
        let inferred = accepted_return_nullability(&analysis).unwrap_or(Nullability::Unknown);
        drop(context);
        if ordinary_shared_by_declaration
            .get(&entry.declaration.id)
            .copied()
            == Some(inferred)
        {
            continue;
        }
        Arc::make_mut(&mut ordinary_shared_by_declaration)
            .insert(Arc::clone(&entry.declaration.id), inferred);
        let summary = source_methods
            .summarize_return(&entry.declaration.call_key, &ordinary_shared_by_declaration);
        Arc::make_mut(&mut ordinary_shared).insert(entry.declaration.call_key.clone(), summary);
        if let Some(dependents) = method_dependents.get(&entry.declaration.id) {
            for dependent in dependents {
                if ordinary_in_worklist.insert(*dependent) {
                    ordinary_worklist.push_back(*dependent);
                }
            }
        }
    }
    ordinary_returns_by_declaration =
        Arc::try_unwrap(ordinary_shared_by_declaration).unwrap_or_else(|shared| (*shared).clone());

    for state in method_returns_by_declaration.values_mut() {
        if *state == Nullability::Bottom {
            *state = Nullability::Unknown;
        }
    }
    for state in ordinary_returns_by_declaration.values_mut() {
        if *state == Nullability::Bottom {
            *state = Nullability::Unknown;
        }
    }

    method_returns = source_methods.summarize_returns(&method_returns_by_declaration);
    ordinary_returns = source_methods.summarize_returns(&ordinary_returns_by_declaration);
    let initialization_method_returns = Arc::new(method_returns);
    let initialization_method_returns_by_declaration = Arc::new(method_returns_by_declaration);
    let method_returns = Arc::new(ordinary_returns);
    let method_returns_by_declaration = Arc::new(ordinary_returns_by_declaration);
    let mut file_analysis = FileNullabilityAnalysis::empty();
    for entry in &method_entries {
        let method_offset = u32::from(entry.node.text_range().start());
        let context = MethodContext {
            source_file: Some(file),
            return_nullability: entry.info.return_nullability,
            return_is_primitive: entry.info.return_is_primitive,
            current_class: entry.class_name.clone(),
            call_owners: Arc::clone(&entry.call_owners),
            call_declarations: Arc::clone(&entry.call_declarations),
            member_uses: Arc::clone(&entry.member_uses),
            argument_conversions: Arc::clone(&entry.argument_conversions),
            method_returns: Arc::clone(&method_returns),
            method_returns_by_declaration: Arc::clone(&method_returns_by_declaration),
            method_params: Arc::clone(&method_params),
            method_params_by_declaration: Arc::clone(&method_params_by_declaration),
            method_signatures: Arc::clone(&method_signatures),
            exception_type_shadows: Arc::clone(&exception_type_shadows),
            exception_types: exception_types.clone(),
            constructor_assigned_fields: Arc::new(
                guaranteed_fields
                    .get(&entry.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
            constructor_fields_after_explicit_invocation: false,
            nullable_fields: Arc::new(
                nullable_fields_by_class
                    .get(&entry.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
            local_names: entry.local_names.clone(),
            param_names: entry.param_names.clone(),
            explicit_nonnull_params: entry.info.explicit_nonnull_params.clone(),
            field_annotations: Arc::new(
                class_field_annotations
                    .get(&entry.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
        };
        let analysis = analyze(
            &entry.cfg,
            entry.initial_env.clone(),
            &entry.resolver,
            &context,
        );
        tracing::trace!(
            "Nullability analysis for method at {}: {} blocks, {} diagnostics",
            method_offset,
            analysis.entry_states.len(),
            analysis.diagnostics.len()
        );
        file_analysis
            .diagnostics
            .extend(analysis.diagnostics.clone());
        file_analysis.methods.insert(method_offset, analysis);
    }

    // Replay both initialization phases in source order for user-facing
    // diagnostics. Entry environments are the state at that exact program
    // point, never the post-construction intersection used by ordinary methods.
    let mut initialization_phase_exits: HashMap<(String, bool), std::collections::HashSet<String>> =
        HashMap::new();
    for class_name in &initialization_classes {
        let mut completed_phase_fields = std::collections::HashSet::new();
        for is_static in [true, false] {
            let names = phase_field_names
                .get(&(class_name.clone(), is_static))
                .cloned()
                .unwrap_or_default();
            let mut phase_fields = std::collections::HashSet::new();
            let mut phase_reachable = true;
            let Some(program) = initialization_programs.get(&(class_name.clone(), is_static))
            else {
                initialization_phase_exits.insert((class_name.clone(), is_static), phase_fields);
                continue;
            };
            for (_, step) in program {
                let visible_fields: std::collections::HashSet<String> = completed_phase_fields
                    .iter()
                    .chain(phase_fields.iter())
                    .cloned()
                    .collect();
                match *step {
                    InitializationStep::Field(index) => {
                        let field = &field_entries[index];
                        let (Some(initializer), Some(cfg)) = (&field.initializer, &field.cfg)
                        else {
                            continue;
                        };
                        let mut initial_env = NullabilityEnv::new();
                        if let Some(all_names) = field_names.get(class_name) {
                            for name in all_names {
                                let state = if visible_fields.contains(name) {
                                    Nullability::NonNull
                                } else {
                                    java_default_field_state(
                                        source_fields.get(&(class_name.clone(), name.clone())),
                                    )
                                };
                                set_source_field_state(
                                    &mut initial_env,
                                    &field.resolver,
                                    class_name,
                                    name,
                                    state,
                                );
                            }
                        }
                        let context = MethodContext {
                            source_file: Some(file),
                            current_class: field.class_name.clone(),
                            call_owners: Arc::clone(&field.call_owners),
                            call_declarations: Arc::clone(&field.call_declarations),
                            member_uses: Arc::clone(&field.member_uses),
                            argument_conversions: Arc::clone(&field.argument_conversions),
                            method_returns: Arc::clone(&initialization_method_returns),
                            method_returns_by_declaration: Arc::clone(
                                &initialization_method_returns_by_declaration,
                            ),
                            method_params: Arc::clone(&method_params),
                            method_params_by_declaration: Arc::clone(&method_params_by_declaration),
                            method_signatures: Arc::clone(&method_signatures),
                            exception_type_shadows: Arc::clone(&exception_type_shadows),
                            exception_types: exception_types.clone(),
                            constructor_assigned_fields: Arc::new(visible_fields.clone()),
                            nullable_fields: Arc::new(
                                nullable_fields_by_class
                                    .get(class_name)
                                    .cloned()
                                    .unwrap_or_default(),
                            ),
                            field_annotations: Arc::new(
                                class_field_annotations
                                    .get(class_name)
                                    .cloned()
                                    .unwrap_or_default(),
                            ),
                            ..MethodContext::default()
                        };
                        let value = expression_nullability(
                            initializer,
                            &initial_env,
                            &field.resolver,
                            &context,
                        );
                        let analysis = analyze(cfg, initial_env, &field.resolver, &context);
                        let normally_completes = analysis.exit_states.contains_key(&cfg.exit);
                        file_analysis
                            .diagnostics
                            .extend(analysis.diagnostics.clone());
                        file_analysis.methods.insert(field.source_offset, analysis);
                        if !phase_reachable {
                            continue;
                        }
                        if !normally_completes {
                            phase_reachable = false;
                            phase_fields.clear();
                            continue;
                        }
                        if !field.explicitly_nullable && value == Nullability::NonNull {
                            phase_fields.insert(field.name.clone());
                        } else {
                            phase_fields.remove(&field.name);
                        }
                    }
                    InitializationStep::Block(index) => {
                        let initializer = &initializer_entries[index];
                        let mut initial_env = NullabilityEnv::new();
                        if let Some(all_names) = field_names.get(class_name) {
                            for name in all_names {
                                let state = if visible_fields.contains(name) {
                                    Nullability::NonNull
                                } else {
                                    java_default_field_state(
                                        source_fields.get(&(class_name.clone(), name.clone())),
                                    )
                                };
                                set_source_field_state(
                                    &mut initial_env,
                                    &initializer.resolver,
                                    class_name,
                                    name,
                                    state,
                                );
                            }
                        }
                        let context = MethodContext {
                            source_file: Some(file),
                            current_class: initializer.class_name.clone(),
                            call_owners: Arc::clone(&initializer.call_owners),
                            call_declarations: Arc::clone(&initializer.call_declarations),
                            member_uses: Arc::clone(&initializer.member_uses),
                            argument_conversions: Arc::clone(&initializer.argument_conversions),
                            method_returns: Arc::clone(&initialization_method_returns),
                            method_returns_by_declaration: Arc::clone(
                                &initialization_method_returns_by_declaration,
                            ),
                            method_params: Arc::clone(&method_params),
                            method_params_by_declaration: Arc::clone(&method_params_by_declaration),
                            method_signatures: Arc::clone(&method_signatures),
                            exception_type_shadows: Arc::clone(&exception_type_shadows),
                            exception_types: exception_types.clone(),
                            constructor_assigned_fields: Arc::new(visible_fields),
                            nullable_fields: Arc::new(
                                nullable_fields_by_class
                                    .get(class_name)
                                    .cloned()
                                    .unwrap_or_default(),
                            ),
                            local_names: initializer.local_names.clone(),
                            field_annotations: Arc::new(
                                class_field_annotations
                                    .get(class_name)
                                    .cloned()
                                    .unwrap_or_default(),
                            ),
                            ..MethodContext::default()
                        };
                        let analysis = analyze(
                            &initializer.cfg,
                            initial_env,
                            &initializer.resolver,
                            &context,
                        );
                        let exit_env = analysis.exit_states.get(&initializer.cfg.exit).cloned();
                        file_analysis
                            .diagnostics
                            .extend(analysis.diagnostics.clone());
                        file_analysis
                            .methods
                            .insert(initializer.source_offset, analysis);
                        if !phase_reachable {
                            continue;
                        }
                        let Some(exit_env) = exit_env else {
                            phase_reachable = false;
                            phase_fields.clear();
                            continue;
                        };
                        for name in &names {
                            let explicitly_nullable = field_entries.iter().any(|field| {
                                field.class_name == *class_name
                                    && field.name == *name
                                    && field.explicitly_nullable
                            });
                            let assigned = source_field_is_nonnull(
                                &exit_env,
                                &initializer.resolver,
                                class_name,
                                name,
                                !initializer.local_names.contains(name),
                            );
                            if !explicitly_nullable && assigned {
                                phase_fields.insert(name.clone());
                            } else {
                                phase_fields.remove(name);
                            }
                        }
                    }
                }
            }
            completed_phase_fields.extend(phase_fields.iter().cloned());
            initialization_phase_exits.insert((class_name.clone(), is_static), phase_fields);
        }
    }

    for constructor in &constructor_entries {
        let mut initial_env = NullabilityEnv::new();
        let mut constructor_entry_fields = initialization_phase_exits
            .get(&(constructor.class_name.clone(), true))
            .cloned()
            .unwrap_or_default();
        if let Some(targets) = &constructor.delegates_to {
            if let Some((first, rest)) = targets.split_first() {
                let mut delegated = constructor_guarantees
                    .get(first)
                    .cloned()
                    .unwrap_or_default();
                for target in rest {
                    let Some(fields) = constructor_guarantees.get(target) else {
                        delegated.clear();
                        break;
                    };
                    delegated.retain(|field| fields.contains(field));
                }
                constructor_entry_fields.extend(delegated);
            }
        } else {
            constructor_entry_fields.extend(
                initialization_phase_exits
                    .get(&(constructor.class_name.clone(), false))
                    .into_iter()
                    .flat_map(|fields| fields.iter())
                    .cloned(),
            );
        }
        let instance_names = phase_field_names
            .get(&(constructor.class_name.clone(), false))
            .cloned()
            .unwrap_or_default();
        let initially_visible_fields = if constructor.has_explicit_invocation {
            constructor_entry_fields
                .iter()
                .filter(|name| !instance_names.contains(*name))
                .cloned()
                .collect()
        } else {
            constructor_entry_fields.clone()
        };
        if let Some(names) = field_names.get(&constructor.class_name) {
            for name in names {
                let state = if initially_visible_fields.contains(name) {
                    Nullability::NonNull
                } else {
                    java_default_field_state(
                        source_fields.get(&(constructor.class_name.clone(), name.clone())),
                    )
                };
                set_source_field_state(
                    &mut initial_env,
                    &constructor.resolver,
                    &constructor.class_name,
                    name,
                    state,
                );
            }
        }
        for (name, state) in &constructor.parameter_states {
            if let Some(id) = constructor.resolver.resolve_variable(name) {
                initial_env.set(id, *state);
            }
        }
        let context = MethodContext {
            source_file: Some(file),
            current_class: constructor.class_name.clone(),
            call_owners: Arc::clone(&constructor.call_owners),
            call_declarations: Arc::clone(&constructor.call_declarations),
            member_uses: Arc::clone(&constructor.member_uses),
            argument_conversions: Arc::clone(&constructor.argument_conversions),
            method_returns: Arc::clone(&initialization_method_returns),
            method_returns_by_declaration: Arc::clone(
                &initialization_method_returns_by_declaration,
            ),
            method_params: Arc::clone(&method_params),
            method_params_by_declaration: Arc::clone(&method_params_by_declaration),
            method_signatures: Arc::clone(&method_signatures),
            exception_type_shadows: Arc::clone(&exception_type_shadows),
            exception_types: exception_types.clone(),
            constructor_assigned_fields: Arc::new(constructor_entry_fields),
            constructor_fields_after_explicit_invocation: constructor.has_explicit_invocation,
            nullable_fields: Arc::new(
                nullable_fields_by_class
                    .get(&constructor.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
            param_names: constructor
                .parameter_states
                .iter()
                .map(|(name, _)| name.clone())
                .collect(),
            explicit_nonnull_params: constructor.explicit_nonnull_params.clone(),
            field_annotations: Arc::new(
                class_field_annotations
                    .get(&constructor.class_name)
                    .cloned()
                    .unwrap_or_default(),
            ),
            ..MethodContext::default()
        };
        let analysis = analyze(
            &constructor.cfg,
            initial_env,
            &constructor.resolver,
            &context,
        );
        file_analysis
            .diagnostics
            .extend(analysis.diagnostics.clone());
        file_analysis
            .methods
            .insert(u32::from(constructor.node.text_range().start()), analysis);
    }

    tracing::debug!(
        "Nullability analysis complete for {:?}: {} methods, {} diagnostics",
        file,
        file_analysis.methods.len(),
        file_analysis.diagnostics.len()
    );

    Arc::new(file_analysis)
}
