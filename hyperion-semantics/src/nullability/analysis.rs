// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Forward dataflow analysis for nullability

use super::cfg::{
    catch_binding_name, foreach_binding_name, switch_pattern_binding, BlockId, CleanupKind,
    CleanupScopeId, ControlFlowGraph, ExceptionDestination, ExceptionTypeRevision, ExprRef,
    StatementRef, Terminator,
};
use super::environment::{NullabilityEnv, VariableId};
use super::patterns::{
    analyze_condition_with_call_owners, NullCheckCondition, ValueKind, VariableResolver,
};
use super::{stable_access_path, Nullability};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_workspace::{usage_extractor::signature_inference::argument_expressions, FileId};
use rowan::NodeOrToken;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Per-method context for nullability analysis.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct MethodContext {
    /// File whose syntax offsets are being analyzed. When present, workspace
    /// call bindings can select an exact overload contract.
    pub source_file: Option<FileId>,
    /// Nullability of the method's return type.
    pub return_nullability: Nullability,
    /// True when the method returns a primitive type (boolean/int/etc).
    pub return_is_primitive: bool,
    /// Known return nullability for source methods, keyed by owner, name, and arity.
    pub method_returns: Arc<HashMap<MethodCallKey, Nullability>>,
    /// Exact source return summaries keyed by declaration identity.
    #[doc(hidden)]
    pub method_returns_by_declaration: Arc<HashMap<Arc<str>, Nullability>>,
    /// Declared source parameter contracts, keyed by owner, name, and arity.
    pub method_params: Arc<HashMap<MethodCallKey, Vec<Nullability>>>,
    /// Exact source parameter contracts keyed by declaration identity.
    #[doc(hidden)]
    pub method_params_by_declaration: Arc<HashMap<Arc<str>, Arc<[Nullability]>>>,
    /// Immutable cross-file summaries from the Salsa revision being analyzed.
    pub method_signatures: Arc<super::MethodSignatureSnapshot>,
    /// Class containing the method currently being analyzed.
    pub current_class: String,
    /// Resolved or lexically inferred owner keyed by method-name token offset.
    /// Name offsets remain unique in chained calls whose syntax nodes share a
    /// common start offset.
    pub call_owners: Arc<HashMap<u32, String>>,
    /// Exact source declaration selected for a call, when overload resolution
    /// had enough type information to choose without guessing.
    #[doc(hidden)]
    pub call_declarations: Arc<HashMap<u32, Arc<str>>>,
    /// Exact runtime selection semantics keyed by the final member-name token.
    /// Contracts and dispatch are separate: fields and dependency members may
    /// have dispatch metadata even when no nullability signature exists.
    #[doc(hidden)]
    pub member_uses: Arc<HashMap<u32, ResolvedMemberUse>>,
    /// Deduplicated primitive-conversion plans referenced by member uses.
    /// Plan zero means no argument conversion; nonzero values are one-based
    /// indices into this arena.
    #[doc(hidden)]
    pub(crate) argument_conversions: Arc<[Box<[ValueKind]>]>,
    /// Same-package workspace types which shadow implicitly imported
    /// `java.lang` names for this file.
    #[doc(hidden)]
    pub exception_type_shadows: Arc<HashSet<String>>,
    /// Frozen source exception identities shared by the method and every
    /// nested lambda/switch CFG in this file revision.
    #[doc(hidden)]
    pub exception_types: ExceptionTypeRevision,
    /// Fields assigned in constructors, treated as non-null in other methods.
    pub constructor_assigned_fields: Arc<HashSet<String>>,
    /// Constructor-only staging flag. For an explicit `this(...)` or
    /// `super(...)`, the arguments are evaluated before instance initializers
    /// or delegated-constructor guarantees become visible.
    #[doc(hidden)]
    pub constructor_fields_after_explicit_invocation: bool,
    /// Fields whose declarations explicitly permit null.
    pub nullable_fields: Arc<HashSet<String>>,
    /// Local variable names in the current method.
    pub local_names: HashSet<String>,
    /// Parameter names in the current method.
    pub param_names: HashSet<String>,
    /// Parameters with a declaration-level non-null contract. Unannotated
    /// parameters may use a quiet strict body assumption, but do not make a
    /// defensive null check redundant.
    pub explicit_nonnull_params: HashSet<String>,
    /// Field annotations (field_name -> annotation simple names).
    ///
    /// Used to detect dependency injection (`@Autowired`, `@Inject`)
    /// and Lombok annotations (`@NonNull`) on fields.
    pub field_annotations: Arc<HashMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum DispatchKind {
    Static,
    Instance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum QualifierKind {
    Implicit,
    TypeName,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub(crate) enum InvocationEffect {
    NonThrowing,
    ClassInitialization,
    Arbitrary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct ResolvedMemberUse {
    pub value_kind: ValueKind,
    flags: u8,
    pub(crate) argument_plan: u16,
}

impl ResolvedMemberUse {
    const DISPATCH_STATIC: u8 = 1 << 0;
    const QUALIFIER_SHIFT: u8 = 1;
    const QUALIFIER_MASK: u8 = 0b11 << Self::QUALIFIER_SHIFT;
    const EFFECT_SHIFT: u8 = 3;
    const EFFECT_MASK: u8 = 0b11 << Self::EFFECT_SHIFT;
    const NULLABILITY_SHIFT: u8 = 5;
    const NULLABILITY_MASK: u8 = 0b111 << Self::NULLABILITY_SHIFT;

    pub(crate) const fn new(
        dispatch: DispatchKind,
        qualifier: QualifierKind,
        value_kind: ValueKind,
        effect: InvocationEffect,
        result_nullability: Nullability,
        argument_plan: u16,
    ) -> Self {
        let dispatch = match dispatch {
            DispatchKind::Static => Self::DISPATCH_STATIC,
            DispatchKind::Instance => 0,
        };
        let qualifier = match qualifier {
            QualifierKind::Implicit => 0,
            QualifierKind::TypeName => 1,
            QualifierKind::Value => 2,
        } << Self::QUALIFIER_SHIFT;
        let effect = match effect {
            InvocationEffect::NonThrowing => 0,
            InvocationEffect::ClassInitialization => 1,
            InvocationEffect::Arbitrary => 2,
        } << Self::EFFECT_SHIFT;
        let nullability = match result_nullability {
            Nullability::NonNull => 0,
            Nullability::Null => 1,
            Nullability::Nullable => 2,
            Nullability::Unknown => 3,
            Nullability::Bottom => 4,
        } << Self::NULLABILITY_SHIFT;
        Self {
            value_kind,
            flags: dispatch | qualifier | effect | nullability,
            argument_plan,
        }
    }

    pub(crate) const fn dispatch(self) -> DispatchKind {
        if self.flags & Self::DISPATCH_STATIC != 0 {
            DispatchKind::Static
        } else {
            DispatchKind::Instance
        }
    }

    pub(crate) const fn qualifier(self) -> QualifierKind {
        match (self.flags & Self::QUALIFIER_MASK) >> Self::QUALIFIER_SHIFT {
            1 => QualifierKind::TypeName,
            2 => QualifierKind::Value,
            _ => QualifierKind::Implicit,
        }
    }

    pub(crate) const fn effect(self) -> InvocationEffect {
        match (self.flags & Self::EFFECT_MASK) >> Self::EFFECT_SHIFT {
            1 => InvocationEffect::ClassInitialization,
            2 => InvocationEffect::Arbitrary,
            _ => InvocationEffect::NonThrowing,
        }
    }

    pub(crate) const fn result_nullability(self) -> Nullability {
        match (self.flags & Self::NULLABILITY_MASK) >> Self::NULLABILITY_SHIFT {
            0 => Nullability::NonNull,
            1 => Nullability::Null,
            2 => Nullability::Nullable,
            4 => Nullability::Bottom,
            _ => Nullability::Unknown,
        }
    }
}

/// Key for method call-site inference (class + name + arity).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MethodCallKey {
    pub class_name: String,
    pub method_name: String,
    pub arity: usize,
}

/// Resolver that assigns stable VariableIds for a method.
#[derive(Debug, Clone, Default)]
pub struct NullabilityResolver {
    name_to_id: HashMap<String, VariableId>,
    id_to_name: HashMap<VariableId, String>,
    metadata: Option<Box<ResolverMetadata>>,
    next_id: u32,
}

#[derive(Debug, Clone, Default)]
struct ResolverMetadata {
    syntax_to_id: HashMap<SyntaxNode, VariableId>,
    declarator_to_id: HashMap<(SyntaxNode, u32), VariableId>,
    root_by_id: HashMap<VariableId, VariableId>,
    value_kinds: HashMap<VariableId, ValueKind>,
    declared_types: HashMap<VariableId, String>,
}

impl NullabilityResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn declare(&mut self, name: &str) -> VariableId {
        if let Some(id) = self.name_to_id.get(name) {
            return *id;
        }
        let id = self.fresh_id(name);
        self.name_to_id.insert(name.to_string(), id);
        id
    }

    /// Register compound-statement variables with identities distinct from
    /// same-named fields and bindings in sibling scopes. References are mapped
    /// by syntax identity, so a binding disappears naturally at the end of its
    /// lexical body instead of leaking a name-based environment entry through
    /// a CFG merge. Syntax identity also distinguishes nested paths that share
    /// a start offset, such as `service` and `service.child`.
    pub fn register_scoped_body_bindings(&mut self, root: &SyntaxNode) {
        for declaration in root.descendants().filter(|node| {
            node.kind() == SyntaxKind::FOREACH_STMT
                || node.kind() == SyntaxKind::CATCH_CLAUSE
                || (node.kind() == SyntaxKind::TYPE_PATTERN
                    && node
                        .ancestors()
                        .skip(1)
                        .find(|ancestor| {
                            matches!(
                                ancestor.kind(),
                                SyntaxKind::CASE_CLAUSE | SyntaxKind::PATTERN_INSTANCEOF_EXPR
                            )
                        })
                        .is_some_and(|ancestor| ancestor.kind() == SyntaxKind::CASE_CLAUSE))
        }) {
            if declaration
                .ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor != root)
                .any(|ancestor| ancestor.kind().is_type_declaration())
            {
                continue;
            }
            let Some((name, body)) = scoped_body_binding(&declaration) else {
                continue;
            };

            let binding_id = self.fresh_id(&name);
            if declaration.kind() == SyntaxKind::CATCH_CLAUSE {
                let declared_types: Vec<_> = declaration
                    .descendants()
                    .take_while(|node| node.kind() != SyntaxKind::BLOCK_STMT)
                    .filter(|node| node.kind() == SyntaxKind::CLASS_TYPE)
                    .map(|node| node.text().to_string().trim().to_string())
                    .collect();
                if let [declared_type] = declared_types.as_slice() {
                    self.metadata
                        .get_or_insert_with(|| Box::new(ResolverMetadata::default()))
                        .declared_types
                        .insert(binding_id, declared_type.clone());
                }
            }
            self.register_scoped_identity(declaration.clone(), binding_id, binding_id);

            let mut paths = HashMap::new();
            for reference in body.descendants().filter(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::NAME_EXPR
                        | SyntaxKind::FIELD_ACCESS_EXPR
                        | SyntaxKind::ARRAY_ACCESS_EXPR
                        | SyntaxKind::PAREN_EXPR
                        | SyntaxKind::CAST_EXPR
                )
            }) {
                if reference
                    .ancestors()
                    .skip(1)
                    .take_while(|ancestor| ancestor != &body)
                    .any(|ancestor| ancestor.kind().is_type_declaration())
                {
                    continue;
                }
                let Some(path) = stable_access_path(&reference) else {
                    continue;
                };
                let id = if path == name {
                    binding_id
                } else if path
                    .strip_prefix(&name)
                    .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
                {
                    let id = *paths
                        .entry(path.clone())
                        .or_insert_with(|| self.fresh_id(&path));
                    id
                } else {
                    continue;
                };
                self.register_scoped_identity(reference, id, binding_id);
            }
        }

        // Lambda parameters are lexical bindings too. Register each PARAM
        // independently so a parameter never aliases a same-named enclosing
        // local or field when a synchronously invoked lambda is analyzed.
        for lambda in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::LAMBDA_EXPR)
        {
            if lambda
                .ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor != root)
                .any(|ancestor| ancestor.kind().is_type_declaration())
            {
                continue;
            }
            let Some(body) = lambda.children().last() else {
                continue;
            };
            for parameter in lambda
                .children()
                .filter(|child| child.kind() == SyntaxKind::PARAM)
            {
                let Some(name) = parameter
                    .children_with_tokens()
                    .filter_map(NodeOrToken::into_token)
                    .filter(|token| token.kind().is_identifier())
                    .last()
                    .map(|token| token.text().to_string())
                else {
                    continue;
                };
                let binding_id = self.fresh_id(&name);
                self.register_scoped_identity(parameter.clone(), binding_id, binding_id);
                if let Some(declared_type) = declared_type_name(&parameter) {
                    let metadata = self
                        .metadata
                        .get_or_insert_with(|| Box::new(ResolverMetadata::default()));
                    metadata
                        .declared_types
                        .insert(binding_id, declared_type.clone());
                    let kind = value_kind_for_type_name(&declared_type);
                    if kind != ValueKind::Unknown {
                        metadata.value_kinds.insert(binding_id, kind);
                    }
                }

                let mut paths = HashMap::new();
                for reference in body.descendants().filter(|node| {
                    matches!(
                        node.kind(),
                        SyntaxKind::NAME_EXPR
                            | SyntaxKind::FIELD_ACCESS_EXPR
                            | SyntaxKind::ARRAY_ACCESS_EXPR
                            | SyntaxKind::PAREN_EXPR
                            | SyntaxKind::CAST_EXPR
                    )
                }) {
                    let Some(path) = stable_access_path(&reference) else {
                        continue;
                    };
                    let id = if path == name {
                        binding_id
                    } else if path
                        .strip_prefix(&name)
                        .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
                    {
                        *paths
                            .entry(path.clone())
                            .or_insert_with(|| self.fresh_id(&path))
                    } else {
                        continue;
                    };
                    self.register_scoped_identity(reference, id, binding_id);
                }
            }
        }

        // `instanceof` pattern variables have boolean flow scope rather than
        // an ordinary enclosing-block scope. Register one fresh identity and
        // map only uses whose evaluation or control-flow path proves that the
        // pattern matched. This keeps a same-named field visible everywhere
        // else without leaking the binding through joins.
        for pattern in root.descendants().filter(|node| {
            node.kind() == SyntaxKind::TYPE_PATTERN
                && node
                    .ancestors()
                    .skip(1)
                    .find(|ancestor| {
                        matches!(
                            ancestor.kind(),
                            SyntaxKind::CASE_CLAUSE | SyntaxKind::PATTERN_INSTANCEOF_EXPR
                        )
                    })
                    .is_some_and(|ancestor| ancestor.kind() == SyntaxKind::PATTERN_INSTANCEOF_EXPR)
        }) {
            if pattern
                .ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor != root)
                .any(|ancestor| ancestor.kind().is_type_declaration())
            {
                continue;
            }
            let Some(name) = type_pattern_binding_name(&pattern) else {
                continue;
            };
            let binding_id = self.fresh_id(&name);
            self.register_scoped_identity(pattern.clone(), binding_id, binding_id);

            let mut paths = HashMap::new();
            for reference in root.descendants().filter(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::NAME_EXPR
                        | SyntaxKind::FIELD_ACCESS_EXPR
                        | SyntaxKind::ARRAY_ACCESS_EXPR
                        | SyntaxKind::PAREN_EXPR
                        | SyntaxKind::CAST_EXPR
                ) && pattern_binding_is_in_scope(&pattern, node)
            }) {
                let Some(path) = stable_access_path(&reference) else {
                    continue;
                };
                let id = if path == name {
                    binding_id
                } else if path
                    .strip_prefix(&name)
                    .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
                {
                    *paths
                        .entry(path.clone())
                        .or_insert_with(|| self.fresh_id(&path))
                } else {
                    continue;
                };
                self.register_scoped_identity(reference, id, binding_id);
            }
        }

        // Ordinary locals need the same syntax identity as compound-statement
        // bindings. A name-only ID aliases `Object value` in an arm block with
        // a same-named field, allowing the local's state to escape its lexical
        // scope and suppress a later field diagnostic. Register each local and
        // every access path in the remainder of its enclosing scope.
        for declaration in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::LOCAL_VAR_DECL)
        {
            if declaration
                .ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor != root)
                .any(|ancestor| ancestor.kind().is_type_declaration())
            {
                continue;
            }
            let Some(scope) = lexical_local_scope(&declaration, root) else {
                continue;
            };

            for declarator in local_declarators(&declaration) {
                let binding_id = self.fresh_id(&declarator.name);
                self.register_declarator_identity(
                    declaration.clone(),
                    declarator.name_start,
                    binding_id,
                );

                let mut paths = HashMap::new();
                for reference in scope.descendants().filter(|node| {
                    u32::from(node.text_range().start()) >= declarator.name_end
                        && local_reference_is_in_scope(&declaration, node)
                        && matches!(
                            node.kind(),
                            SyntaxKind::NAME_EXPR
                                | SyntaxKind::FIELD_ACCESS_EXPR
                                | SyntaxKind::ARRAY_ACCESS_EXPR
                                | SyntaxKind::PAREN_EXPR
                                | SyntaxKind::CAST_EXPR
                        )
                }) {
                    if reference
                        .ancestors()
                        .skip(1)
                        .take_while(|ancestor| ancestor != &scope)
                        .any(|ancestor| ancestor.kind().is_type_declaration())
                    {
                        continue;
                    }
                    let Some(path) = stable_access_path(&reference) else {
                        continue;
                    };
                    let id = if path == declarator.name {
                        binding_id
                    } else if path
                        .strip_prefix(&declarator.name)
                        .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('['))
                    {
                        *paths
                            .entry(path.clone())
                            .or_insert_with(|| self.fresh_id(&path))
                    } else {
                        continue;
                    };
                    self.register_scoped_identity(reference, id, binding_id);
                }
            }
        }
    }

    fn register_scoped_identity(&mut self, syntax: SyntaxNode, id: VariableId, root: VariableId) {
        let metadata = self
            .metadata
            .get_or_insert_with(|| Box::new(ResolverMetadata::default()));
        metadata.syntax_to_id.insert(syntax, id);
        metadata.root_by_id.insert(id, root);
    }

    fn register_declarator_identity(
        &mut self,
        declaration: SyntaxNode,
        name_offset: u32,
        id: VariableId,
    ) {
        let metadata = self
            .metadata
            .get_or_insert_with(|| Box::new(ResolverMetadata::default()));
        metadata
            .declarator_to_id
            .insert((declaration, name_offset), id);
        metadata.root_by_id.insert(id, id);
    }

    fn scoped_root(&self, id: VariableId) -> Option<VariableId> {
        self.metadata
            .as_deref()
            .and_then(|metadata| metadata.root_by_id.get(&id).copied())
    }

    fn fresh_id(&mut self, name: &str) -> VariableId {
        let id = VariableId(self.next_id);
        self.next_id += 1;
        self.id_to_name.insert(id, name.to_string());
        id
    }

    pub fn name_for(&self, id: VariableId) -> Option<&str> {
        self.id_to_name.get(&id).map(String::as_str)
    }

    pub(crate) fn register_declared_value_kinds(&mut self, root: &SyntaxNode) {
        for declaration in root.descendants().filter(|node| {
            matches!(node.kind(), SyntaxKind::PARAM | SyntaxKind::LOCAL_VAR_DECL)
                && !node
                    .ancestors()
                    .skip(1)
                    .take_while(|ancestor| ancestor != root)
                    .any(|ancestor| ancestor.kind().is_type_declaration())
        }) {
            let kind = declared_value_kind(&declaration);
            let declared_type = declared_type_name(&declaration);
            if declaration.kind() == SyntaxKind::LOCAL_VAR_DECL {
                for declarator in local_declarators(&declaration) {
                    if let Some(variable) = self.resolve_declarator(
                        &declarator.name,
                        &declaration,
                        declarator.name_start,
                    ) {
                        let kind = if declarator.array_suffix {
                            ValueKind::Reference
                        } else {
                            kind
                        };
                        let metadata = self
                            .metadata
                            .get_or_insert_with(|| Box::new(ResolverMetadata::default()));
                        if kind != ValueKind::Unknown {
                            metadata.value_kinds.insert(variable, kind);
                        }
                        if let Some(declared_type) = &declared_type {
                            metadata
                                .declared_types
                                .insert(variable, declared_type.clone());
                        }
                    }
                }
            } else if let Some(name) = declaration
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .filter(|token| token.kind().is_identifier())
                .last()
                .map(|token| token.text().to_string())
            {
                if let Some(variable) = self.resolve_variable_node(&name, &declaration) {
                    let metadata = self
                        .metadata
                        .get_or_insert_with(|| Box::new(ResolverMetadata::default()));
                    if kind != ValueKind::Unknown {
                        metadata.value_kinds.insert(variable, kind);
                    }
                    if let Some(declared_type) = &declared_type {
                        metadata
                            .declared_types
                            .insert(variable, declared_type.clone());
                    }
                }
            }
        }
    }

    pub(crate) fn register_named_declared_types(
        &mut self,
        declared_types: &HashMap<String, String>,
    ) {
        for (name, declared_type) in declared_types {
            let kind = value_kind_for_type_name(declared_type);
            for candidate in [name.clone(), format!("this.{name}")] {
                let Some(variable) = self.name_to_id.get(&candidate).copied() else {
                    continue;
                };
                let metadata = self
                    .metadata
                    .get_or_insert_with(|| Box::new(ResolverMetadata::default()));
                metadata
                    .declared_types
                    .insert(variable, declared_type.clone());
                if kind != ValueKind::Unknown {
                    metadata.value_kinds.insert(variable, kind);
                }
            }
        }
    }
}

fn declared_type_name(declaration: &SyntaxNode) -> Option<String> {
    declared_type_node(declaration).map(|node| node.text().to_string().trim().to_string())
}

fn declared_type_node(declaration: &SyntaxNode) -> Option<SyntaxNode> {
    declaration.children().find(|node| {
        matches!(
            node.kind(),
            SyntaxKind::ARRAY_TYPE
                | SyntaxKind::CLASS_TYPE
                | SyntaxKind::PRIMITIVE_TYPE
                | SyntaxKind::VAR_TYPE
        )
    })
}

fn declared_value_kind(declaration: &SyntaxNode) -> ValueKind {
    let Some(declared_type) = declared_type_node(declaration) else {
        return ValueKind::Unknown;
    };
    if declared_type.kind() == SyntaxKind::VAR_TYPE {
        return find_initializer_expr(declaration).map_or(ValueKind::Unknown, |initializer| {
            match initializer.kind() {
                SyntaxKind::LITERAL_EXPR => {
                    let text = initializer.text().to_string();
                    let text = text.trim();
                    if matches!(text, "true" | "false") {
                        ValueKind::PrimitiveBoolean
                    } else if text.starts_with('"') {
                        ValueKind::StringReference
                    } else if text == "null" {
                        ValueKind::Reference
                    } else if numeric_literal_is_floating(text) {
                        ValueKind::PrimitiveFloating
                    } else {
                        ValueKind::PrimitiveNumeric
                    }
                }
                SyntaxKind::NEW_EXPR | SyntaxKind::ARRAY_INIT_EXPR => ValueKind::Reference,
                SyntaxKind::CAST_EXPR => declared_value_kind(&initializer),
                _ => ValueKind::Unknown,
            }
        });
    }
    if declared_type.kind() == SyntaxKind::ARRAY_TYPE {
        return ValueKind::Reference;
    }
    if !matches!(
        declared_type.kind(),
        SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::CLASS_TYPE
    ) {
        return ValueKind::Unknown;
    }
    value_kind_for_type_name(declared_type.text().to_string().trim())
}

pub(crate) fn value_kind_for_type_name(type_name: &str) -> ValueKind {
    let type_name = type_name.trim();
    if type_name.ends_with("[]") || type_name.ends_with("...") {
        return ValueKind::Reference;
    }
    match type_name.rsplit('.').next().unwrap_or_default() {
        "boolean" => ValueKind::PrimitiveBoolean,
        "byte" | "short" | "int" | "long" | "char" => ValueKind::PrimitiveNumeric,
        "float" | "double" => ValueKind::PrimitiveFloating,
        "Boolean" => ValueKind::BoxedBoolean,
        "Float" | "Double" => ValueKind::BoxedFloating,
        "Byte" | "Short" | "Integer" | "Long" | "Character" => ValueKind::BoxedNumeric,
        "String" => ValueKind::StringReference,
        "void" => ValueKind::Unknown,
        _ => ValueKind::Reference,
    }
}

pub(crate) fn invocation_parameter_value_kinds<'a>(
    parameter_types: impl IntoIterator<Item = &'a str>,
    invocation_arity: usize,
) -> Vec<ValueKind> {
    let mut parameter_types: Vec<_> = parameter_types.into_iter().collect();
    let vararg_element = parameter_types
        .last()
        .copied()
        .and_then(|parameter| parameter.strip_suffix("..."));
    if vararg_element.is_some() {
        parameter_types.pop();
    }
    let mut kinds: Vec<_> = parameter_types
        .into_iter()
        .map(value_kind_for_type_name)
        .collect();
    if let Some(vararg_element) = vararg_element {
        kinds.resize(
            invocation_arity.max(kinds.len()),
            value_kind_for_type_name(vararg_element),
        );
    }
    kinds
}

fn lexical_local_scope(declaration: &SyntaxNode, root: &SyntaxNode) -> Option<SyntaxNode> {
    declaration.ancestors().skip(1).find(|ancestor| {
        ancestor == root
            || matches!(
                ancestor.kind(),
                SyntaxKind::BLOCK_STMT
                    | SyntaxKind::CASE_CLAUSE
                    | SyntaxKind::FOR_STMT
                    | SyntaxKind::FOREACH_STMT
                    | SyntaxKind::TRY_STMT
                    | SyntaxKind::TRY_WITH_RESOURCES_STMT
            )
    })
}

fn local_reference_is_in_scope(declaration: &SyntaxNode, reference: &SyntaxNode) -> bool {
    let Some(resource_spec) = declaration
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.kind() == SyntaxKind::RESOURCE_SPEC)
    else {
        return true;
    };

    if is_node_within(reference, &resource_spec) {
        // A resource variable is not in scope in its own initializer, only in
        // the remainder of the resource specification.
        return reference.text_range().start() >= declaration.text_range().end();
    }

    let Some(try_statement) = resource_spec.parent() else {
        return false;
    };
    try_statement
        .children()
        .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
        .is_some_and(|body| is_node_within(reference, &body))
}

impl VariableResolver for NullabilityResolver {
    fn resolve_variable(&self, name: &str) -> Option<VariableId> {
        self.name_to_id.get(name).copied()
    }

    fn resolve_variable_node(&self, name: &str, node: &SyntaxNode) -> Option<VariableId> {
        self.metadata
            .as_deref()
            .and_then(|metadata| metadata.syntax_to_id.get(node).copied())
            .or_else(|| self.resolve_variable(name))
    }

    fn resolve_declarator(
        &self,
        name: &str,
        declaration: &SyntaxNode,
        name_offset: u32,
    ) -> Option<VariableId> {
        self.metadata
            .as_deref()
            .and_then(|metadata| {
                metadata
                    .declarator_to_id
                    .get(&(declaration.clone(), name_offset))
                    .copied()
            })
            .or_else(|| self.resolve_variable(name))
    }

    fn is_scoped_binding(&self, variable: VariableId) -> bool {
        self.scoped_root(variable).is_some()
    }

    fn variable_name(&self, variable: VariableId) -> Option<&str> {
        self.name_for(variable)
    }

    fn value_kind(&self, variable: VariableId) -> ValueKind {
        self.metadata
            .as_deref()
            .and_then(|metadata| metadata.value_kinds.get(&variable).copied())
            .unwrap_or_default()
    }

    fn declared_type_name(&self, variable: VariableId) -> Option<&str> {
        self.metadata
            .as_deref()
            .and_then(|metadata| metadata.declared_types.get(&variable))
            .map(String::as_str)
    }

    fn dependent_variables(&self, prefix: &str) -> Vec<VariableId> {
        let dot_prefix = format!("{prefix}.");
        let index_prefix = format!("{prefix}[");
        self.id_to_name
            .iter()
            .filter_map(|(id, name)| {
                (self.scoped_root(*id).is_none()
                    && (name.starts_with(&dot_prefix) || name.starts_with(&index_prefix)))
                .then_some(*id)
            })
            .collect()
    }

    fn dependent_variables_for(&self, variable: VariableId) -> Vec<VariableId> {
        let Some(prefix) = self.id_to_name.get(&variable) else {
            return Vec::new();
        };
        let dot_prefix = format!("{prefix}.");
        let index_prefix = format!("{prefix}[");
        let scoped_root = self.scoped_root(variable);

        self.id_to_name
            .iter()
            .filter_map(|(id, name)| {
                let same_identity_domain = match scoped_root {
                    Some(root) => self.scoped_root(*id) == Some(root),
                    None => self.scoped_root(*id).is_none(),
                };
                (same_identity_domain
                    && (name.starts_with(&dot_prefix) || name.starts_with(&index_prefix)))
                .then_some(*id)
            })
            .collect()
    }
}

fn scoped_body_binding(declaration: &SyntaxNode) -> Option<(String, SyntaxNode)> {
    match declaration.kind() {
        SyntaxKind::FOREACH_STMT => {
            let body = declaration
                .children()
                .filter(|child| !is_expression(child.kind()))
                .last()?;
            Some((foreach_binding_name(declaration)?, body))
        }
        SyntaxKind::TYPE_PATTERN => {
            let case = declaration
                .ancestors()
                .skip(1)
                .find(|ancestor| ancestor.kind() == SyntaxKind::CASE_CLAUSE)?;
            Some((switch_pattern_binding(declaration)?.0, case))
        }
        SyntaxKind::CATCH_CLAUSE => {
            let body = declaration
                .children()
                .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)?;
            Some((catch_binding_name(declaration)?, body))
        }
        _ => None,
    }
}

fn type_pattern_binding_name(pattern: &SyntaxNode) -> Option<String> {
    pattern
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier())
        .last()
        .map(|token| token.text().to_string())
}

fn pattern_binding_is_in_scope(pattern: &SyntaxNode, reference: &SyntaxNode) -> bool {
    if reference.text_range().start() <= pattern.text_range().start() {
        return false;
    }
    let Some(pattern_expression) = pattern
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.kind() == SyntaxKind::PATTERN_INSTANCEOF_EXPR)
    else {
        return false;
    };
    let Some(host) = pattern_expression.ancestors().skip(1).find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::IF_STMT | SyntaxKind::WHILE_STMT
        )
    }) else {
        return false;
    };
    let Some(condition) = host
        .children()
        .filter(|child| is_expression(child.kind()))
        .last()
    else {
        return false;
    };

    if expression_contains(&condition, reference) {
        return expression_evaluation_requires_pattern(&condition, reference, &pattern_expression);
    }

    let bodies: Vec<_> = host
        .children()
        .filter(|child| child.text_range().start() >= condition.text_range().end())
        .collect();
    match host.kind() {
        SyntaxKind::WHILE_STMT => bodies.first().is_some_and(|body| {
            is_node_within(reference, body)
                && condition_outcome_requires_pattern(&condition, &pattern_expression, true)
        }),
        SyntaxKind::IF_STMT => {
            if bodies
                .first()
                .is_some_and(|body| is_node_within(reference, body))
            {
                return condition_outcome_requires_pattern(&condition, &pattern_expression, true);
            }
            if bodies
                .get(1)
                .is_some_and(|body| is_node_within(reference, body))
            {
                return condition_outcome_requires_pattern(&condition, &pattern_expression, false);
            }
            if reference.text_range().start() < host.text_range().end() {
                return false;
            }
            let Some(parent) = host.parent() else {
                return false;
            };
            if !is_node_within(reference, &parent) {
                return false;
            }

            let true_completes = bodies.first().is_none_or(statement_can_complete_normally);
            let false_completes = bodies.get(1).is_none_or(statement_can_complete_normally);
            (!true_completes
                || condition_outcome_requires_pattern(&condition, &pattern_expression, true))
                && (!false_completes
                    || condition_outcome_requires_pattern(&condition, &pattern_expression, false))
        }
        _ => false,
    }
}

fn statement_can_complete_normally(statement: &SyntaxNode) -> bool {
    match statement.kind() {
        SyntaxKind::RETURN_STMT
        | SyntaxKind::THROW_STMT
        | SyntaxKind::BREAK_STMT
        | SyntaxKind::CONTINUE_STMT
        | SyntaxKind::YIELD_STMT => false,
        SyntaxKind::BLOCK_STMT => statement
            .children()
            .last()
            .is_none_or(|last| statement_can_complete_normally(&last)),
        SyntaxKind::IF_STMT => {
            let Some(condition) = statement
                .children()
                .filter(|child| is_expression(child.kind()))
                .last()
            else {
                return true;
            };
            let bodies: Vec<_> = statement
                .children()
                .filter(|child| child.text_range().start() >= condition.text_range().end())
                .collect();
            bodies.get(1).is_none() || bodies.iter().any(statement_can_complete_normally)
        }
        _ => true,
    }
}

#[derive(Clone, Copy)]
struct BooleanOutcomes {
    false_value: bool,
    true_value: bool,
}

impl BooleanOutcomes {
    const BOTH: Self = Self {
        false_value: true,
        true_value: true,
    };

    const fn exact(value: bool) -> Self {
        Self {
            false_value: !value,
            true_value: value,
        }
    }

    const fn contains(self, value: bool) -> bool {
        if value {
            self.true_value
        } else {
            self.false_value
        }
    }
}

fn condition_outcome_requires_pattern(
    condition: &SyntaxNode,
    pattern: &SyntaxNode,
    outcome: bool,
) -> bool {
    !condition_outcomes(condition, pattern, false).contains(outcome)
}

fn condition_outcomes(
    expression: &SyntaxNode,
    pattern: &SyntaxNode,
    pattern_matched: bool,
) -> BooleanOutcomes {
    if expression == pattern {
        return BooleanOutcomes::exact(pattern_matched);
    }
    match expression.kind() {
        SyntaxKind::PAREN_EXPR | SyntaxKind::CAST_EXPR => expression
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
            .map_or(BooleanOutcomes::BOTH, |inner| {
                condition_outcomes(&inner, pattern, pattern_matched)
            }),
        SyntaxKind::UNARY_EXPR
            if expression
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|token| token.kind() == SyntaxKind::BANG) =>
        {
            let Some(operand) = expression
                .children()
                .find(|child| is_expression(child.kind()))
                .or_else(|| super::patterns::continuation_left_operand(expression))
            else {
                return BooleanOutcomes::BOTH;
            };
            let inner = condition_outcomes(&operand, pattern, pattern_matched);
            BooleanOutcomes {
                false_value: inner.true_value,
                true_value: inner.false_value,
            }
        }
        SyntaxKind::BINARY_EXPR => {
            let Some((operator, left, right)) = boolean_binary_parts(expression) else {
                return BooleanOutcomes::BOTH;
            };
            let left = condition_outcomes(&left, pattern, pattern_matched);
            let right = condition_outcomes(&right, pattern, pattern_matched);
            match operator {
                SyntaxKind::AMP_AMP => BooleanOutcomes {
                    false_value: left.false_value || (left.true_value && right.false_value),
                    true_value: left.true_value && right.true_value,
                },
                SyntaxKind::PIPE_PIPE => BooleanOutcomes {
                    false_value: left.false_value && right.false_value,
                    true_value: left.true_value || (left.false_value && right.true_value),
                },
                _ => BooleanOutcomes::BOTH,
            }
        }
        _ => BooleanOutcomes::BOTH,
    }
}

fn expression_evaluation_requires_pattern(
    expression: &SyntaxNode,
    target: &SyntaxNode,
    pattern: &SyntaxNode,
) -> bool {
    !expression_may_evaluate_target(expression, target, pattern, false)
}

fn expression_may_evaluate_target(
    expression: &SyntaxNode,
    target: &SyntaxNode,
    pattern: &SyntaxNode,
    pattern_matched: bool,
) -> bool {
    if expression == target {
        return true;
    }
    if expression.kind() == SyntaxKind::BINARY_EXPR {
        if let Some((operator, left, right)) = boolean_binary_parts(expression) {
            if expression_contains(&left, target) {
                return expression_may_evaluate_target(&left, target, pattern, pattern_matched);
            }
            if expression_contains(&right, target) {
                let required_left = operator == SyntaxKind::AMP_AMP;
                return condition_outcomes(&left, pattern, pattern_matched).contains(required_left)
                    && expression_may_evaluate_target(&right, target, pattern, pattern_matched);
            }
        }
    }
    for child in expression
        .children()
        .filter(|child| is_expression(child.kind()))
    {
        if expression_contains(&child, target) {
            return expression_may_evaluate_target(&child, target, pattern, pattern_matched);
        }
    }
    is_node_within(target, expression)
}

fn expression_contains(expression: &SyntaxNode, target: &SyntaxNode) -> bool {
    if expression == target || is_node_within(target, expression) {
        return true;
    }
    super::patterns::continuation_left_operand(expression)
        .is_some_and(|left| expression_contains(&left, target))
}

fn boolean_binary_parts(expression: &SyntaxNode) -> Option<(SyntaxKind, SyntaxNode, SyntaxNode)> {
    let operator = expression
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| matches!(token.kind(), SyntaxKind::AMP_AMP | SyntaxKind::PIPE_PIPE))?
        .kind();
    let operands: Vec<_> = expression
        .children()
        .filter(|child| is_expression(child.kind()))
        .collect();
    let (left, right) = match operands.as_slice() {
        [left, right, ..] => (left.clone(), right.clone()),
        [right] => (
            super::patterns::continuation_left_operand(expression)?,
            right.clone(),
        ),
        _ => return None,
    };
    Some((operator, left, right))
}

/// Canonical half-open byte range used as a nullability diagnostic location.
pub use hyperion_types::TextRange as SourceLocation;

/// Diagnostic from nullability analysis
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NullabilityDiagnostic {
    pub kind: DiagnosticKind,
    pub location: SourceLocation,
    pub variable_name: String,
    pub nullability: Nullability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    /// Dereferencing a potentially null value
    PotentialNullDereference,
    /// Null passed to @NonNull parameter
    NullPassedToNonNull,
    /// Null returned from @NonNull method
    NullReturnFromNonNull,
    /// Redundant null check (value is already non-null)
    RedundantNullCheck,
    /// Dead code after null check
    DeadCode,
}

/// Results of nullability analysis
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NullabilityAnalysis {
    /// Environment at entry to each block
    pub entry_states: HashMap<BlockId, NullabilityEnv>,
    /// Environment at exit from each block
    pub exit_states: HashMap<BlockId, NullabilityEnv>,
    /// Collected diagnostics
    pub diagnostics: Vec<NullabilityDiagnostic>,
    /// Typed exceptional completions which escaped this graph. Nested
    /// expression CFGs feed these directly into their parent evaluation.
    uncaught_exceptions: ExceptionalExits,
    /// Values from return completions which survived every crossed cleanup.
    /// Kept here because projecting lane states by block loses that fact.
    inferred_return: Option<Nullability>,
    /// Values from yields which survived every crossed cleanup.
    inferred_yield: Option<Nullability>,
}

impl NullabilityAnalysis {
    /// Get nullability of a variable at a specific location
    pub fn get_nullability_at(&self, block: BlockId, var: VariableId) -> Nullability {
        self.entry_states
            .get(&block)
            .map_or(Nullability::Unknown, |env| env.get(var))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum FlowLane {
    Normal,
    PendingStatic {
        resume: BlockId,
        stop_cleanup: Option<CleanupScopeId>,
        awaiting_cleanup: CleanupScopeId,
        continuation: PendingId,
    },
    PendingException {
        kind: ExceptionKind,
        destination: ExceptionDestination,
        awaiting_cleanup: CleanupScopeId,
        continuation: PendingId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PendingId(u32);

#[derive(Debug)]
struct LaneInterner {
    lanes: Vec<FlowLane>,
    ids: HashMap<FlowLane, PendingId>,
}

impl LaneInterner {
    fn new() -> Self {
        Self {
            lanes: vec![FlowLane::Normal],
            ids: HashMap::from([(FlowLane::Normal, PendingId(0))]),
        }
    }

    fn intern(&mut self, lane: FlowLane) -> PendingId {
        if let Some(id) = self.ids.get(&lane) {
            return *id;
        }
        let id = PendingId(u32::try_from(self.lanes.len()).expect("flow lane arena exceeds u32"));
        self.lanes.push(lane.clone());
        self.ids.insert(lane, id);
        id
    }

    fn get(&self, id: PendingId) -> &FlowLane {
        &self.lanes[id.0 as usize]
    }

    fn id(&self, lane: &FlowLane) -> Option<PendingId> {
        self.ids.get(lane).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct FlowPoint {
    block: BlockId,
    pending: PendingId,
}

#[derive(Debug)]
struct FlowSuccessor {
    block: BlockId,
    lane: FlowLane,
}

#[derive(Debug, Clone)]
enum CompletionValue {
    Return {
        value: Nullability,
        normal_exit: NullabilityEnv,
    },
    Yield(Nullability),
}

fn join_projected_state(
    states: &mut HashMap<BlockId, NullabilityEnv>,
    block: BlockId,
    env: &NullabilityEnv,
) {
    if let Some(existing) = states.get_mut(&block) {
        *existing = existing.join(env);
    } else {
        states.insert(block, env.clone());
    }
}

fn exception_successor(
    cfg: &ControlFlowGraph,
    active_cleanup: Option<CleanupScopeId>,
    kind: ExceptionKind,
    destination: ExceptionDestination,
    continuation: PendingId,
    lanes: &LaneInterner,
) -> Option<FlowSuccessor> {
    if active_cleanup != destination.cleanup_scope {
        let cleanup = active_cleanup?;
        return Some(FlowSuccessor {
            block: cfg.cleanup_scope(cleanup).entry,
            lane: FlowLane::PendingException {
                kind,
                destination,
                awaiting_cleanup: cleanup,
                continuation,
            },
        });
    }
    destination.target.map(|block| FlowSuccessor {
        block,
        lane: lanes.get(continuation).clone(),
    })
}

const fn awaited_cleanup(lane: &FlowLane) -> Option<CleanupScopeId> {
    match lane {
        FlowLane::Normal => None,
        FlowLane::PendingStatic {
            awaiting_cleanup, ..
        }
        | FlowLane::PendingException {
            awaiting_cleanup, ..
        } => Some(*awaiting_cleanup),
    }
}

fn completion_lane_for_target(
    cfg: &ControlFlowGraph,
    target: BlockId,
    incoming: &FlowLane,
) -> FlowLane {
    awaited_cleanup(incoming)
        .filter(|cleanup| cfg.block_is_within_cleanup_region(target, *cleanup))
        .map_or(FlowLane::Normal, |_| incoming.clone())
}

fn exception_continuation(
    cfg: &ControlFlowGraph,
    destination: ExceptionDestination,
    incoming: &FlowLane,
    incoming_id: PendingId,
) -> PendingId {
    destination
        .target
        .zip(awaited_cleanup(incoming))
        .filter(|(target, cleanup)| cfg.block_is_within_cleanup_region(*target, *cleanup))
        .map_or(PendingId(0), |_| incoming_id)
}

fn normal_successor_lane(
    cfg: &ControlFlowGraph,
    terminator: &Terminator,
    incoming: &FlowLane,
    incoming_id: PendingId,
    active_cleanup: Option<CleanupScopeId>,
) -> FlowLane {
    match terminator {
        Terminator::ReturnThroughCleanup {
            resume,
            stop_cleanup,
            ..
        }
        | Terminator::EnterCleanup {
            resume,
            stop_cleanup,
            ..
        } => FlowLane::PendingStatic {
            resume: *resume,
            stop_cleanup: *stop_cleanup,
            awaiting_cleanup: active_cleanup
                .expect("cleanup entry terminator must be inside the cleanup it enters"),
            continuation: incoming_id,
        },
        Terminator::Yield {
            target,
            resume,
            stop_cleanup,
            ..
        } if target != resume => FlowLane::PendingStatic {
            resume: *resume,
            stop_cleanup: *stop_cleanup,
            awaiting_cleanup: active_cleanup
                .expect("cleanup-bound yield must be inside the cleanup it enters"),
            continuation: incoming_id,
        },
        // A completion local to the currently executing cleanup does not
        // replace the older return/throw that caused entry into that cleanup.
        // One which leaves the cleanup does replace it.
        Terminator::Yield { target, .. }
        | Terminator::MissingYield(target)
        | Terminator::ResumeYield(target)
        | Terminator::Break(target)
        | Terminator::Continue(target) => completion_lane_for_target(cfg, *target, incoming),
        _ => incoming.clone(),
    }
}

/// Whether the completion produced at `point` reaches its source-specific
/// resume block. A block-only lookup is insufficient: one invocation of a
/// shared `finally` can override a completion while another invocation of the
/// same source terminator survives.
fn completion_is_accepted(
    cfg: &ControlFlowGraph,
    terminator: &Terminator,
    point: FlowPoint,
    lanes: &LaneInterner,
    lane_entry_states: &HashMap<FlowPoint, NullabilityEnv>,
) -> bool {
    match terminator {
        Terminator::ReturnThroughCleanup { resume, .. } => {
            lane_entry_states.contains_key(&FlowPoint {
                block: *resume,
                pending: point.pending,
            })
        }
        Terminator::Yield { target, resume, .. } if target != resume => lane_entry_states
            .contains_key(&FlowPoint {
                block: *resume,
                pending: point.pending,
            }),
        Terminator::Yield { target, .. } => {
            let successor_lane = completion_lane_for_target(cfg, *target, lanes.get(point.pending));
            lanes.id(&successor_lane).is_some_and(|pending| {
                lane_entry_states.contains_key(&FlowPoint {
                    block: *target,
                    pending,
                })
            })
        }
        _ => true,
    }
}

/// Perform forward dataflow analysis on a CFG
pub fn analyze<R: VariableResolver>(
    cfg: &ControlFlowGraph,
    initial_env: NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
) -> NullabilityAnalysis {
    analyze_internal(cfg, initial_env, resolver, method_context, false).0
}

fn analyze_internal<R: VariableResolver>(
    cfg: &ControlFlowGraph,
    initial_env: NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    capture_return_env: bool,
) -> (NullabilityAnalysis, Option<NullabilityEnv>) {
    // The CFG owns the exception/type revision used to build its handler
    // arena. Expression effects must use that exact same revision; accepting
    // an independently resolved MethodContext here can silently route one
    // completion under two different type universes.
    let cfg_exception_types = cfg.exception_type_revision();
    let fallback_context = (!method_context
        .exception_types
        .is_same_revision(&cfg_exception_types))
    .then(|| {
        let mut context = method_context.clone();
        context.exception_types = cfg_exception_types;
        context
    });
    let method_context = fallback_context.as_ref().unwrap_or(method_context);
    let mut lanes = LaneInterner::new();
    let entry = FlowPoint {
        block: cfg.entry,
        pending: PendingId(0),
    };
    let mut lane_entry_states = HashMap::from([(entry, initial_env)]);
    let mut lane_exit_states: HashMap<FlowPoint, NullabilityEnv> = HashMap::new();
    let mut uncaught_exceptions = ExceptionalExits::new();
    let mut diagnostics = Vec::new();
    let mut worklist = VecDeque::from([entry]);
    let mut in_worklist = HashSet::from([entry]);

    // Diagnostics are NOT collected during the worklist iterations: blocks are
    // re-processed as states widen, so diagnostics computed from optimistic
    // pre-fixpoint states can be both duplicated and wrong — anti-monotone
    // ones (RedundantNullCheck, on states that later widen to may-be-null)
    // would never be retracted. A single post-fixpoint pass over
    // the converged entry states emits each diagnostic exactly once.
    let mut scratch_diagnostics = Vec::new();

    while let Some(point) = worklist.pop_front() {
        in_worklist.remove(&point);

        let Some(block) = cfg.blocks.get(&point.block) else {
            continue;
        };
        let entry_env = lane_entry_states.get(&point).cloned().unwrap_or_default();
        let incoming_lane = lanes.get(point.pending).clone();

        // Transfer function: process statements
        let mut env = entry_env.clone();

        scratch_diagnostics.clear();
        let mut completes_normally = true;
        let mut exceptional_exit = ExceptionalExits::new();
        for stmt in &block.statements {
            let completion = process_statement(
                stmt,
                &mut env,
                resolver,
                method_context,
                &mut scratch_diagnostics,
            );
            exceptional_exit = join_optional_env(exceptional_exit, completion.exceptional_exit);
            if !completion.completes_normally {
                completes_normally = false;
                break;
            }
        }

        // Handle terminator
        let successor_envs = if completes_normally {
            let mut ignored_completion = None;
            transfer_terminator(
                &block.terminator,
                &env,
                resolver,
                method_context,
                &mut scratch_diagnostics,
                false,
                &mut exceptional_exit,
                &mut ignored_completion,
            )
        } else {
            Vec::new()
        };

        // An expression embedded in a statement can complete abruptly (for
        // example an all-throw switch initializer). Such a block has no
        // normal exit state; publishing one lets downstream constructor and
        // summary consumers mistake an unreachable terminator for a real
        // completion.
        if completes_normally {
            lane_exit_states.insert(point, env.clone());
        } else {
            lane_exit_states.remove(&point);
        }

        let mut successors: Vec<(FlowSuccessor, NullabilityEnv)> = Vec::new();
        for (succ_id, succ_env) in successor_envs {
            // A pending static completion reaching the normal end of a shared
            // finally advances outward, then resumes its return/yield/jump.
            if let (
                Terminator::CleanupEnd { cleanup, .. },
                FlowLane::PendingStatic {
                    resume,
                    stop_cleanup,
                    awaiting_cleanup,
                    continuation,
                },
            ) = (&block.terminator, &incoming_lane)
            {
                if cleanup == awaiting_cleanup {
                    let parent_cleanup = cfg.cleanup_scope(*cleanup).parent;
                    let next = if parent_cleanup == *stop_cleanup {
                        FlowSuccessor {
                            block: *resume,
                            lane: lanes.get(*continuation).clone(),
                        }
                    } else {
                        let next_cleanup = parent_cleanup
                            .expect("pending cleanup chain ended before its boundary");
                        FlowSuccessor {
                            block: cfg.cleanup_scope(next_cleanup).entry,
                            lane: FlowLane::PendingStatic {
                                resume: *resume,
                                stop_cleanup: *stop_cleanup,
                                awaiting_cleanup: next_cleanup,
                                continuation: *continuation,
                            },
                        }
                    };
                    successors.push((next, succ_env));
                    continue;
                }
            }
            // A pending exception similarly advances to the next cleanup or
            // its already-selected catch.
            if let (
                Terminator::CleanupEnd { cleanup, .. },
                FlowLane::PendingException {
                    kind,
                    destination,
                    awaiting_cleanup,
                    continuation,
                },
            ) = (&block.terminator, &incoming_lane)
            {
                if cleanup == awaiting_cleanup {
                    let parent_cleanup = cfg.cleanup_scope(*cleanup).parent;
                    if let Some(next) = exception_successor(
                        cfg,
                        parent_cleanup,
                        kind.clone(),
                        *destination,
                        *continuation,
                        &lanes,
                    ) {
                        successors.push((next, succ_env));
                    } else {
                        uncaught_exceptions.push(ExceptionalExit {
                            kind: kind.clone(),
                            env: succ_env,
                        });
                    }
                    continue;
                }
            }
            successors.push((
                FlowSuccessor {
                    block: succ_id,
                    lane: normal_successor_lane(
                        cfg,
                        &block.terminator,
                        &incoming_lane,
                        point.pending,
                        block.cleanup_scope,
                    ),
                },
                succ_env,
            ));
        }

        // A close failure while an existing exception is crossing this exact
        // resource cleanup is suppressed by Java. The primary exception stays
        // on the pending lane and advances through CleanupEnd below. In every
        // other lane, a close failure is primary and routes normally (possibly
        // crossing earlier resources before entering a catch).
        let suppress_close_failures = matches!(
            (&block.terminator, &incoming_lane),
            (
                Terminator::CleanupEnd { cleanup, .. },
                FlowLane::PendingException {
                    awaiting_cleanup,
                    ..
                }
            ) if cleanup == awaiting_cleanup
                && cfg.cleanup_scope(*cleanup).kind == CleanupKind::Resource
        );

        // Each non-suppressed exceptional completion retains its own exact
        // state and type. Catch selection is ordered within a handler group;
        // only its residual may continue outward. Cleanup bodies are shared
        // and the pending lane prevents distinct destinations from being
        // joined prematurely.
        for exceptional in exceptional_exit
            .into_iter()
            .filter(|_| !suppress_close_failures)
        {
            let destinations = match &exceptional.kind {
                ExceptionKind::Unknown => cfg.exception_destinations(block.handler_scope, None),
                ExceptionKind::Known(name) => {
                    cfg.exception_destinations(block.handler_scope, Some(name))
                }
                ExceptionKind::SubtypeOf(upper_bound) => {
                    cfg.exception_destinations_for_upper_bound(block.handler_scope, upper_bound)
                }
            };
            for destination in destinations {
                let continuation =
                    exception_continuation(cfg, destination, &incoming_lane, point.pending);
                if let Some(next) = exception_successor(
                    cfg,
                    block.cleanup_scope,
                    exceptional.kind.clone(),
                    destination,
                    continuation,
                    &lanes,
                ) {
                    successors.push((next, exceptional.env.clone()));
                } else {
                    uncaught_exceptions.push(ExceptionalExit {
                        kind: exceptional.kind.clone(),
                        env: exceptional.env.clone(),
                    });
                }
            }
        }

        for (successor, successor_env) in successors {
            let successor = FlowPoint {
                block: successor.block,
                pending: lanes.intern(successor.lane),
            };
            let changed = if let Some(existing) = lane_entry_states.get(&successor) {
                let joined = existing.join(&successor_env);
                if joined == *existing {
                    false
                } else {
                    lane_entry_states.insert(successor, joined);
                    true
                }
            } else {
                lane_entry_states.insert(successor, successor_env);
                true
            };

            if changed && in_worklist.insert(successor) {
                worklist.push_back(successor);
            }
        }
    }

    let mut entry_states = HashMap::new();
    for (point, env) in &lane_entry_states {
        join_projected_state(&mut entry_states, point.block, env);
    }
    let mut exit_states = HashMap::new();
    for (point, env) in &lane_exit_states {
        join_projected_state(&mut exit_states, point.block, env);
    }

    // Post-fixpoint diagnostic pass: replay every reachable block once against
    // its converged entry state. Blocks without an entry state were never
    // reached by the worklist (unreachable) and stay silent, matching the old
    // behavior. Sorted by block id for deterministic diagnostic order.
    let mut reachable_points: Vec<_> = lane_entry_states.keys().copied().collect();
    reachable_points.sort();
    let mut inferred_return = None;
    let mut inferred_return_env: Option<NullabilityEnv> = None;
    let mut inferred_yield = None;

    for point in reachable_points {
        let Some(block) = cfg.blocks.get(&point.block) else {
            continue;
        };
        let mut env = lane_entry_states.get(&point).cloned().unwrap_or_default();

        let mut completes_normally = true;
        for stmt in &block.statements {
            if !process_statement(stmt, &mut env, resolver, method_context, &mut diagnostics)
                .completes_normally
            {
                completes_normally = false;
                break;
            }
        }
        if completes_normally {
            let completion_is_accepted =
                completion_is_accepted(cfg, &block.terminator, point, &lanes, &lane_entry_states);
            let mut ignored_exceptional_exit = ExceptionalExits::new();
            let mut completion = None;
            transfer_terminator(
                &block.terminator,
                &env,
                resolver,
                method_context,
                &mut diagnostics,
                completion_is_accepted,
                &mut ignored_exceptional_exit,
                &mut completion,
            );
            if completion_is_accepted {
                match completion {
                    Some(CompletionValue::Return {
                        value,
                        mut normal_exit,
                    }) => {
                        if let Terminator::ReturnThroughCleanup { resume, .. } = &block.terminator {
                            normal_exit = lane_entry_states
                                .get(&FlowPoint {
                                    block: *resume,
                                    pending: point.pending,
                                })
                                .cloned()
                                .expect("accepted cleanup return must reach its resume block");
                        }
                        inferred_return = Some(
                            inferred_return
                                .map_or(value, |current: Nullability| current.join(value)),
                        );
                        if capture_return_env {
                            inferred_return_env = Some(inferred_return_env.map_or_else(
                                || normal_exit.clone(),
                                |current| current.join(&normal_exit),
                            ));
                        }
                    }
                    Some(CompletionValue::Yield(value)) => {
                        inferred_yield = Some(
                            inferred_yield
                                .map_or(value, |current: Nullability| current.join(value)),
                        );
                    }
                    None => {}
                }
            }
        }
    }

    deduplicate_diagnostics(&mut diagnostics);
    diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic.location.start,
            diagnostic.location.end,
            diagnostic.kind as u8,
        )
    });

    (
        NullabilityAnalysis {
            entry_states,
            exit_states,
            diagnostics,
            uncaught_exceptions,
            inferred_return,
            inferred_yield,
        },
        inferred_return_env,
    )
}

fn deduplicate_diagnostics(diagnostics: &mut Vec<NullabilityDiagnostic>) {
    let mut first_by_source: HashMap<_, usize> = HashMap::new();
    let mut deduplicated: Vec<NullabilityDiagnostic> = Vec::with_capacity(diagnostics.len());
    for diagnostic in diagnostics.drain(..) {
        let key = (
            diagnostic.kind,
            diagnostic.location.start,
            diagnostic.location.end,
            diagnostic.variable_name.clone(),
        );
        if let Some(index) = first_by_source.get(&key).copied() {
            let existing = &mut deduplicated[index];
            existing.nullability = existing.nullability.join(diagnostic.nullability);
        } else {
            first_by_source.insert(key, deduplicated.len());
            deduplicated.push(diagnostic);
        }
    }
    *diagnostics = deduplicated;
}

/// Return the value from completions which survived every crossed cleanup.
///
/// The analysis retains only completions which survive every crossed cleanup;
/// block-projected states cannot recover that lane-specific fact. `None`
/// means the method has no reachable value return.
pub const fn accepted_return_nullability(analysis: &NullabilityAnalysis) -> Option<Nullability> {
    analysis.inferred_return
}

/// Infer values from reachable source `return` blocks.
///
/// This compatibility API predates lane-aware cleanup analysis. New code
/// should analyze the CFG and call [`accepted_return_nullability`] so a return
/// overridden by `finally` is not resurrected from a projected block state.
#[deprecated(note = "use analyze() followed by accepted_return_nullability()")]
#[allow(clippy::implicit_hasher)]
pub fn infer_return_nullability<R: VariableResolver>(
    cfg: &ControlFlowGraph,
    entry_states: &HashMap<BlockId, NullabilityEnv>,
    resolver: &R,
    method_context: &MethodContext,
) -> Option<Nullability> {
    let mut inferred = None;
    let mut scratch_diagnostics = Vec::new();
    for (block_id, block) in &cfg.blocks {
        let Some(entry_env) = entry_states.get(block_id) else {
            continue;
        };
        let Terminator::Return(Some(returned)) = &block.terminator else {
            continue;
        };
        let mut env = entry_env.clone();
        if block.statements.iter().any(|statement| {
            !process_statement(
                statement,
                &mut env,
                resolver,
                method_context,
                &mut scratch_diagnostics,
            )
            .completes_normally
        }) {
            continue;
        }
        let value = expr_nullability(&returned.node, &env, resolver, method_context);
        inferred = Some(inferred.map_or(value, |current: Nullability| current.join(value)));
    }
    inferred
}

/// Infer the value produced by every reachable switch-expression yield.
pub(crate) const fn infer_yield_nullability(analysis: &NullabilityAnalysis) -> Option<Nullability> {
    analysis.inferred_yield
}

/// Collect nullability for arguments at call sites within the current method.
#[allow(clippy::implicit_hasher)]
pub fn collect_call_site_nullability<R: VariableResolver>(
    cfg: &ControlFlowGraph,
    entry_states: &HashMap<BlockId, NullabilityEnv>,
    resolver: &R,
    method_context: &MethodContext,
    class_name: &str,
) -> HashMap<MethodCallKey, Vec<Option<Nullability>>> {
    let mut call_sites: HashMap<MethodCallKey, Vec<Option<Nullability>>> = HashMap::new();
    let mut scratch_diagnostics = Vec::new();
    let structurally_reachable = structurally_reachable_blocks(cfg);

    {
        let mut accumulator = CallSiteAccumulator::enabled(class_name, &mut call_sites);
        for (block_id, block) in &cfg.blocks {
            if !structurally_reachable.contains(block_id) {
                continue;
            }
            // Catch/finally entries are conservatively observable even while
            // exceptional flow lacks full state propagation. An ordinary block
            // with no entry state is genuinely unreachable and must stay out of
            // summaries (notably after an all-throw nested expression).
            let mut env = if let Some(entry) = entry_states.get(block_id) {
                entry.clone()
            } else if cfg.is_conservative_entry(*block_id) {
                NullabilityEnv::default()
            } else {
                continue;
            };

            let mut completes_normally = true;
            for stmt in &block.statements {
                if !process_statement_observed(
                    stmt,
                    &mut env,
                    resolver,
                    method_context,
                    &mut scratch_diagnostics,
                    &mut accumulator,
                )
                .completes_normally
                {
                    completes_normally = false;
                    break;
                }
            }

            if completes_normally {
                observe_terminator_calls(
                    &block.terminator,
                    &env,
                    resolver,
                    method_context,
                    &mut scratch_diagnostics,
                    &mut accumulator,
                );
            }
        }
    }

    call_sites
}

fn structurally_reachable_blocks(cfg: &ControlFlowGraph) -> HashSet<BlockId> {
    let mut reachable = HashSet::new();
    let mut worklist = VecDeque::from([cfg.entry]);
    while let Some(block) = worklist.pop_front() {
        if reachable.insert(block) {
            worklist.extend(cfg.conservative_successors(block));
        }
    }
    reachable
}

/// Apply transfer function for terminators
fn transfer_terminator<R: VariableResolver>(
    terminator: &Terminator,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    validate_return_contract: bool,
    exceptional_exit: &mut ExceptionalExits,
    completion: &mut Option<CompletionValue>,
) -> Vec<(BlockId, NullabilityEnv)> {
    match terminator {
        Terminator::Goto(target)
        | Terminator::EnterCleanup { target, .. }
        | Terminator::CleanupEnd {
            normal_target: target,
            ..
        }
        | Terminator::ResumeYield(target)
        | Terminator::Break(target)
        | Terminator::Continue(target) => vec![(*target, env.clone())],

        Terminator::MissingYield(target) => {
            *completion = Some(CompletionValue::Yield(Nullability::Unknown));
            vec![(*target, env.clone())]
        }

        Terminator::Branch {
            condition,
            then_block,
            else_block,
        } => {
            // Check for null dereferences in the condition expression.
            // Condition expressions live on terminators rather than in a block's
            // statement list, so check their dereferences here.
            let flow =
                check_condition_derefs(condition, env, resolver, method_context, diagnostics);
            let mut flow = require_unboxing(
                flow,
                &condition.node,
                RequiredPrimitive::Boolean,
                resolver,
                method_context,
                diagnostics,
            );
            *exceptional_exit =
                join_optional_env(exceptional_exit.take(), flow.exceptional_exit.take());
            let Some(env) = flow.normal_exit else {
                return Vec::new();
            };

            if let Some(value) = constant_boolean_value(&condition.node) {
                return vec![(if value { *then_block } else { *else_block }, env)];
            }

            // Try to analyze the condition for null checks
            analyze_condition_in_context(condition, resolver, method_context).map_or_else(
                // No null check found, propagate unchanged
                || vec![(*then_block, env.clone()), (*else_block, env.clone())],
                |null_check| {
                    maybe_emit_redundant_check(
                        &null_check,
                        condition,
                        &env,
                        resolver,
                        method_context,
                        diagnostics,
                    );
                    let then_env = narrow_environment(&env, &null_check, true);
                    let else_env = narrow_environment(&env, &null_check, false);
                    match known_condition_truth(&null_check, &env) {
                        Some(true) => vec![(*then_block, then_env)],
                        Some(false) => vec![(*else_block, else_env)],
                        None => vec![(*then_block, then_env), (*else_block, else_env)],
                    }
                },
            )
        }

        Terminator::BranchUnknown {
            then_block,
            else_block,
        } => vec![(*then_block, env.clone()), (*else_block, env.clone())],

        Terminator::Return(expr) => {
            if let Some(expr) = expr {
                // A return expression lives on the terminator, not in any block's
                // statements, so its dereferences (`return x.foo();`) are checked
                // here rather than by the per-statement pass.
                let mut flow = check_expression_chain_derefs(
                    &expr.node,
                    env,
                    resolver,
                    method_context,
                    diagnostics,
                );
                if method_context.return_is_primitive {
                    flow = require_unboxing(
                        flow,
                        &expr.node,
                        RequiredPrimitive::Any,
                        resolver,
                        method_context,
                        diagnostics,
                    );
                }
                *exceptional_exit =
                    join_optional_env(exceptional_exit.take(), flow.exceptional_exit.take());
                let Some(normal_exit) = flow.normal_exit.take() else {
                    return Vec::new();
                };
                let value_nullability = flow.value;
                if validate_return_contract {
                    *completion = Some(CompletionValue::Return {
                        value: value_nullability,
                        normal_exit,
                    });
                }
                if validate_return_contract
                    && method_context.return_nullability == Nullability::NonNull
                    && !method_context.return_is_primitive
                    && value_nullability.may_be_null()
                {
                    let range = expr.range();
                    diagnostics.push(NullabilityDiagnostic {
                        kind: DiagnosticKind::NullReturnFromNonNull,
                        location: SourceLocation {
                            start: range.start,
                            end: range.end,
                        },
                        variable_name: "<return>".to_string(),
                        nullability: value_nullability,
                    });
                }
            }
            vec![]
        }

        Terminator::ReturnThroughCleanup { value, target, .. } => {
            let mut flow = check_expression_chain_derefs(
                &value.node,
                env,
                resolver,
                method_context,
                diagnostics,
            );
            if method_context.return_is_primitive {
                flow = require_unboxing(
                    flow,
                    &value.node,
                    RequiredPrimitive::Any,
                    resolver,
                    method_context,
                    diagnostics,
                );
            }
            *exceptional_exit =
                join_optional_env(exceptional_exit.take(), flow.exceptional_exit.take());
            let Some(exit) = flow.normal_exit else {
                return Vec::new();
            };
            if validate_return_contract {
                *completion = Some(CompletionValue::Return {
                    value: flow.value,
                    normal_exit: exit.clone(),
                });
            }
            if validate_return_contract
                && method_context.return_nullability == Nullability::NonNull
                && !method_context.return_is_primitive
                && flow.value.may_be_null()
            {
                let range = value.range();
                diagnostics.push(NullabilityDiagnostic {
                    kind: DiagnosticKind::NullReturnFromNonNull,
                    location: SourceLocation {
                        start: range.start,
                        end: range.end,
                    },
                    variable_name: "<return>".to_string(),
                    nullability: flow.value,
                });
            }
            vec![(*target, exit)]
        }

        Terminator::Throw(expr) => {
            // Like return expressions, the thrown expression is terminator-only,
            // so check its dereferences here (`throw new E(x.getMessage());`).
            let mut flow = check_expression_chain_derefs(
                &expr.node,
                env,
                resolver,
                method_context,
                diagnostics,
            );
            *exceptional_exit =
                join_optional_env(exceptional_exit.take(), flow.exceptional_exit.take());
            if let Some(exit) = flow.normal_exit {
                let source =
                    resolve_variable_from_expr(&expr.node, resolver).map(|(variable, _)| variable);
                if flow.value.may_be_null() {
                    let mut null_exit = exit.clone();
                    if let Some(variable) = source {
                        null_exit.set(variable, Nullability::Null);
                    }
                    exceptional_exit.push(ExceptionalExit {
                        kind: ExceptionKind::NULL_POINTER,
                        env: null_exit,
                    });
                }
                if flow.value != Nullability::Null {
                    let mut thrown_exit = exit;
                    if let Some(variable) = source {
                        thrown_exit.set(variable, Nullability::NonNull);
                    }
                    exceptional_exit.push(ExceptionalExit {
                        kind: thrown_exception_kind(&expr.node, resolver, method_context),
                        env: thrown_exit,
                    });
                }
            }
            vec![]
        }

        Terminator::Yield { value, target, .. } => {
            let mut flow = check_expression_chain_derefs(
                &value.node,
                env,
                resolver,
                method_context,
                diagnostics,
            );
            *exceptional_exit =
                join_optional_env(exceptional_exit.take(), flow.exceptional_exit.take());
            let Some(exit) = flow.normal_exit else {
                return Vec::new();
            };
            *completion = Some(CompletionValue::Yield(flow.value));
            vec![(*target, exit)]
        }

        Terminator::ResumeReturn | Terminator::Exit | Terminator::Unreachable => {
            // No successors
            vec![]
        }

        Terminator::Switch {
            scrutinee,
            cases,
            default,
        } => {
            // The scrutinee is evaluated before dispatch, so check its
            // dereferences here (`switch (x.kind()) { ... }`). A successful
            // deref proves the receiver non-null for the case bodies, so
            // propagate that (mirrors the Branch arm) to avoid re-warning.
            let mut flow = check_expression_chain_derefs(
                &scrutinee.node,
                env,
                resolver,
                method_context,
                diagnostics,
            );
            *exceptional_exit =
                join_optional_env(exceptional_exit.take(), flow.exceptional_exit.take());
            let Some(env) = flow.normal_exit else {
                return Vec::new();
            };

            let null_target = cases.iter().find_map(|(label, target)| {
                label.as_ref().and_then(|label| {
                    (label.node.kind() == SyntaxKind::LITERAL_EXPR
                        && label.node.text().to_string().trim() == "null")
                        .then_some(*target)
                })
            });
            let handles_null = null_target.is_some();
            let default_handles_null = default.is_some_and(|default| Some(default) == null_target);
            if !handles_null && matches!(flow.value, Nullability::Null | Nullability::Nullable) {
                let range = scrutinee.range();
                diagnostics.push(NullabilityDiagnostic {
                    kind: DiagnosticKind::PotentialNullDereference,
                    location: SourceLocation {
                        start: range.start,
                        end: range.end,
                    },
                    variable_name: extract_name_from_expr(&scrutinee.node)
                        .unwrap_or_else(|| "<selector>".to_string()),
                    nullability: flow.value,
                });
                let mut null_env = env.clone();
                if let Some((variable, _)) = resolve_variable_from_expr(&scrutinee.node, resolver) {
                    null_env.set(variable, Nullability::Null);
                }
                exceptional_exit.push(ExceptionalExit {
                    kind: ExceptionKind::NULL_POINTER,
                    env: null_env,
                });
                if flow.value == Nullability::Null {
                    return Vec::new();
                }
            }

            let selector = resolve_variable_from_expr(&scrutinee.node, resolver);
            let mut results: Vec<(BlockId, NullabilityEnv)> = Vec::new();
            let mut add_successor = |target: BlockId, incoming: NullabilityEnv| {
                if let Some((_, existing)) = results
                    .iter_mut()
                    .find(|(existing_target, _)| *existing_target == target)
                {
                    *existing = existing.join(&incoming);
                } else {
                    results.push((target, incoming));
                }
            };
            for (label, target) in cases {
                let accepts_null = label.as_ref().is_some_and(|label| {
                    label.node.kind() == SyntaxKind::LITERAL_EXPR
                        && label.node.text().to_string().trim() == "null"
                });
                if (flow.value == Nullability::Null && !accepts_null)
                    || (flow.value == Nullability::NonNull && accepts_null)
                {
                    continue;
                }
                let mut case_env = env.clone();
                if let Some((variable, _)) = selector.as_ref() {
                    case_env.set(
                        *variable,
                        if accepts_null {
                            Nullability::Null
                        } else {
                            Nullability::NonNull
                        },
                    );
                }
                add_successor(*target, case_env);
            }
            if let Some(def) = default {
                let default_reachable = flow.value != Nullability::Null || default_handles_null;
                if default_reachable {
                    let mut default_env = env.clone();
                    if let Some((variable, _)) = selector {
                        if !default_handles_null {
                            default_env.set(variable, Nullability::NonNull);
                        }
                    }
                    add_successor(*def, default_env);
                }
            }
            results
        }
    }
}

fn observe_terminator_calls<R: VariableResolver>(
    terminator: &Terminator,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) {
    let expression = match terminator {
        Terminator::Branch { condition, .. } => Some(&condition.node),
        Terminator::Return(Some(expression)) | Terminator::Throw(expression) => {
            Some(&expression.node)
        }
        Terminator::ReturnThroughCleanup { value, .. } => Some(&value.node),
        Terminator::Yield { value, .. } => Some(&value.node),
        Terminator::Switch { scrutinee, .. } => Some(&scrutinee.node),
        Terminator::Goto(_)
        | Terminator::MissingYield(_)
        | Terminator::ResumeYield(_)
        | Terminator::BranchUnknown { .. }
        | Terminator::Return(None)
        | Terminator::ResumeReturn
        | Terminator::Break(_)
        | Terminator::Continue(_)
        | Terminator::EnterCleanup { .. }
        | Terminator::CleanupEnd { .. }
        | Terminator::Exit
        | Terminator::Unreachable => None,
    };
    if let Some(expression) = expression {
        evaluate_expression_chain(
            expression,
            env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
    }
}

fn merge_call_site_nullability(
    destination: &mut HashMap<MethodCallKey, Vec<Option<Nullability>>>,
    source: HashMap<MethodCallKey, Vec<Option<Nullability>>>,
) {
    for (key, incoming) in source {
        let entry = destination
            .entry(key)
            .or_insert_with(|| vec![None; incoming.len()]);
        if entry.len() != incoming.len() {
            continue;
        }
        for (current, incoming) in entry.iter_mut().zip(incoming) {
            if let Some(incoming) = incoming {
                *current = Some(current.map_or(incoming, |state| state.join(incoming)));
            }
        }
    }
}

fn is_current_class_call(receiver: Option<&SyntaxNode>, class_name: &str) -> bool {
    let Some(receiver) = receiver else {
        return true;
    };
    if receiver.kind() == SyntaxKind::THIS_EXPR {
        return true;
    }
    extract_name_from_expr(receiver).is_some_and(|name| name == class_name)
}

struct StatementCompletion {
    completes_normally: bool,
    exceptional_exit: ExceptionalExits,
}

impl StatementCompletion {
    const fn normal() -> Self {
        Self {
            completes_normally: true,
            exceptional_exit: ExceptionalExits::new(),
        }
    }
}

fn process_statement<R: VariableResolver>(
    stmt: &StatementRef,
    env: &mut NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) -> StatementCompletion {
    let mut call_sites = CallSiteAccumulator::disabled();
    process_statement_observed(
        stmt,
        env,
        resolver,
        method_context,
        diagnostics,
        &mut call_sites,
    )
}

fn process_statement_observed<R: VariableResolver>(
    stmt: &StatementRef,
    env: &mut NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> StatementCompletion {
    match stmt {
        StatementRef::Bind {
            declaration,
            nullability,
        } => {
            if let Some((variable_name, _)) = scoped_body_binding(declaration) {
                if let Some(var_id) = resolver.resolve_variable_node(&variable_name, declaration) {
                    env.set(var_id, *nullability);
                }
            }
            return StatementCompletion::normal();
        }
        StatementRef::RequireNonNull(node) => {
            let mut flow = evaluate_statement_expression(
                stmt,
                env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            let value = flow.value;
            let Some(post_expression_env) = flow.normal_exit.take() else {
                return StatementCompletion {
                    completes_normally: false,
                    exceptional_exit: flow.exceptional_exit,
                };
            };
            *env = post_expression_env;
            let resolved = check_receiver_dereference(
                node,
                node,
                value,
                resolver,
                method_context,
                diagnostics,
            );
            if value.may_be_null() {
                let mut exceptional = env.clone();
                if let Some(variable) = resolved {
                    exceptional.set(variable, Nullability::Null);
                }
                flow.exceptional_exit.push(ExceptionalExit {
                    kind: ExceptionKind::NULL_POINTER,
                    env: exceptional,
                });
            }
            if value == Nullability::Null {
                return StatementCompletion {
                    completes_normally: false,
                    exceptional_exit: flow.exceptional_exit,
                };
            }
            if let Some(var_id) = resolved {
                env.set(var_id, Nullability::NonNull);
            } else if value.may_be_null() {
                let range = node.text_range();
                diagnostics.push(NullabilityDiagnostic {
                    kind: DiagnosticKind::PotentialNullDereference,
                    location: SourceLocation {
                        start: range.start().into(),
                        end: range.end().into(),
                    },
                    variable_name: "<iterable>".to_string(),
                    nullability: value,
                });
            }
            return StatementCompletion {
                completes_normally: true,
                exceptional_exit: flow.exceptional_exit,
            };
        }
        StatementRef::CloseResource(resource) => {
            return process_close_resource(resource, env, resolver);
        }
        StatementRef::Analyze(_) => {}
    }

    if stmt.kind() == SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION {
        let mut effective_env = env.clone();
        let mut exceptional_exit = ExceptionalExits::new();
        let mut writes = SmallVec::new();
        let arguments = stmt
            .node()
            .children()
            .find(|child| child.kind() == SyntaxKind::ARG_LIST)
            .map_or_else(Vec::new, |arguments| argument_expressions(&arguments));
        for argument in arguments {
            let flow = evaluate_expression_chain(
                &argument,
                &effective_env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            effective_env = match absorb_child_flow(flow, &mut exceptional_exit, &mut writes) {
                AbsorbedChild::Normal(_, _, exit) => exit,
                AbsorbedChild::Abrupt(_) => {
                    return StatementCompletion {
                        completes_normally: false,
                        exceptional_exit,
                    };
                }
            };
        }
        // The selected constructor may complete abruptly after every argument
        // has completed. Its exact effect is not yet part of constructor
        // bindings, so retain the arbitrary exceptional lane.
        exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::Unknown,
            env: effective_env.clone(),
        });
        for field in method_context.constructor_assigned_fields.iter() {
            for path in [field.clone(), format!("this.{field}")] {
                if let Some(variable) = resolver.resolve_variable(&path) {
                    effective_env.set(variable, Nullability::NonNull);
                }
            }
        }
        *env = effective_env;
        return StatementCompletion {
            completes_normally: true,
            exceptional_exit,
        };
    }

    if stmt.kind() == SyntaxKind::LOCAL_VAR_DECL {
        let primitive_base = stmt
            .node()
            .children()
            .any(|node| node.kind() == SyntaxKind::PRIMITIVE_TYPE);
        let common_array = stmt
            .node()
            .children()
            .any(|node| node.kind() == SyntaxKind::ARRAY_TYPE);
        let required_primitive = primitive_base
            .then(|| {
                stmt.node()
                    .descendants()
                    .find(|node| node.kind() == SyntaxKind::PRIMITIVE_TYPE)
            })
            .flatten()
            .map(|primitive| {
                if primitive.text().to_string().trim() == "boolean" {
                    RequiredPrimitive::Boolean
                } else {
                    RequiredPrimitive::Numeric
                }
            });
        let mut exceptional_exit = ExceptionalExits::new();
        for declarator in local_declarators(stmt.node()) {
            let mut flow = declarator.initializer.as_ref().map_or_else(
                || normal_expression(Nullability::Unknown, env),
                |initializer| {
                    evaluate_expression_chain(
                        initializer,
                        env,
                        resolver,
                        method_context,
                        diagnostics,
                        call_sites,
                    )
                },
            );
            if !common_array && !declarator.array_suffix {
                if let (Some(initializer), Some(required)) =
                    (declarator.initializer.as_ref(), required_primitive)
                {
                    flow = require_unboxing(
                        flow,
                        initializer,
                        required,
                        resolver,
                        method_context,
                        diagnostics,
                    );
                }
            }
            let value = if primitive_base && !common_array && !declarator.array_suffix {
                Nullability::NonNull
            } else {
                flow.value
            };
            exceptional_exit = join_optional_env(exceptional_exit, flow.exceptional_exit.take());
            let Some(exit) = flow.normal_exit.take() else {
                return StatementCompletion {
                    completes_normally: false,
                    exceptional_exit,
                };
            };
            *env = exit;
            if let Some(variable) =
                resolver.resolve_declarator(&declarator.name, stmt.node(), declarator.name_start)
            {
                env.set(variable, value);
            }
        }
        return StatementCompletion {
            completes_normally: true,
            exceptional_exit,
        };
    }

    if stmt.kind() == SyntaxKind::ASSERT_STMT {
        return process_assert_statement(
            stmt.node(),
            env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
    }

    let mut flow =
        evaluate_statement_expression(stmt, env, resolver, method_context, diagnostics, call_sites);
    let Some(post_expression_env) = flow.normal_exit.take() else {
        return StatementCompletion {
            completes_normally: false,
            exceptional_exit: flow.exceptional_exit,
        };
    };
    *env = post_expression_env;

    match stmt.kind() {
        SyntaxKind::LOCAL_VAR_DECL => unreachable!("local declarations return above"),
        SyntaxKind::EXPR_STMT => {}
        SyntaxKind::ASSIGNMENT_EXPR => {}
        _ => {}
    }

    StatementCompletion {
        completes_normally: true,
        exceptional_exit: flow.exceptional_exit,
    }
}

fn process_close_resource<R: VariableResolver>(
    resource: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
) -> StatementCompletion {
    let variable = if resource.kind() == SyntaxKind::LOCAL_VAR_DECL {
        local_declarators(resource)
            .into_iter()
            .next()
            .and_then(|declarator| {
                resolver.resolve_declarator(&declarator.name, resource, declarator.name_start)
            })
    } else {
        resolve_variable_from_expr(resource, resolver).map(|(variable, _)| variable)
    };
    let value = variable.map_or(Nullability::Unknown, |variable| env.get(variable));
    let mut exceptional_exit = ExceptionalExits::new();

    // The translation specified by JLS 14.20.3 tests the captured resource
    // against null before invoking close(). A null resource is therefore a
    // no-op, not a null dereference. For every other state we conservatively
    // preserve both normal completion and an unknown close failure.
    if value != Nullability::Null {
        let mut exceptional = env.clone();
        if let Some(variable) = variable {
            exceptional.set(variable, Nullability::NonNull);
        }
        exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::Unknown,
            env: exceptional,
        });
    }

    StatementCompletion {
        completes_normally: true,
        exceptional_exit,
    }
}

fn process_assert_statement<R: VariableResolver>(
    statement: &SyntaxNode,
    env: &mut NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> StatementCompletion {
    let (Some(condition), message) = assert_expression_heads(statement) else {
        return StatementCompletion::normal();
    };
    let condition_flow = evaluate_expression_chain(
        &condition,
        env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let mut condition_flow = require_unboxing(
        condition_flow,
        &condition,
        RequiredPrimitive::Boolean,
        resolver,
        method_context,
        diagnostics,
    );
    let mut exceptional_exit = condition_flow.exceptional_exit.take();
    let Some(condition_exit) = condition_flow.normal_exit.take() else {
        return StatementCompletion {
            completes_normally: false,
            exceptional_exit,
        };
    };

    let condition_pattern =
        analyze_condition_in_context(&ExprRef::new(condition), resolver, method_context);
    let condition_truth = condition_flow.boolean_value.or_else(|| {
        condition_pattern
            .as_ref()
            .and_then(|pattern| known_condition_truth(pattern, &condition_exit))
    });

    if condition_truth != Some(true) {
        let false_exit = condition_pattern.as_ref().map_or_else(
            || condition_exit.clone(),
            |pattern| narrow_environment(&condition_exit, pattern, false),
        );
        let assertion_exit = if let Some(message) = message {
            let mut message_flow = evaluate_expression_chain(
                &message,
                &false_exit,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            exceptional_exit.extend(message_flow.exceptional_exit.take());
            message_flow.normal_exit.take()
        } else {
            Some(false_exit)
        };
        if let Some(assertion_exit) = assertion_exit {
            exceptional_exit.push(ExceptionalExit {
                kind: ExceptionKind::ASSERTION_ERROR,
                env: assertion_exit,
            });
        }
    }

    if condition_truth == Some(false) {
        return StatementCompletion {
            completes_normally: false,
            exceptional_exit,
        };
    }

    *env = condition_pattern
        .as_ref()
        .map_or(condition_exit.clone(), |pattern| {
            narrow_environment(&condition_exit, pattern, true)
        });
    StatementCompletion {
        completes_normally: true,
        exceptional_exit,
    }
}

fn apply_assignment_value_effect<R: VariableResolver>(
    var_id: VariableId,
    env: &mut NullabilityEnv,
    value_nullability: Nullability,
    resolver: &R,
) {
    for dependent in resolver.dependent_variables_for(var_id) {
        env.set(dependent, Nullability::Unknown);
    }
    env.set(var_id, value_nullability);
}

/// Evaluate the expression directly owned by a lowered statement operation.
/// Compound bodies are intentionally absent: the CFG owns their execution.
fn evaluate_statement_expression<R: VariableResolver>(
    stmt: &StatementRef,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let node = stmt.node();
    let head = if is_expression(node.kind()) {
        Some(node.clone())
    } else if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
        find_initializer_expr(node)
    } else {
        node.children()
            .filter(|child| is_expression(child.kind()))
            .last()
    };
    head.map_or_else(
        || normal_expression(Nullability::Unknown, env),
        |head| {
            evaluate_expression_chain(
                &head,
                env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            )
        },
    )
}

/// Check for null dereferences in a branch condition.
///
/// The condition node is the head of the (possibly chained) expression: for
/// `a && b && c` it is the continuation node `[&&, c]`, whose left operand is
/// the preceding sibling. Walk the head and every chain
/// predecessor so dereferences in earlier conjuncts are checked too.
fn check_condition_derefs<R: VariableResolver>(
    condition: &ExprRef,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) -> ExpressionFlow {
    check_expression_chain_derefs(&condition.node, env, resolver, method_context, diagnostics)
}

/// The expression head plus every left operand reached through continuation
/// nodes (`BINARY_EXPR [op, operand]` siblings produced for same-precedence
/// chains). Each returned node's descendants are disjoint from the others'.
fn expression_chain_nodes(head: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut nodes = vec![head.clone()];
    let mut current = head.clone();
    while let Some(left) = super::patterns::continuation_left_operand(&current) {
        nodes.push(left.clone());
        current = left;
    }
    nodes.reverse();
    nodes
}

/// Ordered result of evaluating an expression.
///
/// `normal_exit == None` means evaluation cannot complete normally. Effects
/// and dereference evidence are applied at their evaluation point, never
/// batched and replayed after a later write.
struct ExpressionFlow {
    value: Nullability,
    /// Known primitive boolean result, when evaluation proves one. This is
    /// carried across precede-style continuation siblings so a skipped suffix
    /// cannot be resurrected by re-reading syntax under a later environment.
    boolean_value: Option<bool>,
    normal_exit: Option<NullabilityEnv>,
    /// Joined state at exceptions that escape this expression. This remains
    /// separate from normal completion so an all-throw nested CFG can hand its
    /// completed side effects to the enclosing handler without manufacturing
    /// a normal path.
    exceptional_exit: ExceptionalExits,
    /// Variables whose bindings or stable access paths were assigned while
    /// evaluating this expression. Most expressions write nothing, so keep
    /// the common case inline. The footprint lets users of a captured place
    /// distinguish an argument/RHS that replaced its receiver from one that
    /// merely performed unrelated work.
    writes: SmallVec<[VariableId; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ExceptionKind {
    Unknown,
    Known(Cow<'static, str>),
    /// Runtime identity is unknown but is constrained to this static upper
    /// bound (for example `throw parameter`).
    SubtypeOf(Cow<'static, str>),
}

impl ExceptionKind {
    const NULL_POINTER: Self = Self::Known(Cow::Borrowed("java.lang.NullPointerException"));
    const ARITHMETIC: Self = Self::Known(Cow::Borrowed("java.lang.ArithmeticException"));
    const CLASS_CAST: Self = Self::Known(Cow::Borrowed("java.lang.ClassCastException"));
    const ASSERTION_ERROR: Self = Self::Known(Cow::Borrowed("java.lang.AssertionError"));
    const ILLEGAL_ARGUMENT: Self = Self::Known(Cow::Borrowed("java.lang.IllegalArgumentException"));
    const ILLEGAL_STATE: Self = Self::Known(Cow::Borrowed("java.lang.IllegalStateException"));
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExceptionalExit {
    kind: ExceptionKind,
    env: NullabilityEnv,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ExceptionalExits(Vec<ExceptionalExit>);

impl ExceptionalExits {
    const fn new() -> Self {
        Self(Vec::new())
    }

    fn unknown(env: NullabilityEnv) -> Self {
        Self::known(ExceptionKind::Unknown, env)
    }

    fn known(kind: ExceptionKind, env: NullabilityEnv) -> Self {
        Self(vec![ExceptionalExit { kind, env }])
    }

    fn push(&mut self, exit: ExceptionalExit) {
        if let Some(existing) = self
            .0
            .iter_mut()
            .find(|existing| existing.kind == exit.kind)
        {
            existing.env = existing.env.join(&exit.env);
        } else {
            self.0.push(exit);
        }
    }

    fn extend(&mut self, other: Self) {
        for exit in other.0 {
            self.push(exit);
        }
    }

    fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut ExceptionalExit> {
        self.0.iter_mut()
    }

    fn into_iter(self) -> impl Iterator<Item = ExceptionalExit> {
        self.0.into_iter()
    }
}

impl From<Option<NullabilityEnv>> for ExceptionalExits {
    fn from(env: Option<NullabilityEnv>) -> Self {
        env.map_or_else(Self::new, Self::unknown)
    }
}

trait JoinFlowState: Sized {
    fn joined(self, other: Self) -> Self;
}

impl JoinFlowState for Option<NullabilityEnv> {
    fn joined(self, other: Self) -> Self {
        match (self, other) {
            (Some(left), Some(right)) => Some(left.join(&right)),
            (Some(env), None) | (None, Some(env)) => Some(env),
            (None, None) => None,
        }
    }
}

impl JoinFlowState for ExceptionalExits {
    fn joined(mut self, other: Self) -> Self {
        self.extend(other);
        self
    }
}

fn join_optional_env<T, U>(left: T, right: U) -> T
where
    T: JoinFlowState,
    U: Into<T>,
{
    left.joined(right.into())
}

impl ExpressionFlow {
    fn record_write(&mut self, variable: VariableId) {
        if !self.writes.contains(&variable) {
            self.writes.push(variable);
        }
    }

    fn merge_writes(&mut self, writes: impl IntoIterator<Item = VariableId>) {
        for variable in writes {
            self.record_write(variable);
        }
    }
}

/// Fold one already-evaluated child into a left-to-right expression sequence.
///
/// Keep the three observable products of evaluation -- normal state,
/// exceptional state, and writes -- together. In particular, callers must not
/// `take()` a child's exceptional state and then return that child when it has
/// no normal exit: doing so silently discards the only reachable completion.
enum AbsorbedChild {
    Normal(Nullability, Option<bool>, NullabilityEnv),
    Abrupt(ExpressionFlow),
}

fn absorb_child_flow(
    child: ExpressionFlow,
    exceptional_exit: &mut ExceptionalExits,
    writes: &mut SmallVec<[VariableId; 2]>,
) -> AbsorbedChild {
    let ExpressionFlow {
        value,
        boolean_value,
        normal_exit,
        exceptional_exit: child_exceptional_exit,
        writes: child_writes,
    } = child;
    *exceptional_exit = join_optional_env(exceptional_exit.take(), child_exceptional_exit);
    for variable in child_writes {
        if !writes.contains(&variable) {
            writes.push(variable);
        }
    }
    normal_exit.map_or_else(
        || {
            AbsorbedChild::Abrupt(ExpressionFlow {
                value,
                boolean_value,
                normal_exit: None,
                exceptional_exit: exceptional_exit.take(),
                writes: std::mem::take(writes),
            })
        },
        |exit| AbsorbedChild::Normal(value, boolean_value, exit),
    )
}

struct CapturedPlace {
    variable: VariableId,
    /// Bindings evaluated to compute the place's receiver/index. Reassigning
    /// any of them after capture makes the stable source path denote a
    /// different runtime location.
    address_dependencies: SmallVec<[VariableId; 4]>,
}

fn capture_stable_place<R: VariableResolver>(
    node: &SyntaxNode,
    resolver: &R,
) -> Option<CapturedPlace> {
    let (variable, _) = resolve_variable_from_expr(node, resolver)?;
    let mut address_dependencies = SmallVec::new();
    for child in node.descendants().filter(|child| {
        matches!(
            child.kind(),
            SyntaxKind::NAME_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::ARRAY_ACCESS_EXPR
                | SyntaxKind::PAREN_EXPR
                | SyntaxKind::CAST_EXPR
        )
    }) {
        let Some((dependency, _)) = resolve_variable_from_expr(&child, resolver) else {
            continue;
        };
        if dependency != variable && !address_dependencies.contains(&dependency) {
            address_dependencies.push(dependency);
        }
    }
    Some(CapturedPlace {
        variable,
        address_dependencies,
    })
}

fn write_invalidates_captured_place<R: VariableResolver>(
    writes: &[VariableId],
    captured: &CapturedPlace,
    self_write_invalidates: bool,
    resolver: &R,
) -> bool {
    writes.iter().any(|written| {
        if (self_write_invalidates && *written == captured.variable)
            || captured.address_dependencies.contains(written)
        {
            return true;
        }
        let dependents = resolver.dependent_variables_for(*written);
        dependents.contains(&captured.variable)
            || captured
                .address_dependencies
                .iter()
                .any(|dependency| dependents.contains(dependency))
    })
}

fn conservative_assignment_writes<R: VariableResolver>(
    root: &SyntaxNode,
    resolver: &R,
) -> SmallVec<[VariableId; 2]> {
    let mut writes = SmallVec::new();
    for assignment in root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::ASSIGNMENT_EXPR)
    {
        if assignment
            .ancestors()
            .skip(1)
            .take_while(|ancestor| ancestor != root)
            .any(|ancestor| {
                ancestor.kind().is_type_declaration()
                    || matches!(
                        ancestor.kind(),
                        SyntaxKind::LAMBDA_EXPR
                            | SyntaxKind::METHOD_REF_EXPR
                            | SyntaxKind::CONSTRUCTOR_REF_EXPR
                    )
            })
        {
            continue;
        }
        let Some(target) = assignment_target(&assignment) else {
            continue;
        };
        let Some((variable, _)) = resolve_variable_from_expr(&target, resolver) else {
            continue;
        };
        if !writes.contains(&variable) {
            writes.push(variable);
        }
    }
    writes
}

struct CallSiteAccumulator<'a> {
    class_name: Option<&'a str>,
    call_sites: Option<&'a mut HashMap<MethodCallKey, Vec<Option<Nullability>>>>,
}

impl<'a> CallSiteAccumulator<'a> {
    const fn disabled() -> Self {
        Self {
            class_name: None,
            call_sites: None,
        }
    }

    const fn enabled(
        class_name: &'a str,
        call_sites: &'a mut HashMap<MethodCallKey, Vec<Option<Nullability>>>,
    ) -> Self {
        Self {
            class_name: Some(class_name),
            call_sites: Some(call_sites),
        }
    }

    fn record(&mut self, call: &SyntaxNode, arguments: &[(SyntaxNode, Nullability)]) {
        let (Some(class_name), Some(call_sites)) =
            (self.class_name, self.call_sites.as_deref_mut())
        else {
            return;
        };
        let (Some(method_name), receiver) = method_call_info(call) else {
            return;
        };
        if !is_current_class_call(receiver.as_ref(), class_name) {
            return;
        }
        let key = MethodCallKey {
            class_name: class_name.to_string(),
            method_name,
            arity: arguments.len(),
        };
        let entry = call_sites
            .entry(key)
            .or_insert_with(|| vec![None; arguments.len()]);
        if entry.len() != arguments.len() {
            return;
        }
        for (current, (_, incoming)) in entry.iter_mut().zip(arguments) {
            *current = Some(current.map_or(*incoming, |state| state.join(*incoming)));
        }
    }

    fn merge(&mut self, source: HashMap<MethodCallKey, Vec<Option<Nullability>>>) {
        let Some(destination) = self.call_sites.as_deref_mut() else {
            return;
        };
        merge_call_site_nullability(destination, source);
    }
}

fn check_expression_chain_derefs<R: VariableResolver>(
    head: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) -> ExpressionFlow {
    let mut call_sites = CallSiteAccumulator::disabled();
    evaluate_expression_chain(
        head,
        env,
        resolver,
        method_context,
        diagnostics,
        &mut call_sites,
    )
}

fn evaluate_expression_chain<R: VariableResolver>(
    head: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    // Assignment continuations keep their lvalue as the preceding sibling in
    // the CST. The assignment evaluator owns that lvalue's phased evaluation;
    // treating it as an ordinary expression first performs field/array null
    // checks too early and can evaluate it twice.
    if head.kind() == SyntaxKind::ASSIGNMENT_EXPR
        && super::patterns::continuation_left_operand(head).is_some()
    {
        return evaluate_expression_node(
            head,
            env,
            None,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
    }
    let mut effective_env = env.clone();
    let mut value = Nullability::Unknown;
    let mut boolean_value = None;
    let mut exceptional_exit = ExceptionalExits::new();
    let mut writes = SmallVec::new();
    let chain = expression_chain_nodes(head);
    for (index, node) in chain.iter().enumerate() {
        let mut flow = evaluate_expression_node(
            node,
            &effective_env,
            boolean_value,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        if let Some(required) = chain
            .get(index + 1)
            .and_then(|next| required_primitive_for_continuation(next, resolver, method_context))
        {
            flow = require_unboxing(flow, node, required, resolver, method_context, diagnostics);
        }
        if chain.get(index + 1).is_some_and(unary_mutates_operand) {
            apply_unary_write(&mut flow, node, resolver);
        }
        value = flow.value;
        boolean_value = flow.boolean_value;
        exceptional_exit = join_optional_env(exceptional_exit, flow.exceptional_exit);
        for variable in &flow.writes {
            if !writes.contains(variable) {
                writes.push(*variable);
            }
        }
        let Some(exit) = flow.normal_exit else {
            return ExpressionFlow {
                value,
                boolean_value,
                normal_exit: None,
                exceptional_exit,
                writes,
            };
        };
        effective_env = exit;
    }
    ExpressionFlow {
        value,
        boolean_value,
        normal_exit: Some(effective_env),
        exceptional_exit,
        writes,
    }
}

fn evaluate_expression_node<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    prior_boolean_value: Option<bool>,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    match node.kind() {
        SyntaxKind::SWITCH_EXPR => {
            let evaluated = analyze_switch_expression(node, env, resolver, method_context);
            if let Some(class_name) = call_sites.class_name {
                let nested = collect_call_site_nullability(
                    &evaluated.cfg,
                    &evaluated.analysis.entry_states,
                    resolver,
                    method_context,
                    class_name,
                );
                call_sites.merge(nested);
            }
            diagnostics.extend(evaluated.analysis.diagnostics);
            ExpressionFlow {
                value: evaluated.value,
                boolean_value: None,
                normal_exit: evaluated.normal_exit,
                exceptional_exit: evaluated.exceptional_exit,
                // A nested CFG currently exports its joined environment but
                // not its transfer footprint. Preserve captured-place
                // correctness conservatively until nested graphs are lowered
                // into the parent analysis.
                writes: conservative_assignment_writes(node, resolver),
            }
        }
        SyntaxKind::LAMBDA_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR => ExpressionFlow {
            value: Nullability::NonNull,
            boolean_value: None,
            normal_exit: Some(env.clone()),
            exceptional_exit: ExceptionalExits::new(),
            writes: SmallVec::new(),
        },
        SyntaxKind::METHOD_REF_EXPR => {
            evaluate_method_reference(node, env, resolver, method_context, diagnostics, call_sites)
        }
        SyntaxKind::PAREN_EXPR => {
            let Some(head) = node
                .children()
                .filter(|child| is_expression(child.kind()))
                .last()
            else {
                return normal_expression(Nullability::Unknown, env);
            };
            evaluate_expression_chain(
                &head,
                env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            )
        }
        SyntaxKind::CAST_EXPR => {
            let Some(head) = node
                .children()
                .filter(|child| is_expression(child.kind()))
                .last()
            else {
                return normal_expression(Nullability::Unknown, env);
            };
            let mut flow = evaluate_expression_chain(
                &head,
                env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            if let Some(required) = primitive_cast_requirement(node) {
                flow = apply_primitive_cast_conversion(
                    flow,
                    &head,
                    required,
                    resolver,
                    method_context,
                    diagnostics,
                );
            } else if flow.value != Nullability::Null
                && reference_cast_may_fail(node, &head, resolver, method_context)
            {
                if let Some(exit) = &flow.normal_exit {
                    let mut exit = exit.clone();
                    if let Some((variable, _)) = resolve_variable_from_expr(&head, resolver) {
                        // A null reference cast succeeds. Reaching CCE proves
                        // the captured operand was a non-null incompatible
                        // object, even when the source binding was nullable.
                        exit.set(variable, Nullability::NonNull);
                    }
                    flow.exceptional_exit.push(ExceptionalExit {
                        kind: ExceptionKind::CLASS_CAST,
                        env: exit,
                    });
                }
            }
            flow
        }
        SyntaxKind::ASSIGNMENT_EXPR => evaluate_assignment_expression(
            node,
            env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        ),
        SyntaxKind::METHOD_CALL_EXPR => {
            evaluate_method_call(node, env, resolver, method_context, diagnostics, call_sites)
        }
        SyntaxKind::NEW_EXPR => {
            evaluate_new_expression(node, env, resolver, method_context, diagnostics, call_sites)
        }
        SyntaxKind::FIELD_ACCESS_EXPR => {
            evaluate_field_access(node, env, resolver, method_context, diagnostics, call_sites)
        }
        SyntaxKind::ARRAY_ACCESS_EXPR => {
            evaluate_array_access(node, env, resolver, method_context, diagnostics, call_sites)
        }
        SyntaxKind::TERNARY_EXPR => evaluate_ternary_expression(
            node,
            env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        ),
        SyntaxKind::BINARY_EXPR => evaluate_binary_expression(
            node,
            env,
            prior_boolean_value,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        ),
        SyntaxKind::UNARY_EXPR => evaluate_unary_expression(
            node,
            env,
            prior_boolean_value,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        ),
        _ => evaluate_expression_children(
            node,
            env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        ),
    }
}

fn primitive_cast_requirement(cast: &SyntaxNode) -> Option<RequiredPrimitive> {
    let primitive = cast
        .children()
        .find(|child| child.kind() == SyntaxKind::PRIMITIVE_TYPE)?;
    Some(if primitive.text().to_string().trim() == "boolean" {
        RequiredPrimitive::Boolean
    } else {
        RequiredPrimitive::Numeric
    })
}

fn normal_expression(value: Nullability, env: &NullabilityEnv) -> ExpressionFlow {
    ExpressionFlow {
        value,
        boolean_value: None,
        normal_exit: Some(env.clone()),
        exceptional_exit: ExceptionalExits::new(),
        writes: SmallVec::new(),
    }
}

#[derive(Clone, Copy)]
enum RequiredPrimitive {
    Boolean,
    Numeric,
    Any,
}

const fn primitive_requirement_for_kind(kind: ValueKind) -> Option<RequiredPrimitive> {
    match kind {
        ValueKind::PrimitiveBoolean => Some(RequiredPrimitive::Boolean),
        ValueKind::PrimitiveNumeric | ValueKind::PrimitiveFloating => {
            Some(RequiredPrimitive::Numeric)
        }
        _ => None,
    }
}

fn compound_assignment_requirement<R: VariableResolver>(
    operator: Option<SyntaxKind>,
    target: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> Option<RequiredPrimitive> {
    let target_kind = expression_value_kind(target, resolver, method_context);
    match operator? {
        SyntaxKind::PLUS_EQ if target_kind == ValueKind::StringReference => None,
        SyntaxKind::AMP_EQ | SyntaxKind::PIPE_EQ | SyntaxKind::CARET_EQ
            if matches!(
                target_kind,
                ValueKind::PrimitiveBoolean | ValueKind::BoxedBoolean
            ) =>
        {
            Some(RequiredPrimitive::Boolean)
        }
        SyntaxKind::PLUS_EQ
        | SyntaxKind::MINUS_EQ
        | SyntaxKind::STAR_EQ
        | SyntaxKind::SLASH_EQ
        | SyntaxKind::PERCENT_EQ
        | SyntaxKind::AMP_EQ
        | SyntaxKind::PIPE_EQ
        | SyntaxKind::CARET_EQ
        | SyntaxKind::SHL_EQ
        | SyntaxKind::SHR_EQ
        | SyntaxKind::USHR_EQ => Some(RequiredPrimitive::Numeric),
        _ => None,
    }
}

fn require_unboxing<R: VariableResolver>(
    mut flow: ExpressionFlow,
    expression: &SyntaxNode,
    required: RequiredPrimitive,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) -> ExpressionFlow {
    let kind = expression_value_kind(expression, resolver, method_context);
    let is_boxed = matches!(
        (required, kind),
        (RequiredPrimitive::Boolean, ValueKind::BoxedBoolean)
            | (
                RequiredPrimitive::Numeric,
                ValueKind::BoxedNumeric | ValueKind::BoxedFloating,
            )
            | (
                RequiredPrimitive::Any,
                ValueKind::BoxedBoolean | ValueKind::BoxedNumeric | ValueKind::BoxedFloating
            )
    );
    if !is_boxed {
        return flow;
    }
    let Some(mut normal_exit) = flow.normal_exit.take() else {
        return flow;
    };
    let resolved = check_receiver_dereference(
        expression,
        expression,
        flow.value,
        resolver,
        method_context,
        diagnostics,
    );
    if flow.value.may_be_null() {
        let mut exceptional = normal_exit.clone();
        if let Some(variable) = resolved {
            exceptional.set(variable, Nullability::Null);
        }
        flow.exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::NULL_POINTER,
            env: exceptional,
        });
    }
    if flow.value == Nullability::Null {
        return flow;
    }
    if let Some(variable) = resolved {
        normal_exit.set(variable, Nullability::NonNull);
    }
    flow.value = Nullability::NonNull;
    flow.normal_exit = Some(normal_exit);
    flow
}

fn apply_primitive_cast_conversion<R: VariableResolver>(
    flow: ExpressionFlow,
    operand: &SyntaxNode,
    required: RequiredPrimitive,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) -> ExpressionFlow {
    let kind = expression_value_kind(operand, resolver, method_context);
    if matches!(
        kind,
        ValueKind::BoxedBoolean
            | ValueKind::BoxedNumeric
            | ValueKind::BoxedFloating
            | ValueKind::PrimitiveBoolean
            | ValueKind::PrimitiveNumeric
            | ValueKind::PrimitiveFloating
    ) {
        return require_unboxing(
            flow,
            operand,
            required,
            resolver,
            method_context,
            diagnostics,
        );
    }
    if !matches!(kind, ValueKind::Reference | ValueKind::Unknown) {
        return flow;
    }

    // A cast from Object/Number/a type variable to a primitive is a runtime
    // cast to the corresponding wrapper followed by unboxing. Null passes the
    // wrapper cast then throws NPE; a non-null incompatible object throws CCE.
    let mut flow = flow;
    let Some(mut normal_exit) = flow.normal_exit.take() else {
        return flow;
    };
    let resolved = check_receiver_dereference(
        operand,
        operand,
        flow.value,
        resolver,
        method_context,
        diagnostics,
    );
    if flow.value.may_be_null() {
        let mut null_exit = normal_exit.clone();
        if let Some(variable) = resolved {
            null_exit.set(variable, Nullability::Null);
        }
        flow.exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::NULL_POINTER,
            env: null_exit,
        });
    }
    if flow.value != Nullability::Null {
        let mut cast_exit = normal_exit.clone();
        if let Some(variable) = resolved {
            cast_exit.set(variable, Nullability::NonNull);
        }
        flow.exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::CLASS_CAST,
            env: cast_exit,
        });
    }
    if flow.value == Nullability::Null {
        return flow;
    }
    if let Some(variable) = resolved {
        normal_exit.set(variable, Nullability::NonNull);
    }
    flow.value = Nullability::NonNull;
    flow.normal_exit = Some(normal_exit);
    flow
}

fn expression_value_kind<R: VariableResolver>(
    expression: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> ValueKind {
    let member_offset = match expression.kind() {
        SyntaxKind::METHOD_CALL_EXPR => method_call_name_offset(expression),
        SyntaxKind::FIELD_ACCESS_EXPR => field_access_name_offset(expression),
        SyntaxKind::NAME_EXPR => Some(u32::from(expression.text_range().start())),
        _ => None,
    };
    if let Some(kind) = member_offset
        .and_then(|offset| method_context.member_uses.get(&offset))
        .map(|member| member.value_kind)
        .filter(|kind| *kind != ValueKind::Unknown)
    {
        return kind;
    }
    if let Some((variable, _)) = resolve_variable_from_expr(expression, resolver) {
        let kind = resolver.value_kind(variable);
        if kind != ValueKind::Unknown {
            return kind;
        }
    }
    match expression.kind() {
        SyntaxKind::LITERAL_EXPR => {
            let text = expression.text().to_string();
            let text = text.trim();
            if matches!(text, "true" | "false") {
                ValueKind::PrimitiveBoolean
            } else if text.starts_with('"') {
                ValueKind::StringReference
            } else if text == "null" {
                ValueKind::Reference
            } else if numeric_literal_is_floating(text) {
                ValueKind::PrimitiveFloating
            } else {
                ValueKind::PrimitiveNumeric
            }
        }
        SyntaxKind::PAREN_EXPR => expression
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
            .map_or(ValueKind::Unknown, |child| {
                expression_value_kind(&child, resolver, method_context)
            }),
        SyntaxKind::CAST_EXPR => declared_value_kind(expression),
        SyntaxKind::PATTERN_INSTANCEOF_EXPR | SyntaxKind::INSTANCEOF_EXPR => {
            ValueKind::PrimitiveBoolean
        }
        SyntaxKind::UNARY_EXPR => {
            if expression
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|token| token.kind() == SyntaxKind::BANG)
            {
                ValueKind::PrimitiveBoolean
            } else if expression
                .children()
                .filter(|child| is_expression(child.kind()))
                .last()
                .is_some_and(|operand| {
                    matches!(
                        expression_value_kind(&operand, resolver, method_context),
                        ValueKind::PrimitiveFloating | ValueKind::BoxedFloating
                    )
                })
            {
                ValueKind::PrimitiveFloating
            } else {
                ValueKind::PrimitiveNumeric
            }
        }
        SyntaxKind::BINARY_EXPR => {
            let operators: Vec<_> = expression
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .map(|token| token.kind())
                .collect();
            if operators.iter().any(|operator| {
                matches!(
                    operator,
                    SyntaxKind::EQ_EQ
                        | SyntaxKind::BANG_EQ
                        | SyntaxKind::LT
                        | SyntaxKind::GT
                        | SyntaxKind::LT_EQ
                        | SyntaxKind::GT_EQ
                        | SyntaxKind::AMP_AMP
                        | SyntaxKind::PIPE_PIPE
                )
            }) {
                ValueKind::PrimitiveBoolean
            } else if operators.contains(&SyntaxKind::PLUS) {
                let mut operands: Vec<_> = expression
                    .children()
                    .filter(|child| is_expression(child.kind()))
                    .collect();
                if operands.len() == 1 {
                    if let Some(left) = super::patterns::continuation_left_operand(expression) {
                        operands.push(left);
                    }
                }
                if operands.iter().any(|operand| {
                    expression_value_kind(operand, resolver, method_context)
                        == ValueKind::StringReference
                }) {
                    ValueKind::StringReference
                } else if operands.iter().any(|operand| {
                    matches!(
                        expression_value_kind(operand, resolver, method_context),
                        ValueKind::PrimitiveFloating | ValueKind::BoxedFloating
                    )
                }) {
                    ValueKind::PrimitiveFloating
                } else {
                    ValueKind::PrimitiveNumeric
                }
            } else if expressions_for_binary(expression).iter().any(|operand| {
                matches!(
                    expression_value_kind(operand, resolver, method_context),
                    ValueKind::PrimitiveFloating | ValueKind::BoxedFloating
                )
            }) {
                ValueKind::PrimitiveFloating
            } else {
                ValueKind::PrimitiveNumeric
            }
        }
        SyntaxKind::ASSIGNMENT_EXPR => assignment_target(expression)
            .map_or(ValueKind::Unknown, |value| {
                expression_value_kind(&value, resolver, method_context)
            }),
        SyntaxKind::TERNARY_EXPR => {
            let mut expressions = expression
                .children()
                .filter(|child| is_expression(child.kind()));
            let _condition = expressions.next();
            let Some(then_value) = expressions.next() else {
                return ValueKind::Unknown;
            };
            let Some(else_value) = expressions.next() else {
                return ValueKind::Unknown;
            };
            joined_value_kind(
                expression_value_kind(&then_value, resolver, method_context),
                expression_value_kind(&else_value, resolver, method_context),
            )
        }
        SyntaxKind::ARRAY_ACCESS_EXPR => expression
            .children()
            .find(|child| is_expression(child.kind()))
            .and_then(|receiver| {
                resolve_variable_from_expr(&receiver, resolver)
                    .and_then(|(variable, _)| resolver.declared_type_name(variable))
                    .and_then(|declared_type| declared_type.strip_suffix("[]"))
                    .map(value_kind_for_type_name)
            })
            .unwrap_or(ValueKind::Unknown),
        SyntaxKind::NEW_EXPR
        | SyntaxKind::ARRAY_INIT_EXPR
        | SyntaxKind::LAMBDA_EXPR
        | SyntaxKind::METHOD_REF_EXPR
        | SyntaxKind::CONSTRUCTOR_REF_EXPR
        | SyntaxKind::CLASS_LITERAL_EXPR => ValueKind::Reference,
        _ => ValueKind::Unknown,
    }
}

fn joined_value_kind(left: ValueKind, right: ValueKind) -> ValueKind {
    if left == right {
        return left;
    }
    if matches!(
        left,
        ValueKind::PrimitiveFloating | ValueKind::BoxedFloating
    ) && matches!(
        right,
        ValueKind::PrimitiveFloating
            | ValueKind::BoxedFloating
            | ValueKind::PrimitiveNumeric
            | ValueKind::BoxedNumeric
    ) || matches!(
        right,
        ValueKind::PrimitiveFloating | ValueKind::BoxedFloating
    ) && matches!(
        left,
        ValueKind::PrimitiveFloating
            | ValueKind::BoxedFloating
            | ValueKind::PrimitiveNumeric
            | ValueKind::BoxedNumeric
    ) {
        return ValueKind::PrimitiveFloating;
    }
    if matches!(left, ValueKind::PrimitiveNumeric | ValueKind::BoxedNumeric)
        && matches!(right, ValueKind::PrimitiveNumeric | ValueKind::BoxedNumeric)
    {
        return ValueKind::PrimitiveNumeric;
    }
    if matches!(left, ValueKind::PrimitiveBoolean | ValueKind::BoxedBoolean)
        && matches!(right, ValueKind::PrimitiveBoolean | ValueKind::BoxedBoolean)
    {
        return ValueKind::PrimitiveBoolean;
    }
    ValueKind::Unknown
}

fn conditional_conversion_requirement<R: VariableResolver>(
    then_value: &SyntaxNode,
    else_value: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> Option<RequiredPrimitive> {
    let left = expression_value_kind(then_value, resolver, method_context);
    let right = expression_value_kind(else_value, resolver, method_context);
    if matches!(left, ValueKind::PrimitiveBoolean) || matches!(right, ValueKind::PrimitiveBoolean) {
        return matches!(left, ValueKind::BoxedBoolean)
            .then_some(RequiredPrimitive::Boolean)
            .or_else(|| {
                matches!(right, ValueKind::BoxedBoolean).then_some(RequiredPrimitive::Boolean)
            });
    }
    if matches!(
        left,
        ValueKind::PrimitiveNumeric | ValueKind::PrimitiveFloating
    ) || matches!(
        right,
        ValueKind::PrimitiveNumeric | ValueKind::PrimitiveFloating
    ) {
        return matches!(left, ValueKind::BoxedNumeric | ValueKind::BoxedFloating)
            .then_some(RequiredPrimitive::Numeric)
            .or_else(|| {
                matches!(right, ValueKind::BoxedNumeric | ValueKind::BoxedFloating)
                    .then_some(RequiredPrimitive::Numeric)
            });
    }
    None
}

fn numeric_literal_is_floating(text: &str) -> bool {
    let unsigned = text.trim_start_matches(['+', '-']);
    if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        return unsigned.contains('.') || unsigned.contains('p') || unsigned.contains('P');
    }
    text.ends_with(['f', 'F', 'd', 'D'])
        || text.contains('.')
        || text.contains('e')
        || text.contains('E')
}

fn integral_literal_is_definitely_nonzero(expression: &SyntaxNode) -> bool {
    if expression.kind() != SyntaxKind::LITERAL_EXPR {
        return false;
    }
    let text = expression.text().to_string();
    let text = text.trim().replace('_', "");
    if numeric_literal_is_floating(&text) {
        return false;
    }
    let text = text.trim_end_matches(['l', 'L']);
    let parsed = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u128::from_str_radix(hex, 16).ok()
    } else if let Some(binary) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
        u128::from_str_radix(binary, 2).ok()
    } else if text.len() > 1 && text.starts_with('0') {
        u128::from_str_radix(&text[1..], 8).ok()
    } else {
        text.parse::<u128>().ok()
    };
    parsed.is_some_and(|value| value != 0)
}

fn expressions_for_binary(binary: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut operands: Vec<_> = binary
        .children()
        .filter(|child| is_expression(child.kind()))
        .collect();
    if operands.len() == 1 {
        if let Some(left) = super::patterns::continuation_left_operand(binary) {
            operands.push(left);
        }
    }
    operands
}

fn division_exception_kind<R: VariableResolver>(
    operands: &[SyntaxNode],
    resolver: &R,
    method_context: &MethodContext,
) -> Option<ExceptionKind> {
    let kinds: Vec<_> = operands
        .iter()
        .map(|operand| expression_value_kind(operand, resolver, method_context))
        .collect();
    if kinds.iter().any(|kind| {
        matches!(
            kind,
            ValueKind::PrimitiveFloating | ValueKind::BoxedFloating
        )
    }) {
        return None;
    }
    if kinds
        .iter()
        .all(|kind| matches!(kind, ValueKind::PrimitiveNumeric | ValueKind::BoxedNumeric))
    {
        Some(ExceptionKind::ARITHMETIC)
    } else {
        // The Java type is not available to this local evaluator. Preserve a
        // conservative exceptional lane without claiming it is definitely an
        // integral divide-by-zero edge.
        Some(ExceptionKind::Unknown)
    }
}

fn cast_reference_type(cast: &SyntaxNode) -> Option<String> {
    let operand_start = cast
        .children()
        .filter(|child| is_expression(child.kind()))
        .last()
        .map(|operand| operand.text_range().start())?;
    cast.descendants()
        .filter(|node| node.kind() == SyntaxKind::CLASS_TYPE)
        .find(|ty| ty.text_range().end() <= operand_start)
        .map(|ty| ty.text().to_string().trim().to_string())
}

fn new_reference_type(expression: &SyntaxNode) -> Option<String> {
    (expression.kind() == SyntaxKind::NEW_EXPR)
        .then(|| {
            expression
                .descendants()
                .find(|node| node.kind() == SyntaxKind::CLASS_TYPE)
                .map(|ty| ty.text().to_string().trim().to_string())
        })
        .flatten()
}

fn reference_cast_may_fail<R: VariableResolver>(
    cast: &SyntaxNode,
    operand: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> bool {
    let Some(target) = cast_reference_type(cast) else {
        return true;
    };
    if method_context
        .exception_types
        .is_unshadowed_java_lang_type(cast, &target, "Object")
    {
        return false;
    }
    let resolved_target = method_context
        .exception_types
        .resolve_type_at(cast, &target)
        .unwrap_or(target);
    if resolve_variable_from_expr(operand, resolver)
        .and_then(|(variable, _)| resolver.declared_type_name(variable))
        .and_then(|declared_type| {
            method_context
                .exception_types
                .resolve_type_at(operand, declared_type)
                .or_else(|| Some(declared_type.to_owned()))
        })
        .is_some_and(|declared_type| declared_type == resolved_target)
    {
        return false;
    }
    if new_reference_type(operand).is_some_and(|operand_type| {
        method_context
            .exception_types
            .resolve_type_at(operand, &operand_type)
            .unwrap_or(operand_type)
            == resolved_target
    }) {
        return false;
    }
    if operand.kind() == SyntaxKind::CAST_EXPR
        && cast_reference_type(operand).is_some_and(|ty| {
            method_context
                .exception_types
                .resolve_type_at(operand, &ty)
                .unwrap_or(ty)
                == resolved_target
        })
    {
        return false;
    }
    true
}

fn thrown_exception_kind<R: VariableResolver>(
    expression: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> ExceptionKind {
    let mut value = expression.clone();
    while value.kind() == SyntaxKind::PAREN_EXPR {
        let Some(inner) = value
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
        else {
            return ExceptionKind::Unknown;
        };
        value = inner;
    }
    if value.kind() == SyntaxKind::CAST_EXPR {
        return cast_reference_type(&value).map_or(ExceptionKind::Unknown, |name| {
            let name = method_context
                .exception_types
                .resolve_type_at(&value, &name)
                .unwrap_or(name);
            ExceptionKind::SubtypeOf(Cow::Owned(name))
        });
    }
    if let Some(name) = super::cfg::explicit_thrown_type(&value) {
        let name = method_context
            .exception_types
            .resolve_type_at(&value, &name)
            .unwrap_or(name);
        return ExceptionKind::Known(Cow::Owned(name));
    }
    resolve_variable_from_expr(&value, resolver)
        .and_then(|(variable, _)| resolver.declared_type_name(variable))
        .map_or(ExceptionKind::Unknown, |name| {
            let name = method_context
                .exception_types
                .resolve_type_at(&value, name)
                .unwrap_or_else(|| name.to_owned());
            ExceptionKind::SubtypeOf(Cow::Owned(name))
        })
}

fn required_primitive_for_binary<R: VariableResolver>(
    binary: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> Option<RequiredPrimitive> {
    if binary.kind() != SyntaxKind::BINARY_EXPR {
        return None;
    }
    let operator = binary
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .map(|token| token.kind())
        .find(|kind| {
            matches!(
                kind,
                SyntaxKind::AMP_AMP
                    | SyntaxKind::PIPE_PIPE
                    | SyntaxKind::AMP
                    | SyntaxKind::PIPE
                    | SyntaxKind::CARET
                    | SyntaxKind::PLUS
                    | SyntaxKind::MINUS
                    | SyntaxKind::STAR
                    | SyntaxKind::SLASH
                    | SyntaxKind::PERCENT
                    | SyntaxKind::LT
                    | SyntaxKind::GT
                    | SyntaxKind::LT_EQ
                    | SyntaxKind::GT_EQ
                    | SyntaxKind::EQ_EQ
                    | SyntaxKind::BANG_EQ
                    | SyntaxKind::SHL
                    | SyntaxKind::SHR
                    | SyntaxKind::USHR
            )
        })?;
    if matches!(operator, SyntaxKind::AMP_AMP | SyntaxKind::PIPE_PIPE) {
        return Some(RequiredPrimitive::Boolean);
    }
    let operands = expressions_for_binary(binary);
    let kinds: Vec<_> = operands
        .iter()
        .map(|operand| expression_value_kind(operand, resolver, method_context))
        .collect();
    if matches!(operator, SyntaxKind::EQ_EQ | SyntaxKind::BANG_EQ) {
        // Wrapper-wrapper equality is reference identity. A primitive operand
        // instead selects binary numeric promotion or boolean unboxing.
        if kinds.contains(&ValueKind::PrimitiveBoolean) {
            return Some(RequiredPrimitive::Boolean);
        }
        if kinds.iter().any(|kind| {
            matches!(
                kind,
                ValueKind::PrimitiveNumeric | ValueKind::PrimitiveFloating
            )
        }) {
            return Some(RequiredPrimitive::Numeric);
        }
        return None;
    }
    if matches!(
        operator,
        SyntaxKind::AMP | SyntaxKind::PIPE | SyntaxKind::CARET
    ) {
        return if kinds
            .iter()
            .any(|kind| matches!(kind, ValueKind::PrimitiveBoolean | ValueKind::BoxedBoolean))
        {
            Some(RequiredPrimitive::Boolean)
        } else {
            Some(RequiredPrimitive::Numeric)
        };
    }
    if operator == SyntaxKind::PLUS
        && operands.iter().any(|operand| {
            expression_value_kind(operand, resolver, method_context) == ValueKind::StringReference
        })
    {
        return None;
    }
    Some(RequiredPrimitive::Numeric)
}

fn required_primitive_for_continuation<R: VariableResolver>(
    expression: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> Option<RequiredPrimitive> {
    match expression.kind() {
        SyntaxKind::BINARY_EXPR => {
            required_primitive_for_binary(expression, resolver, method_context)
        }
        SyntaxKind::UNARY_EXPR => unary_required_primitive(expression),
        _ => None,
    }
}

fn unary_required_primitive(expression: &SyntaxNode) -> Option<RequiredPrimitive> {
    expression
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .map(|token| token.kind())
        .find_map(|operator| match operator {
            SyntaxKind::BANG => Some(RequiredPrimitive::Boolean),
            SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::TILDE
            | SyntaxKind::PLUS_PLUS
            | SyntaxKind::MINUS_MINUS => Some(RequiredPrimitive::Numeric),
            _ => None,
        })
}

fn unary_mutates_operand(expression: &SyntaxNode) -> bool {
    expression
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .any(|token| {
            matches!(
                token.kind(),
                SyntaxKind::PLUS_PLUS | SyntaxKind::MINUS_MINUS
            )
        })
}

fn apply_unary_write<R: VariableResolver>(
    flow: &mut ExpressionFlow,
    operand: &SyntaxNode,
    resolver: &R,
) {
    let Some(exit) = flow.normal_exit.as_mut() else {
        return;
    };
    let Some((variable, _)) = resolve_variable_from_expr(operand, resolver) else {
        return;
    };
    apply_assignment_value_effect(variable, exit, Nullability::NonNull, resolver);
    if !flow.writes.contains(&variable) {
        flow.writes.push(variable);
    }
}

fn evaluate_unary_expression<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    prior_boolean_value: Option<bool>,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let operand = node.children().find(|child| is_expression(child.kind()));
    let Some(operand) = operand else {
        return ExpressionFlow {
            value: Nullability::NonNull,
            boolean_value: node
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|token| token.kind() == SyntaxKind::BANG)
                .then(|| prior_boolean_value.map(|value| !value))
                .flatten(),
            normal_exit: Some(env.clone()),
            exceptional_exit: ExceptionalExits::new(),
            writes: SmallVec::new(),
        };
    };
    let mut flow = evaluate_expression_chain(
        &operand,
        env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    if let Some(required) = unary_required_primitive(node) {
        flow = require_unboxing(
            flow,
            &operand,
            required,
            resolver,
            method_context,
            diagnostics,
        );
    }
    if unary_mutates_operand(node) {
        apply_unary_write(&mut flow, &operand, resolver);
    }
    let negated = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .any(|token| token.kind() == SyntaxKind::BANG);
    flow.boolean_value = negated
        .then(|| flow.boolean_value.map(|value| !value))
        .flatten();
    flow.value = Nullability::NonNull;
    flow
}

fn evaluate_expression_children<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let mut effective_env = env.clone();
    let mut writes = SmallVec::new();
    let mut exceptional_exit = ExceptionalExits::new();
    for child in node.children().filter(|child| is_expression(child.kind())) {
        let flow = evaluate_expression_chain(
            &child,
            &effective_env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        effective_env = match absorb_child_flow(flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(_, _, exit) => exit,
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    }
    ExpressionFlow {
        value: expr_nullability(node, &effective_env, resolver, method_context),
        boolean_value: constant_boolean_value(node),
        normal_exit: Some(effective_env),
        exceptional_exit,
        writes,
    }
}

struct DelayedAssignmentDereference {
    receiver: SyntaxNode,
    value: Nullability,
    captured_place: Option<CapturedPlace>,
    writes_after_capture: SmallVec<[VariableId; 2]>,
    may_throw_after_nonnull_check: bool,
}

fn evaluate_assignment_expression<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let Some(target) = assignment_target(node) else {
        return normal_expression(Nullability::Unknown, env);
    };
    let Some(value_node) = find_assignment_value_expr(node) else {
        return normal_expression(Nullability::Unknown, env);
    };
    let target_place = capture_stable_place(&target, resolver);
    let operator = assignment_operator(node);
    let is_simple = operator == Some(SyntaxKind::EQ);
    let target_shape = transparent_lvalue(&target);
    let static_field_store_effect = (is_simple
        && target_shape.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
        .then(|| {
            field_access_name_offset(&target_shape)
                .and_then(|offset| method_context.member_uses.get(&offset))
                .filter(|member| member.dispatch() == DispatchKind::Static)
                .map(|member| member.effect())
        })
        .flatten()
        .filter(|effect| *effect != InvocationEffect::NonThrowing);
    let conversion = if is_simple {
        primitive_requirement_for_kind(expression_value_kind(&target, resolver, method_context))
    } else {
        compound_assignment_requirement(operator, &target, resolver, method_context)
    };

    let mut effective_env = env.clone();
    let mut exceptional_exit = ExceptionalExits::new();
    let mut writes = SmallVec::new();
    let mut delayed_dereference = None;

    if is_simple {
        match target_shape.kind() {
            SyntaxKind::FIELD_ACCESS_EXPR => {
                if let Some(receiver) = field_access_receiver(&target_shape) {
                    let action = field_qualifier_action(&target_shape, method_context);
                    if action != QualifierAction::SkipTypeName {
                        let receiver_place = capture_stable_place(&receiver, resolver);
                        let receiver_flow = evaluate_expression_chain(
                            &receiver,
                            &effective_env,
                            resolver,
                            method_context,
                            diagnostics,
                            call_sites,
                        );
                        let (value, _, exit) = match absorb_child_flow(
                            receiver_flow,
                            &mut exceptional_exit,
                            &mut writes,
                        ) {
                            AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
                            AbsorbedChild::Abrupt(abrupt) => return abrupt,
                        };
                        effective_env = exit;
                        if action == QualifierAction::Dereference {
                            delayed_dereference = Some(DelayedAssignmentDereference {
                                receiver,
                                value,
                                captured_place: receiver_place,
                                writes_after_capture: SmallVec::new(),
                                may_throw_after_nonnull_check: false,
                            });
                        }
                    }
                }
            }
            SyntaxKind::ARRAY_ACCESS_EXPR => {
                let mut operands = target_shape
                    .children()
                    .filter(|child| is_expression(child.kind()));
                if let Some(receiver) = operands.next() {
                    let receiver_place = capture_stable_place(&receiver, resolver);
                    let receiver_flow = evaluate_expression_chain(
                        &receiver,
                        &effective_env,
                        resolver,
                        method_context,
                        diagnostics,
                        call_sites,
                    );
                    let (value, _, exit) = match absorb_child_flow(
                        receiver_flow,
                        &mut exceptional_exit,
                        &mut writes,
                    ) {
                        AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
                        AbsorbedChild::Abrupt(abrupt) => return abrupt,
                    };
                    effective_env = exit;
                    let mut index_writes = SmallVec::new();
                    for index in operands {
                        let index_flow = evaluate_expression_chain(
                            &index,
                            &effective_env,
                            resolver,
                            method_context,
                            diagnostics,
                            call_sites,
                        );
                        let index_flow = require_unboxing(
                            index_flow,
                            &index,
                            RequiredPrimitive::Numeric,
                            resolver,
                            method_context,
                            diagnostics,
                        );
                        for variable in &index_flow.writes {
                            if !index_writes.contains(variable) {
                                index_writes.push(*variable);
                            }
                        }
                        effective_env =
                            match absorb_child_flow(index_flow, &mut exceptional_exit, &mut writes)
                            {
                                AbsorbedChild::Normal(_, _, exit) => exit,
                                AbsorbedChild::Abrupt(abrupt) => return abrupt,
                            };
                    }
                    delayed_dereference = Some(DelayedAssignmentDereference {
                        receiver,
                        value,
                        captured_place: receiver_place,
                        writes_after_capture: index_writes,
                        may_throw_after_nonnull_check: true,
                    });
                }
            }
            _ => {}
        }
    } else {
        // Compound assignment reads the lvalue before evaluating the RHS. For
        // fields and arrays this performs the null check immediately; the
        // selected value is then saved for the compound operation.
        let target_flow = evaluate_expression_chain(
            &target,
            &effective_env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        let target_flow = if let Some(required) = conversion {
            require_unboxing(
                target_flow,
                &target,
                required,
                resolver,
                method_context,
                diagnostics,
            )
        } else {
            target_flow
        };
        effective_env = match absorb_child_flow(target_flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(_, _, exit) => exit,
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    }

    let value_flow = evaluate_expression_chain(
        &value_node,
        &effective_env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let value_flow = if let Some(required) = conversion {
        require_unboxing(
            value_flow,
            &value_node,
            required,
            resolver,
            method_context,
            diagnostics,
        )
    } else {
        value_flow
    };
    let rhs_writes = value_flow.writes.clone();
    let (rhs_value, rhs_boolean, mut exit) =
        match absorb_child_flow(value_flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };

    if let Some(mut delayed) = delayed_dereference {
        delayed
            .writes_after_capture
            .extend(rhs_writes.iter().copied());
        let resolved = check_receiver_dereference(
            &delayed.receiver,
            &target_shape,
            delayed.value,
            resolver,
            method_context,
            diagnostics,
        );
        let stable_source = match (resolved, delayed.captured_place.as_ref()) {
            (Some(variable), Some(captured_place))
                if !write_invalidates_captured_place(
                    &delayed.writes_after_capture,
                    captured_place,
                    true,
                    resolver,
                ) =>
            {
                Some(variable)
            }
            _ => None,
        };
        if delayed.value.may_be_null() {
            let mut exceptional = exit.clone();
            if let Some(variable) = stable_source {
                exceptional.set(variable, Nullability::Null);
            }
            exceptional_exit.push(ExceptionalExit {
                kind: ExceptionKind::NULL_POINTER,
                env: exceptional,
            });
        }
        if delayed.value == Nullability::Null {
            return ExpressionFlow {
                value: Nullability::Unknown,
                boolean_value: None,
                normal_exit: None,
                exceptional_exit,
                writes,
            };
        }
        if let Some(variable) = stable_source {
            exit.set(variable, Nullability::NonNull);
        }
        if delayed.may_throw_after_nonnull_check {
            // Bounds and reference-array store checks occur only after the
            // delayed null check has completed normally. Keep their concrete
            // unchecked identities: an `Unknown` lane would make unrelated
            // catches such as NullPointerException spuriously reachable.
            exceptional_exit.push(ExceptionalExit {
                kind: ExceptionKind::Known(Cow::Borrowed(
                    "java.lang.ArrayIndexOutOfBoundsException",
                )),
                env: exit.clone(),
            });
            let declared_array_type = resolve_variable_from_expr(&delayed.receiver, resolver)
                .and_then(|(variable, _)| resolver.declared_type_name(variable));
            let definitely_primitive_array = declared_array_type.is_some_and(|declared_type| {
                let declared_type = declared_type.trim().trim_end_matches("[]");
                matches!(
                    declared_type,
                    "boolean" | "byte" | "short" | "int" | "long" | "char" | "float" | "double"
                )
            });
            if !definitely_primitive_array {
                exceptional_exit.push(ExceptionalExit {
                    kind: ExceptionKind::Known(Cow::Borrowed("java.lang.ArrayStoreException")),
                    env: exit.clone(),
                });
            }
        }
    }

    // A simple putstatic becomes an active use only after its RHS has been
    // evaluated. Preserve that ordering so the exceptional lane observes RHS
    // writes but skips every later operation.
    if let Some(effect) = static_field_store_effect {
        exceptional_exit.push(ExceptionalExit {
            kind: if effect == InvocationEffect::ClassInitialization {
                ExceptionKind::SubtypeOf(Cow::Borrowed("java.lang.Error"))
            } else {
                ExceptionKind::Unknown
            },
            env: exit.clone(),
        });
    }

    if matches!(
        operator,
        Some(SyntaxKind::SLASH_EQ | SyntaxKind::PERCENT_EQ)
    ) && !integral_literal_is_definitely_nonzero(&value_node)
    {
        if let Some(kind) = division_exception_kind(
            &[target.clone(), value_node.clone()],
            resolver,
            method_context,
        ) {
            exceptional_exit.push(ExceptionalExit {
                kind,
                env: exit.clone(),
            });
        }
    }

    let stored_value = if is_simple {
        rhs_value
    } else {
        // Every legal compound assignment produces a primitive value or a
        // freshly produced String/boxed primitive on normal completion; it is
        // never the nullable RHS reference itself.
        Nullability::NonNull
    };
    if let Some(target) = target_place {
        if !write_invalidates_captured_place(&writes, &target, false, resolver) {
            apply_assignment_value_effect(target.variable, &mut exit, stored_value, resolver);
            if !writes.contains(&target.variable) {
                writes.push(target.variable);
            }
        }
    }
    ExpressionFlow {
        value: stored_value,
        boolean_value: is_simple.then_some(rhs_boolean).flatten(),
        normal_exit: Some(exit),
        exceptional_exit,
        writes,
    }
}

fn evaluate_method_call<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let mut effective_env = env.clone();
    let mut writes = SmallVec::new();
    let mut exceptional_exit = ExceptionalExits::new();
    let evaluated_receiver = if let Some(receiver) = method_call_receiver(node) {
        let action = method_qualifier_action(&receiver, node, resolver, method_context);
        if action == QualifierAction::SkipTypeName {
            None
        } else {
            let receiver_flow = evaluate_expression_chain(
                &receiver,
                &effective_env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            let (receiver_value, _, exit) =
                match absorb_child_flow(receiver_flow, &mut exceptional_exit, &mut writes) {
                    AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
                    AbsorbedChild::Abrupt(abrupt) => return abrupt,
                };
            effective_env = exit;
            (action == QualifierAction::Dereference).then(|| {
                let captured_place = capture_stable_place(&receiver, resolver);
                (receiver, receiver_value, captured_place)
            })
        }
    } else {
        None
    };

    let mut evaluated_arguments = Vec::new();
    let mut argument_writes: SmallVec<[VariableId; 2]> = SmallVec::new();
    let mut writes_after_first_argument: SmallVec<[VariableId; 2]> = SmallVec::new();
    let mut first_argument_place = None;
    let mut first_argument_boolean = None;
    let mut first_argument_exit = None;
    let argument_conversion_plan = method_call_name_offset(node)
        .and_then(|offset| method_context.member_uses.get(&offset))
        .and_then(|member| member.argument_plan.checked_sub(1))
        .and_then(|plan| method_context.argument_conversions.get(usize::from(plan)));
    for (argument_index, argument) in method_call_arguments(node).into_iter().enumerate() {
        if argument_index == 0 {
            first_argument_place = capture_stable_place(&argument, resolver);
        }
        let argument_flow = evaluate_expression_chain(
            &argument,
            &effective_env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        let argument_requirement = argument_conversion_plan
            .and_then(|plan| plan.get(argument_index))
            .and_then(|kind| primitive_requirement_for_kind(*kind));
        let argument_flow = if let Some(required) = argument_requirement {
            require_unboxing(
                argument_flow,
                &argument,
                required,
                resolver,
                method_context,
                diagnostics,
            )
        } else {
            argument_flow
        };
        for variable in &argument_flow.writes {
            if !argument_writes.contains(variable) {
                argument_writes.push(*variable);
            }
            if argument_index > 0 && !writes_after_first_argument.contains(variable) {
                writes_after_first_argument.push(*variable);
            }
        }
        let (argument_value, argument_boolean, exit) =
            match absorb_child_flow(argument_flow, &mut exceptional_exit, &mut writes) {
                AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
                AbsorbedChild::Abrupt(abrupt) => return abrupt,
            };
        if argument_index == 0 {
            first_argument_boolean = argument_boolean;
            first_argument_exit = Some(exit.clone());
        }
        evaluated_arguments.push((argument.clone(), argument_value));
        effective_env = exit;
    }
    // Invocation captures the target first, evaluates every argument, and only
    // then performs the null target check. The successful check is evidence
    // about the source path only when no argument replaced a binding used to
    // compute that captured place.
    if let Some((receiver, captured_value, captured_place)) = evaluated_receiver {
        let resolved = check_receiver_dereference(
            &receiver,
            node,
            captured_value,
            resolver,
            method_context,
            diagnostics,
        );
        let stable_source = match (resolved, captured_place.as_ref()) {
            (Some(variable), Some(captured_place))
                if !write_invalidates_captured_place(
                    &argument_writes,
                    captured_place,
                    true,
                    resolver,
                ) =>
            {
                Some(variable)
            }
            _ => None,
        };
        if captured_value.may_be_null() {
            let mut exceptional = effective_env.clone();
            if let Some(variable) = stable_source {
                exceptional.set(variable, Nullability::Null);
            }
            exceptional_exit.push(ExceptionalExit {
                kind: ExceptionKind::NULL_POINTER,
                env: exceptional,
            });
        }
        if captured_value == Nullability::Null {
            return ExpressionFlow {
                value: Nullability::Unknown,
                boolean_value: None,
                normal_exit: None,
                exceptional_exit,
                writes,
            };
        }
        if let Some(variable) = stable_source {
            effective_env.set(variable, Nullability::NonNull);
        }
    }

    let invoked_supplier_return = if method_call_info(node).0.as_deref() == Some("orElseGet")
        && is_optional_call(node, method_context)
    {
        evaluated_arguments
            .first()
            .filter(|(argument, _)| argument.kind() == SyntaxKind::LAMBDA_EXPR)
            .map(|(lambda, _)| {
                let nested = analyze_invoked_lambda(
                    lambda,
                    &effective_env,
                    resolver,
                    method_context,
                    diagnostics,
                    call_sites,
                );
                exceptional_exit.extend(nested.exceptional_exit);
                if let Some(normal_exit) = nested.normal_exit {
                    effective_env = effective_env.join(&normal_exit);
                    for variable in nested.writes {
                        if !writes.contains(&variable) {
                            writes.push(variable);
                        }
                    }
                    InvokedSupplierReturn::Returned(
                        nested.return_value.unwrap_or(Nullability::Unknown),
                    )
                } else {
                    debug_assert!(nested.return_value.is_none());
                    InvokedSupplierReturn::Abrupt
                }
            })
    } else {
        None
    };

    let enforces_nonnull = is_nonnull_enforcing_call(node, method_context);
    let condition_exception = condition_enforcing_exception(node, method_context);
    let invocation_effect = method_call_name_offset(node)
        .and_then(|offset| method_context.member_uses.get(&offset))
        .map_or(InvocationEffect::Arbitrary, |member| member.effect());
    if !enforces_nonnull
        && condition_exception.is_none()
        && invocation_effect != InvocationEffect::NonThrowing
    {
        // Once receiver and arguments have completed, an invoked method may
        // itself complete abruptly. This is the exact state its handler sees;
        // do not reconstruct it later from block endpoints.
        exceptional_exit.push(ExceptionalExit {
            kind: if invocation_effect == InvocationEffect::ClassInitialization {
                ExceptionKind::SubtypeOf(Cow::Borrowed("java.lang.Error"))
            } else {
                ExceptionKind::Unknown
            },
            env: effective_env.clone(),
        });
    }
    if enforces_nonnull {
        if let Some((argument, value)) = evaluated_arguments.first() {
            if value.may_be_null() {
                exceptional_exit.push(ExceptionalExit {
                    kind: ExceptionKind::NULL_POINTER,
                    env: effective_env.clone(),
                });
            }
            if *value == Nullability::Null {
                return ExpressionFlow {
                    value: Nullability::Unknown,
                    boolean_value: None,
                    normal_exit: None,
                    exceptional_exit,
                    writes,
                };
            }
            if let Some((variable, _)) = resolve_variable_from_expr(argument, resolver) {
                let source_still_denotes_checked_value =
                    first_argument_place.as_ref().is_none_or(|place| {
                        !write_invalidates_captured_place(
                            &writes_after_first_argument,
                            place,
                            true,
                            resolver,
                        )
                    });
                if source_still_denotes_checked_value {
                    effective_env.set(variable, Nullability::NonNull);
                }
            }
        }
    } else if let Some(condition_exception) = condition_exception {
        let condition = evaluated_arguments.first().and_then(|(argument, _)| {
            analyze_condition_in_context(&ExprRef::new(argument.clone()), resolver, method_context)
        });
        let captured_env = first_argument_exit.as_ref().unwrap_or(&effective_env);
        let condition_truth = first_argument_boolean.or_else(|| {
            condition
                .as_ref()
                .and_then(|condition| known_condition_truth(condition, captured_env))
        });
        if condition_truth != Some(true) {
            let exception_env = condition.as_ref().map_or_else(
                || effective_env.clone(),
                |condition| {
                    apply_captured_condition_narrowing(
                        &effective_env,
                        captured_env,
                        condition,
                        false,
                        &writes_after_first_argument,
                        resolver,
                    )
                },
            );
            exceptional_exit.push(ExceptionalExit {
                kind: condition_exception,
                env: exception_env,
            });
        }
        if condition_truth == Some(false) {
            return ExpressionFlow {
                value: Nullability::Unknown,
                boolean_value: None,
                normal_exit: None,
                exceptional_exit,
                writes,
            };
        }
        if let Some(condition) = &condition {
            effective_env = apply_captured_condition_narrowing(
                &effective_env,
                captured_env,
                condition,
                true,
                &writes_after_first_argument,
                resolver,
            );
        }
    }
    check_call_argument_values(node, &evaluated_arguments, method_context, diagnostics);
    call_sites.record(node, &evaluated_arguments);
    ExpressionFlow {
        value: if enforces_nonnull {
            Nullability::NonNull
        } else {
            evaluated_method_call_nullability(
                node,
                &evaluated_arguments,
                &effective_env,
                resolver,
                method_context,
                invoked_supplier_return,
            )
        },
        boolean_value: None,
        normal_exit: Some(effective_env),
        exceptional_exit,
        writes,
    }
}

fn evaluate_new_expression<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let mut effective_env = env.clone();
    let mut writes = SmallVec::new();
    let mut exceptional_exit = ExceptionalExits::new();
    let qualifier = node
        .children()
        .take_while(|child| {
            !matches!(
                child.kind(),
                SyntaxKind::CLASS_TYPE
                    | SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::ARRAY_TYPE
                    | SyntaxKind::VAR_TYPE
            )
        })
        .find(|child| is_expression(child.kind()));

    if let Some(qualifier) = qualifier.as_ref() {
        let qualifier_flow = evaluate_expression_chain(
            qualifier,
            &effective_env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        let (qualifier_value, _, exit) =
            match absorb_child_flow(qualifier_flow, &mut exceptional_exit, &mut writes) {
                AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
                AbsorbedChild::Abrupt(abrupt) => return abrupt,
            };
        effective_env = exit;
        // A qualified class-instance creation checks its enclosing instance
        // before allocation and before any constructor argument is evaluated
        // (JLS 15.9.4).
        let resolved = check_receiver_dereference(
            qualifier,
            node,
            qualifier_value,
            resolver,
            method_context,
            diagnostics,
        );
        if qualifier_value.may_be_null() {
            let mut exceptional = effective_env.clone();
            if let Some(variable) = resolved {
                exceptional.set(variable, Nullability::Null);
            }
            exceptional_exit.push(ExceptionalExit {
                kind: ExceptionKind::NULL_POINTER,
                env: exceptional,
            });
        }
        if qualifier_value == Nullability::Null {
            return ExpressionFlow {
                value: Nullability::Unknown,
                boolean_value: None,
                normal_exit: None,
                exceptional_exit,
                writes,
            };
        }
        if let Some(variable) = resolved {
            effective_env.set(variable, Nullability::NonNull);
        }
    }

    let arguments = method_call_arguments(node);
    let invokes_constructor = node
        .children()
        .any(|child| child.kind() == SyntaxKind::ARG_LIST);
    let has_explicit_array_dimension = !invokes_constructor
        && node.children().any(|child| {
            is_expression(child.kind())
                && child.kind() != SyntaxKind::ARRAY_INIT_EXPR
                && qualifier
                    .as_ref()
                    .is_none_or(|qualifier| &child != qualifier)
        });
    let array_element_requirement = (!invokes_constructor)
        .then(|| {
            node.children()
                .find(|child| child.kind() == SyntaxKind::PRIMITIVE_TYPE)
                .map(|primitive| {
                    if primitive.text().to_string().trim() == "boolean" {
                        RequiredPrimitive::Boolean
                    } else {
                        RequiredPrimitive::Numeric
                    }
                })
        })
        .flatten();
    let operands: Vec<_> = if arguments.is_empty() {
        node.children()
            .filter(|child| {
                is_expression(child.kind())
                    && qualifier
                        .as_ref()
                        .is_none_or(|qualifier| child != qualifier)
            })
            .collect()
    } else {
        arguments
    };
    for operand in operands {
        let flow = if operand.kind() == SyntaxKind::ARRAY_INIT_EXPR {
            evaluate_array_initializer(
                &operand,
                &effective_env,
                array_element_requirement,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            )
        } else {
            let flow = evaluate_expression_chain(
                &operand,
                &effective_env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            if invokes_constructor {
                flow
            } else {
                require_unboxing(
                    flow,
                    &operand,
                    RequiredPrimitive::Numeric,
                    resolver,
                    method_context,
                    diagnostics,
                )
            }
        };
        effective_env = match absorb_child_flow(flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(_, _, exit) => exit,
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    }
    if invokes_constructor && constructor_invocation_may_throw(node, method_context) {
        // Constructor selection/allocation happens only after the enclosing
        // instance and every argument completed. Until resolved `throws`
        // facts are available, retain one conservative invocation lane just
        // as ordinary method calls do.
        exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::Unknown,
            env: effective_env.clone(),
        });
    } else if has_explicit_array_dimension {
        // Each dimension expression is evaluated before Java rejects a
        // negative size. Initializer-only forms (`new int[]{...}`) have no
        // such completion and must not acquire this lane.
        exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::Known(Cow::Borrowed("java.lang.NegativeArraySizeException")),
            env: effective_env.clone(),
        });
    }
    ExpressionFlow {
        value: Nullability::NonNull,
        boolean_value: None,
        normal_exit: Some(effective_env),
        exceptional_exit,
        writes,
    }
}

fn evaluate_array_initializer<R: VariableResolver>(
    initializer: &SyntaxNode,
    env: &NullabilityEnv,
    element_requirement: Option<RequiredPrimitive>,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let mut effective_env = env.clone();
    let mut exceptional_exit = ExceptionalExits::new();
    let mut writes = SmallVec::new();
    for element in initializer
        .children()
        .filter(|child| is_expression(child.kind()))
    {
        let flow = if element.kind() == SyntaxKind::ARRAY_INIT_EXPR {
            evaluate_array_initializer(
                &element,
                &effective_env,
                element_requirement,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            )
        } else {
            let flow = evaluate_expression_chain(
                &element,
                &effective_env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            if let Some(required) = element_requirement {
                require_unboxing(
                    flow,
                    &element,
                    required,
                    resolver,
                    method_context,
                    diagnostics,
                )
            } else {
                flow
            }
        };
        effective_env = match absorb_child_flow(flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(_, _, exit) => exit,
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    }
    ExpressionFlow {
        value: Nullability::NonNull,
        boolean_value: None,
        normal_exit: Some(effective_env),
        exceptional_exit,
        writes,
    }
}

fn constructor_invocation_may_throw(node: &SyntaxNode, method_context: &MethodContext) -> bool {
    let Some(constructed_type) = new_reference_type(node) else {
        return true;
    };
    if let Some(may_throw) = method_context
        .exception_types
        .source_constructor_may_throw(node, &constructed_type)
    {
        return may_throw;
    }
    if !method_call_arguments(node).is_empty() {
        return true;
    }
    super::cfg::IMPLICIT_JAVA_LANG_TYPE_NAMES
        .iter()
        .copied()
        .all(|simple| {
            !method_context.exception_types.is_unshadowed_java_lang_type(
                node,
                &constructed_type,
                simple,
            )
        })
}

fn evaluate_field_access<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let Some(receiver) = field_access_receiver(node) else {
        return normal_expression(expr_nullability(node, env, resolver, method_context), env);
    };
    let action = field_qualifier_action(node, method_context);
    if action == QualifierAction::SkipTypeName {
        let mut flow =
            normal_expression(expr_nullability(node, env, resolver, method_context), env);
        add_static_field_initialization_exit(node, method_context, env, &mut flow.exceptional_exit);
        return flow;
    }
    let mut writes = SmallVec::new();
    let mut exceptional_exit = ExceptionalExits::new();
    let receiver_flow = evaluate_expression_chain(
        &receiver,
        env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let (receiver_value, _, mut exit) =
        match absorb_child_flow(receiver_flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    if action == QualifierAction::Dereference {
        let resolved = check_receiver_dereference(
            &receiver,
            node,
            receiver_value,
            resolver,
            method_context,
            diagnostics,
        );
        if receiver_value.may_be_null() {
            let mut exceptional = exit.clone();
            if let Some(variable) = resolved {
                exceptional.set(variable, Nullability::Null);
            }
            exceptional_exit.push(ExceptionalExit {
                kind: ExceptionKind::NULL_POINTER,
                env: exceptional,
            });
        }
        if receiver_value == Nullability::Null {
            return ExpressionFlow {
                value: Nullability::Unknown,
                boolean_value: None,
                normal_exit: None,
                exceptional_exit,
                writes,
            };
        }
        if let Some(variable) = resolved {
            exit.set(variable, Nullability::NonNull);
        }
    }
    add_static_field_initialization_exit(node, method_context, &exit, &mut exceptional_exit);
    ExpressionFlow {
        value: expr_nullability(node, &exit, resolver, method_context),
        boolean_value: None,
        normal_exit: Some(exit),
        exceptional_exit,
        writes,
    }
}

fn add_static_field_initialization_exit(
    field_access: &SyntaxNode,
    method_context: &MethodContext,
    env: &NullabilityEnv,
    exceptional_exit: &mut ExceptionalExits,
) {
    let may_initialize = field_access_name_offset(field_access)
        .and_then(|offset| method_context.member_uses.get(&offset))
        .is_some_and(|member| {
            member.dispatch() == DispatchKind::Static
                && member.effect() != InvocationEffect::NonThrowing
        });
    if may_initialize {
        let effect = field_access_name_offset(field_access)
            .and_then(|offset| method_context.member_uses.get(&offset))
            .map_or(InvocationEffect::Arbitrary, |member| member.effect());
        exceptional_exit.push(ExceptionalExit {
            kind: if effect == InvocationEffect::ClassInitialization {
                ExceptionKind::SubtypeOf(Cow::Borrowed("java.lang.Error"))
            } else {
                ExceptionKind::Unknown
            },
            env: env.clone(),
        });
    }
}

fn evaluate_array_access<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let mut expressions = node.children().filter(|child| is_expression(child.kind()));
    let Some(receiver) = expressions.next() else {
        return normal_expression(Nullability::Unknown, env);
    };
    let captured_place = capture_stable_place(&receiver, resolver);
    let mut writes = SmallVec::new();
    let mut exceptional_exit = ExceptionalExits::new();
    let receiver_flow = evaluate_expression_chain(
        &receiver,
        env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let (receiver_value, _, mut effective_env) =
        match absorb_child_flow(receiver_flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    let mut index_writes: SmallVec<[VariableId; 2]> = SmallVec::new();
    for index in expressions {
        let flow = evaluate_expression_chain(
            &index,
            &effective_env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        let flow = require_unboxing(
            flow,
            &index,
            RequiredPrimitive::Numeric,
            resolver,
            method_context,
            diagnostics,
        );
        for variable in &flow.writes {
            if !index_writes.contains(variable) {
                index_writes.push(*variable);
            }
        }
        effective_env = match absorb_child_flow(flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(_, _, exit) => exit,
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    }
    // Java evaluates every index expression before testing the captured array
    // reference for null. In particular, an abruptly completing index means
    // the array dereference never happens.
    let resolved = check_receiver_dereference(
        &receiver,
        node,
        receiver_value,
        resolver,
        method_context,
        diagnostics,
    );
    let stable_source = match (resolved, captured_place.as_ref()) {
        (Some(variable), Some(captured_place))
            if !write_invalidates_captured_place(&index_writes, captured_place, true, resolver) =>
        {
            Some(variable)
        }
        _ => None,
    };
    if receiver_value.may_be_null() {
        let mut exceptional = effective_env.clone();
        if let Some(variable) = stable_source {
            exceptional.set(variable, Nullability::Null);
        }
        exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::NULL_POINTER,
            env: exceptional,
        });
    }
    if receiver_value == Nullability::Null {
        return ExpressionFlow {
            value: Nullability::Unknown,
            boolean_value: None,
            normal_exit: None,
            exceptional_exit,
            writes,
        };
    }
    if let Some(variable) = stable_source {
        effective_env.set(variable, Nullability::NonNull);
    }
    // Bounds checks occur only after the array null check completed normally.
    exceptional_exit.push(ExceptionalExit {
        kind: ExceptionKind::Known(Cow::Borrowed("java.lang.ArrayIndexOutOfBoundsException")),
        env: effective_env.clone(),
    });
    ExpressionFlow {
        value: expr_nullability(node, &effective_env, resolver, method_context),
        boolean_value: None,
        normal_exit: Some(effective_env),
        exceptional_exit,
        writes,
    }
}

fn evaluate_method_reference<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let Some(receiver) = node.children().find(|child| is_expression(child.kind())) else {
        // `TypeName::method`: the left side denotes a type and has no run-time
        // receiver to evaluate.
        return normal_expression(Nullability::NonNull, env);
    };
    let receiver_is_expression = !matches!(receiver.kind(), SyntaxKind::NAME_EXPR)
        || resolve_variable_from_expr(&receiver, resolver).is_some();
    if !receiver_is_expression {
        return normal_expression(Nullability::NonNull, env);
    }

    let mut writes = SmallVec::new();
    let mut exceptional_exit = ExceptionalExits::new();
    let receiver_flow = evaluate_expression_chain(
        &receiver,
        env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let (receiver_value, _, mut normal_exit) =
        match absorb_child_flow(receiver_flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(value, boolean, exit) => (value, boolean, exit),
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    let resolved = check_receiver_dereference(
        &receiver,
        node,
        receiver_value,
        resolver,
        method_context,
        diagnostics,
    );
    if receiver_value.may_be_null() {
        let mut exceptional = normal_exit.clone();
        if let Some(variable) = resolved {
            exceptional.set(variable, Nullability::Null);
        }
        exceptional_exit.push(ExceptionalExit {
            kind: ExceptionKind::NULL_POINTER,
            env: exceptional,
        });
    }
    if receiver_value == Nullability::Null {
        return ExpressionFlow {
            value: Nullability::Unknown,
            boolean_value: None,
            normal_exit: None,
            exceptional_exit,
            writes,
        };
    }
    if let Some(variable) = resolved {
        normal_exit.set(variable, Nullability::NonNull);
    }
    ExpressionFlow {
        value: Nullability::NonNull,
        boolean_value: None,
        normal_exit: Some(normal_exit),
        exceptional_exit,
        writes,
    }
}

fn evaluate_ternary_expression<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let mut expressions = node.children().filter(|child| is_expression(child.kind()));
    let (Some(condition), Some(then_value), Some(else_value)) =
        (expressions.next(), expressions.next(), expressions.next())
    else {
        return normal_expression(Nullability::Unknown, env);
    };
    let condition_flow = evaluate_expression_chain(
        &condition,
        env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let mut condition_flow = require_unboxing(
        condition_flow,
        &condition,
        RequiredPrimitive::Boolean,
        resolver,
        method_context,
        diagnostics,
    );
    let Some(condition_exit) = condition_flow.normal_exit.take() else {
        return condition_flow;
    };
    let condition_exceptional_exit = condition_flow.exceptional_exit.take();
    let condition_writes = condition_flow.writes;
    let narrowing =
        analyze_condition_in_context(&ExprRef::new(condition.clone()), resolver, method_context);
    let then_env = narrowing.as_ref().map_or_else(
        || condition_exit.clone(),
        |condition| narrow_environment(&condition_exit, condition, true),
    );
    let else_env = narrowing.as_ref().map_or_else(
        || condition_exit.clone(),
        |condition| narrow_environment(&condition_exit, condition, false),
    );
    let known_condition = condition_flow
        .boolean_value
        .or_else(|| constant_boolean_value(&condition))
        .or_else(|| {
            narrowing
                .as_ref()
                .and_then(|condition| known_condition_truth(condition, &condition_exit))
        });
    let branch_conversion =
        conditional_conversion_requirement(&then_value, &else_value, resolver, method_context);
    if let Some(value) = known_condition {
        let selected_value = if value { &then_value } else { &else_value };
        let selected = evaluate_expression_chain(
            selected_value,
            if value { &then_env } else { &else_env },
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        let mut selected = if let Some(required) = branch_conversion {
            require_unboxing(
                selected,
                selected_value,
                required,
                resolver,
                method_context,
                diagnostics,
            )
        } else {
            selected
        };
        selected.merge_writes(condition_writes);
        selected.exceptional_exit =
            join_optional_env(condition_exceptional_exit, selected.exceptional_exit);
        return selected;
    }
    let then_flow = evaluate_expression_chain(
        &then_value,
        &then_env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let then_flow = if let Some(required) = branch_conversion {
        require_unboxing(
            then_flow,
            &then_value,
            required,
            resolver,
            method_context,
            diagnostics,
        )
    } else {
        then_flow
    };
    let else_flow = evaluate_expression_chain(
        &else_value,
        &else_env,
        resolver,
        method_context,
        diagnostics,
        call_sites,
    );
    let else_flow = if let Some(required) = branch_conversion {
        require_unboxing(
            else_flow,
            &else_value,
            required,
            resolver,
            method_context,
            diagnostics,
        )
    } else {
        else_flow
    };
    let mut joined = join_expression_branches(then_flow, else_flow);
    joined.merge_writes(condition_writes);
    joined.exceptional_exit =
        join_optional_env(condition_exceptional_exit, joined.exceptional_exit);
    joined
}

fn evaluate_binary_expression<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    prior_boolean_value: Option<bool>,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> ExpressionFlow {
    let operator = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| matches!(token.kind(), SyntaxKind::AMP_AMP | SyntaxKind::PIPE_PIPE))
        .map(|token| token.kind());
    let expressions: Vec<_> = node
        .children()
        .filter(|child| is_expression(child.kind()))
        .collect();

    if let Some(operator) = operator {
        let (condition, right, condition_exit, evaluated_boolean, mut exceptional_exit, mut writes) =
            match expressions.as_slice() {
                [left, right, ..] => {
                    let left_flow = evaluate_expression_chain(
                        left,
                        env,
                        resolver,
                        method_context,
                        diagnostics,
                        call_sites,
                    );
                    let mut left_flow = require_unboxing(
                        left_flow,
                        left,
                        RequiredPrimitive::Boolean,
                        resolver,
                        method_context,
                        diagnostics,
                    );
                    let Some(exit) = left_flow.normal_exit.take() else {
                        return left_flow;
                    };
                    (
                        left.clone(),
                        right.clone(),
                        exit,
                        left_flow.boolean_value,
                        left_flow.exceptional_exit,
                        left_flow.writes,
                    )
                }
                [right] => (
                    super::patterns::continuation_left_operand(node)
                        .unwrap_or_else(|| right.clone()),
                    right.clone(),
                    env.clone(),
                    prior_boolean_value,
                    ExceptionalExits::new(),
                    SmallVec::new(),
                ),
                [] => return normal_expression(Nullability::NonNull, env),
            };
        let narrowing = analyze_condition_in_context(
            &ExprRef::new(condition.clone()),
            resolver,
            method_context,
        );
        let evaluate_when_true = operator == SyntaxKind::AMP_AMP;
        let evaluated_env = narrowing.as_ref().map_or_else(
            || condition_exit.clone(),
            |condition| narrow_environment(&condition_exit, condition, evaluate_when_true),
        );
        let skipped_env = narrowing.as_ref().map_or_else(
            || condition_exit.clone(),
            |condition| narrow_environment(&condition_exit, condition, !evaluate_when_true),
        );
        let known_condition = evaluated_boolean
            .or_else(|| constant_boolean_value(&condition))
            .or_else(|| {
                narrowing
                    .as_ref()
                    .and_then(|condition| known_condition_truth(condition, &condition_exit))
            });
        if let Some(left_value) = known_condition {
            let evaluates_right = if evaluate_when_true {
                left_value
            } else {
                !left_value
            };
            if !evaluates_right {
                return ExpressionFlow {
                    value: Nullability::NonNull,
                    boolean_value: Some(left_value),
                    normal_exit: Some(skipped_env),
                    exceptional_exit,
                    writes,
                };
            }
            let right_flow = evaluate_expression_chain(
                &right,
                &evaluated_env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            let mut right_flow = require_unboxing(
                right_flow,
                &right,
                RequiredPrimitive::Boolean,
                resolver,
                method_context,
                diagnostics,
            );
            right_flow.merge_writes(writes);
            exceptional_exit = join_optional_env(exceptional_exit, right_flow.exceptional_exit);
            return ExpressionFlow {
                value: Nullability::NonNull,
                boolean_value: right_flow.boolean_value,
                normal_exit: right_flow.normal_exit,
                exceptional_exit,
                writes: right_flow.writes,
            };
        }
        let right_flow = evaluate_expression_chain(
            &right,
            &evaluated_env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        let mut right_flow = require_unboxing(
            right_flow,
            &right,
            RequiredPrimitive::Boolean,
            resolver,
            method_context,
            diagnostics,
        );
        right_flow.merge_writes(writes.drain(..));
        exceptional_exit = join_optional_env(exceptional_exit, right_flow.exceptional_exit);
        let skipped_boolean = Some(!evaluate_when_true);
        let (normal_exit, boolean_value) = right_flow.normal_exit.map_or_else(
            || (Some(skipped_env.clone()), skipped_boolean),
            |right_exit| {
                (
                    Some(skipped_env.join(&right_exit)),
                    (right_flow.boolean_value == skipped_boolean)
                        .then_some(skipped_boolean)
                        .flatten(),
                )
            },
        );
        return ExpressionFlow {
            value: Nullability::NonNull,
            boolean_value,
            normal_exit,
            exceptional_exit,
            writes: right_flow.writes,
        };
    }

    let divisor_is_known_nonzero = node
        .children()
        .filter(|child| is_expression(child.kind()))
        .last()
        .is_some_and(|divisor| integral_literal_is_definitely_nonzero(&divisor));
    let division_exception = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .any(|token| matches!(token.kind(), SyntaxKind::SLASH | SyntaxKind::PERCENT))
        .then(|| {
            (!divisor_is_known_nonzero)
                .then(|| {
                    division_exception_kind(&expressions_for_binary(node), resolver, method_context)
                })
                .flatten()
        })
        .flatten();
    let mut effective_env = env.clone();
    let mut writes = SmallVec::new();
    let mut exceptional_exit = ExceptionalExits::new();
    let required_primitive = required_primitive_for_binary(node, resolver, method_context);
    for expression in expressions {
        let flow = evaluate_expression_chain(
            &expression,
            &effective_env,
            resolver,
            method_context,
            diagnostics,
            call_sites,
        );
        let flow = if let Some(required) = required_primitive {
            require_unboxing(
                flow,
                &expression,
                required,
                resolver,
                method_context,
                diagnostics,
            )
        } else {
            flow
        };
        effective_env = match absorb_child_flow(flow, &mut exceptional_exit, &mut writes) {
            AbsorbedChild::Normal(_, _, exit) => exit,
            AbsorbedChild::Abrupt(abrupt) => return abrupt,
        };
    }
    if let Some(kind) = division_exception {
        exceptional_exit.push(ExceptionalExit {
            kind,
            env: effective_env.clone(),
        });
    }
    ExpressionFlow {
        value: Nullability::NonNull,
        boolean_value: constant_boolean_value(node).or_else(|| {
            analyze_condition_in_context(&ExprRef::new(node.clone()), resolver, method_context)
                .as_ref()
                .and_then(|condition| known_condition_truth(condition, &effective_env))
        }),
        normal_exit: Some(effective_env),
        exceptional_exit,
        writes,
    }
}

/// Resolve the small, syntax-local subset of boolean constants that controls
/// evaluation order. This is deliberately not a general Java constant folder;
/// recognizing literals and transparent wrappers is enough to avoid analyzing
/// source operands which the language guarantees are skipped.
fn constant_boolean_value(node: &SyntaxNode) -> Option<bool> {
    match node.kind() {
        SyntaxKind::LITERAL_EXPR => match node.text().to_string().trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        SyntaxKind::PAREN_EXPR | SyntaxKind::CAST_EXPR => node
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
            .and_then(|child| constant_boolean_value(&child)),
        SyntaxKind::UNARY_EXPR => {
            let negated = node
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .any(|token| token.kind() == SyntaxKind::BANG);
            let operand = node
                .children()
                .find(|child| is_expression(child.kind()))
                .or_else(|| super::patterns::continuation_left_operand(node));
            operand
                .and_then(|operand| constant_boolean_value(&operand))
                .map(|value| if negated { !value } else { value })
        }
        _ => None,
    }
}

fn known_condition_truth(condition: &NullCheckCondition, env: &NullabilityEnv) -> Option<bool> {
    match condition {
        NullCheckCondition::IsNull(variable) => match env.get(*variable) {
            Nullability::Null => Some(true),
            Nullability::NonNull => Some(false),
            Nullability::Bottom | Nullability::Nullable | Nullability::Unknown => None,
        },
        NullCheckCondition::IsNonNull(variable) => match env.get(*variable) {
            Nullability::Null => Some(false),
            Nullability::NonNull => Some(true),
            Nullability::Bottom | Nullability::Nullable | Nullability::Unknown => None,
        },
        // A single post-expression environment cannot soundly decide a
        // compound condition whose later operands may write variables read by
        // earlier operands. Path-sensitive evaluation handles its components.
        NullCheckCondition::And(_, _) | NullCheckCondition::Or(_, _) => None,
        NullCheckCondition::Not(inner) => known_condition_truth(inner, env).map(|value| !value),
        NullCheckCondition::TypePattern { .. } => None,
        NullCheckCondition::Top => None,
    }
}

fn join_expression_branches(left: ExpressionFlow, right: ExpressionFlow) -> ExpressionFlow {
    let mut writes = left.writes.clone();
    for variable in &right.writes {
        if !writes.contains(variable) {
            writes.push(*variable);
        }
    }
    let exceptional_exit = join_optional_env(
        left.exceptional_exit.clone(),
        right.exceptional_exit.clone(),
    );
    match (left.normal_exit, right.normal_exit) {
        (Some(left_exit), Some(right_exit)) => ExpressionFlow {
            value: left.value.join(right.value),
            boolean_value: (left.boolean_value == right.boolean_value)
                .then_some(left.boolean_value)
                .flatten(),
            normal_exit: Some(left_exit.join(&right_exit)),
            exceptional_exit,
            writes,
        },
        (Some(exit), None) => ExpressionFlow {
            value: left.value,
            boolean_value: left.boolean_value,
            normal_exit: Some(exit),
            exceptional_exit,
            writes,
        },
        (None, Some(exit)) => ExpressionFlow {
            value: right.value,
            boolean_value: right.boolean_value,
            normal_exit: Some(exit),
            exceptional_exit,
            writes,
        },
        (None, None) => ExpressionFlow {
            value: Nullability::Unknown,
            boolean_value: None,
            normal_exit: None,
            exceptional_exit,
            writes,
        },
    }
}

/// Diagnose one dereference receiver and return its stable variable identity,
/// if any, so a successful dereference can narrow subsequent statements.
fn check_receiver_dereference<R: VariableResolver>(
    receiver: &SyntaxNode,
    dereference: &SyntaxNode,
    evaluated_value: Nullability,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) -> Option<VariableId> {
    let range = dereference.text_range();
    if let Some((var_id, name)) = resolve_variable_from_expr(receiver, resolver) {
        // Use the value captured when the receiver was evaluated. Re-resolving
        // it from the post-evaluation environment is wrong for receivers such
        // as a switch expression which mutates a referenced variable before
        // yielding its old value.
        let nullability = evaluated_value;
        if nullability.may_be_null()
            && (resolver.is_scoped_binding(var_id)
                || !is_constructor_injected_field(&name, method_context))
        {
            diagnostics.push(NullabilityDiagnostic {
                kind: DiagnosticKind::PotentialNullDereference,
                location: SourceLocation {
                    start: u32::from(range.start()),
                    end: u32::from(range.end()),
                },
                variable_name: name,
                nullability,
            });
        }
        return Some(var_id);
    }

    let nullability = evaluated_value;
    if matches!(nullability, Nullability::Null | Nullability::Nullable) {
        let variable_name = if receiver.kind() == SyntaxKind::METHOD_CALL_EXPR {
            method_call_info(receiver)
                .0
                .map_or_else(|| "<expression>".to_string(), |name| format!("{name}()"))
        } else {
            "<expression>".to_string()
        };
        diagnostics.push(NullabilityDiagnostic {
            kind: DiagnosticKind::PotentialNullDereference,
            location: SourceLocation {
                start: u32::from(range.start()),
                end: u32::from(range.end()),
            },
            variable_name,
            nullability,
        });
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QualifierAction {
    SkipTypeName,
    Evaluate,
    Dereference,
}

const fn resolved_qualifier_action(binding: ResolvedMemberUse) -> QualifierAction {
    match (binding.qualifier(), binding.dispatch()) {
        (QualifierKind::Implicit, _) | (QualifierKind::TypeName, DispatchKind::Static) => {
            QualifierAction::SkipTypeName
        }
        (QualifierKind::TypeName, DispatchKind::Instance) => QualifierAction::Dereference,
        (QualifierKind::Value, DispatchKind::Static) => QualifierAction::Evaluate,
        (QualifierKind::Value, DispatchKind::Instance) => QualifierAction::Dereference,
    }
}

fn method_qualifier_action<R: VariableResolver>(
    receiver: &SyntaxNode,
    call: &SyntaxNode,
    resolver: &R,
    method_context: &MethodContext,
) -> QualifierAction {
    if let Some(binding) = method_call_name_offset(call)
        .and_then(|offset| method_context.member_uses.get(&offset))
        .copied()
    {
        return resolved_qualifier_action(binding);
    }
    // A resolved field is a value even when its source path begins with a type
    // name (`Values.TEXT`) or the field is inherited and unqualified
    // (`value`). Without preserving that fact, the fallback owner spelling can
    // incorrectly turn the following invocation into a static call whose
    // receiver is skipped.
    let resolved_field_receiver = match receiver.kind() {
        SyntaxKind::FIELD_ACCESS_EXPR => field_access_name_offset(receiver),
        SyntaxKind::NAME_EXPR => Some(u32::from(receiver.text_range().start())),
        _ => None,
    }
    .is_some_and(|offset| method_context.member_uses.contains_key(&offset));
    if resolved_field_receiver {
        return QualifierAction::Dereference;
    }
    // Exact member binding is not always available for switch/record pattern
    // variables (notably `var` components). Their syntax-scoped resolver ID is
    // nevertheless definitive value provenance. Never let an unresolved call
    // owner whose spelling happens to equal that binding turn the call into a
    // type-qualified static access.
    if resolve_variable_from_expr(receiver, resolver)
        .is_some_and(|(variable, _)| resolver.is_scoped_binding(variable))
    {
        return QualifierAction::Dereference;
    }
    if is_static_call_qualifier(receiver, call, method_context) {
        if qualifier_names_selected_owner(receiver, call, method_context) {
            QualifierAction::SkipTypeName
        } else {
            QualifierAction::Evaluate
        }
    } else {
        QualifierAction::Dereference
    }
}

fn qualifier_names_selected_owner(
    receiver: &SyntaxNode,
    call: &SyntaxNode,
    method_context: &MethodContext,
) -> bool {
    let Some(path) = stable_access_path(receiver) else {
        return false;
    };
    let Some(offset) = method_call_name_offset(call) else {
        return false;
    };
    let selected_owner = method_context
        .source_file
        .and_then(|file| {
            method_context
                .method_signatures
                .indexed_call_owner(file, offset)
        })
        .or_else(|| method_context.call_owners.get(&offset).map(String::as_str));
    selected_owner.is_some_and(|owner| {
        path == owner
            || path
                .strip_suffix(owner)
                .is_some_and(|prefix| prefix.ends_with('.'))
            || owner
                .strip_suffix(&path)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn field_qualifier_action(access: &SyntaxNode, method_context: &MethodContext) -> QualifierAction {
    field_access_name_offset(access)
        .and_then(|offset| method_context.member_uses.get(&offset))
        .copied()
        .map_or(QualifierAction::Dereference, resolved_qualifier_action)
}

fn is_static_call_qualifier(
    receiver: &SyntaxNode,
    call: &SyntaxNode,
    method_context: &MethodContext,
) -> bool {
    let Some(path) = stable_access_path(receiver) else {
        return false;
    };
    let Some(name_offset) = method_call_name_offset(call) else {
        return false;
    };
    if let Some(is_static) = method_context.source_file.and_then(|file| {
        method_context
            .method_signatures
            .indexed_call_is_static(file, name_offset)
    }) {
        return is_static;
    }
    method_context
        .call_owners
        .get(&name_offset)
        .is_some_and(|owner| *owner == path)
}

fn check_call_argument_values(
    call: &SyntaxNode,
    evaluated_arguments: &[(SyntaxNode, Nullability)],
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) {
    let (Some(method_name), receiver) = method_call_info(call) else {
        return;
    };
    let parameters = if let Some(contract) =
        indexed_call_contract(call, method_context, evaluated_arguments.len())
    {
        contract.param_nullabilities
    } else if let Some(parameters) = source_call_id(call, method_context).and_then(|id| {
        method_context
            .method_params_by_declaration
            .get(id)
            .map(AsRef::as_ref)
    }) {
        Cow::Borrowed(parameters)
    } else {
        let receiver_is_this = receiver
            .as_ref()
            .is_some_and(|receiver| receiver.kind() == SyntaxKind::THIS_EXPR);
        let source_owner = method_call_name_offset(call)
            .and_then(|name_offset| method_context.call_owners.get(&name_offset))
            .map(String::as_str)
            .or_else(|| {
                (receiver.is_none() || receiver_is_this)
                    .then_some(method_context.current_class.as_str())
            });
        let Some(source_owner) = source_owner else {
            return;
        };
        let key = MethodCallKey {
            class_name: source_owner.to_string(),
            method_name,
            arity: evaluated_arguments.len(),
        };
        let Some(parameters) = method_context
            .method_params
            .get(&key)
            .map(|parameters| Cow::Borrowed(parameters.as_slice()))
            .or_else(|| {
                method_context
                    .method_signatures
                    .get_call_summary(&key)
                    .map(|summary| summary.param_nullabilities)
            })
        else {
            return;
        };
        parameters
    };

    for ((argument, state), parameter) in evaluated_arguments.iter().zip(parameters.iter()) {
        if *parameter != Nullability::NonNull {
            continue;
        }
        if !matches!(*state, Nullability::Null | Nullability::Nullable) {
            continue;
        }
        let range = argument.text_range();
        diagnostics.push(NullabilityDiagnostic {
            kind: DiagnosticKind::NullPassedToNonNull,
            location: SourceLocation {
                start: u32::from(range.start()),
                end: u32::from(range.end()),
            },
            variable_name: extract_name_from_expr(argument)
                .unwrap_or_else(|| "<argument>".to_string()),
            nullability: *state,
        });
    }
}

/// Infer a call result from the values captured while its arguments were
/// evaluated. Re-reading argument syntax under the post-call environment is
/// unsound when an argument assigns a variable and then yields its old value.
fn evaluated_method_call_nullability<R: VariableResolver>(
    node: &SyntaxNode,
    evaluated_arguments: &[(SyntaxNode, Nullability)],
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    invoked_supplier_return: Option<InvokedSupplierReturn>,
) -> Nullability {
    let (method_name, _) = method_call_info(node);
    if method_name.as_deref() == Some("orElse") && is_optional_call(node, method_context) {
        return evaluated_arguments
            .first()
            .map_or(Nullability::Unknown, |(_, value)| {
                Nullability::NonNull.join(*value)
            });
    }
    if method_name.as_deref() == Some("orElseGet") && is_optional_call(node, method_context) {
        return match invoked_supplier_return {
            Some(InvokedSupplierReturn::Returned(supplier)) => Nullability::NonNull.join(supplier),
            // If the fallback is invoked it cannot return, so every normal
            // completion of `orElseGet` came from Optional's present value.
            Some(InvokedSupplierReturn::Abrupt) => Nullability::NonNull,
            None => Nullability::Unknown,
        };
    }
    expr_nullability(node, env, resolver, method_context)
}

fn expr_nullability<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
) -> Nullability {
    match node.kind() {
        SyntaxKind::LITERAL_EXPR => {
            if node.text() == "null" {
                Nullability::Null
            } else {
                Nullability::NonNull
            }
        }
        SyntaxKind::NEW_EXPR
        | SyntaxKind::ARRAY_INIT_EXPR
        | SyntaxKind::CLASS_LITERAL_EXPR
        | SyntaxKind::LAMBDA_EXPR
        | SyntaxKind::METHOD_REF_EXPR
        | SyntaxKind::CONSTRUCTOR_REF_EXPR
        | SyntaxKind::THIS_EXPR
        | SyntaxKind::SUPER_EXPR => Nullability::NonNull,
        // Java binary operators produce either a primitive value or a String
        // value. Neither result can be null, even when string concatenation
        // converts a null operand to the literal text "null".
        SyntaxKind::BINARY_EXPR
        | SyntaxKind::UNARY_EXPR
        | SyntaxKind::INSTANCEOF_EXPR
        | SyntaxKind::PATTERN_INSTANCEOF_EXPR => Nullability::NonNull,
        SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR => {
            if let Some(name) = extract_name_from_expr(node) {
                if let Some(id) = resolver.resolve_variable_node(&name, node) {
                    if env.contains(id) {
                        return env.get(id);
                    }
                }
                // A declaration contract is the value's fallback state, not a
                // replacement for flow facts about this exact access path. A
                // successful `request.user != null` check must therefore win
                // over `@Nullable` on `user` until a write to `request` (or the
                // path itself) invalidates that fact.
                if matches!(
                    node.kind(),
                    SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR
                ) {
                    let member_offset = match node.kind() {
                        SyntaxKind::NAME_EXPR => Some(u32::from(node.text_range().start())),
                        SyntaxKind::FIELD_ACCESS_EXPR => field_access_name_offset(node),
                        _ => None,
                    };
                    if let Some(state) = member_offset
                        .and_then(|offset| method_context.member_uses.get(&offset))
                        .map(|member| member.result_nullability())
                        .filter(|state| *state != Nullability::Unknown)
                    {
                        return state;
                    }
                }
                if let Some(state) = declared_field_state(&name, method_context) {
                    return state;
                }
                if is_constructor_injected_field(&name, method_context) {
                    return Nullability::NonNull;
                }
            }
            resolve_variable_from_expr(node, resolver)
                .map_or(Nullability::Unknown, |(id, _)| env.get(id))
        }
        SyntaxKind::PAREN_EXPR | SyntaxKind::CAST_EXPR => node
            .children()
            // Precede-style assignment/binary continuations are sibling
            // expressions. The final child is the value-producing head; the
            // first child can be only the assignment target.
            .filter(|c| is_expression(c.kind()))
            .last()
            .map_or(Nullability::Unknown, |child| {
                expr_nullability(&child, env, resolver, method_context)
            }),
        SyntaxKind::METHOD_CALL_EXPR => {
            let (method_name, receiver) = method_call_info(node);
            if let Some(name) = method_name {
                let owner = method_call_name_offset(node)
                    .and_then(|name_offset| method_context.call_owners.get(&name_offset));
                let receiver_is_this = receiver
                    .as_ref()
                    .is_some_and(|r| r.kind() == SyntaxKind::THIS_EXPR);
                let source_owner = owner.map(String::as_str).or_else(|| {
                    (receiver.is_none() || receiver_is_this)
                        .then_some(method_context.current_class.as_str())
                });
                if let Some(source_owner) = source_owner {
                    let key = MethodCallKey {
                        class_name: source_owner.to_string(),
                        method_name: name.clone(),
                        arity: method_call_arguments(node).len(),
                    };
                    if let Some(return_nullability) = source_call_id(node, method_context)
                        .and_then(|id| method_context.method_returns_by_declaration.get(id))
                    {
                        return *return_nullability;
                    }
                    if let Some(signature) = indexed_call_contract(
                        node,
                        method_context,
                        method_call_arguments(node).len(),
                    ) {
                        return signature.return_nullability;
                    }
                    if let Some(return_nullability) = method_context.method_returns.get(&key) {
                        return *return_nullability;
                    }
                    if let Some(signature) = method_context.method_signatures.get_call_summary(&key)
                    {
                        return signature.return_nullability;
                    }
                }

                if is_nonnull_returning_method(
                    owner.map(String::as_str),
                    &name,
                    node,
                    method_context,
                ) {
                    return Nullability::NonNull;
                }

                // Special handling for Optional.orElse(T)
                // Result nullability depends on the default value's nullability:
                // - opt.orElse("default") → NonNull (literal is non-null)
                // - opt.orElse(someVar) → depends on someVar's nullability
                // If Optional is present, we get a NonNull value; if empty, we get the default
                if name == "orElse" && is_optional_call(node, method_context) {
                    let args = method_call_arguments(node);
                    if let Some(arg) = args.first() {
                        let arg_nullability = expr_nullability(arg, env, resolver, method_context);
                        // Result is join(NonNull from present value, default nullability)
                        // - If default is NonNull → result is NonNull
                        // - If default is Nullable/Null → result is Nullable
                        // - If default is Unknown → result is Unknown
                        return Nullability::NonNull.join(arg_nullability);
                    }
                    return Nullability::Unknown;
                }

                // Special handling for Optional.orElseGet(Supplier<T>)
                // Result nullability depends on what the supplier returns:
                // - opt.orElseGet(() -> "default") → NonNull (lambda returns non-null)
                // - opt.orElseGet(() -> null) → Nullable (lambda returns null)
                // - opt.orElseGet(this::method) → Unknown (method reference, be conservative)
                // If Optional is present, we get a NonNull value; if empty, we invoke supplier
                if name == "orElseGet" && is_optional_call(node, method_context) {
                    let args = method_call_arguments(node);
                    if let Some(arg) = args.first() {
                        // Check if argument is a lambda expression
                        if arg.kind() == SyntaxKind::LAMBDA_EXPR {
                            let supplier_nullability =
                                analyze_lambda_return(arg, env, resolver, method_context);
                            // Result is join(NonNull from present value, supplier return nullability)
                            return Nullability::NonNull.join(supplier_nullability);
                        }
                        // For method references or other complex suppliers, be conservative
                        return Nullability::Unknown;
                    }
                    return Nullability::Unknown;
                }
            }
            Nullability::Unknown
        }
        SyntaxKind::ASSIGNMENT_EXPR => find_assignment_value_expr(node)
            .map_or(Nullability::Unknown, |value| {
                expr_nullability(&value, env, resolver, method_context)
            }),
        SyntaxKind::TERNARY_EXPR => {
            // Children are `cond ? then : else`. The value's nullability is the
            // join of the two arms, each evaluated under the narrowing implied
            // by the condition (so `x != null ? x : dflt` is non-null). The
            // condition itself does not contribute to the value.
            let mut exprs = node.children().filter(|c| is_expression(c.kind()));
            let cond = exprs.next();
            match (exprs.next(), exprs.next()) {
                (Some(then_node), Some(else_node)) => {
                    let narrowed = cond.and_then(|c| {
                        let cond_ref = ExprRef::new(c);
                        analyze_condition_in_context(&cond_ref, resolver, method_context)
                    });
                    let (then_env, else_env) = match &narrowed {
                        Some(cond) => (
                            narrow_environment(env, cond, true),
                            narrow_environment(env, cond, false),
                        ),
                        None => (env.clone(), env.clone()),
                    };
                    let then_arm =
                        expr_nullability(&then_node, &then_env, resolver, method_context);
                    let else_arm =
                        expr_nullability(&else_node, &else_env, resolver, method_context);
                    then_arm.join(else_arm)
                }
                _ => Nullability::Unknown,
            }
        }
        SyntaxKind::SWITCH_EXPR => {
            switch_expression_nullability(node, env, resolver, method_context)
        }
        _ => Nullability::Unknown,
    }
}

fn indexed_call_contract<'a>(
    call: &SyntaxNode,
    method_context: &'a MethodContext,
    invocation_arity: usize,
) -> Option<super::signature_cache::MethodContract<'a>> {
    let file_id = method_context.source_file?;
    let name_offset = method_call_name_offset(call)?;
    method_context
        .method_signatures
        .indexed_call_contract(file_id, name_offset, invocation_arity)
}

fn source_call_id<'a>(
    call: &SyntaxNode,
    method_context: &'a MethodContext,
) -> Option<&'a Arc<str>> {
    method_context
        .call_declarations
        .get(&method_call_name_offset(call)?)
}

fn switch_expression_nullability<R: VariableResolver>(
    switch: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
) -> Nullability {
    analyze_switch_expression(switch, env, resolver, method_context).value
}

struct AnalyzedSwitchExpression {
    cfg: ControlFlowGraph,
    analysis: NullabilityAnalysis,
    value: Nullability,
    normal_exit: Option<NullabilityEnv>,
    exceptional_exit: ExceptionalExits,
}

fn analyze_switch_expression<R: VariableResolver>(
    switch: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
) -> AnalyzedSwitchExpression {
    let cfg = if method_context.exception_types.is_resolved() {
        ControlFlowGraph::build_switch_expression_with_exception_types(
            switch,
            &method_context.exception_types,
        )
    } else {
        ControlFlowGraph::build_switch_expression_with_external_type_shadows(
            switch,
            &method_context.exception_type_shadows,
        )
    };
    let analysis = analyze(&cfg, env.clone(), resolver, method_context);
    let value = infer_yield_nullability(&analysis).unwrap_or(Nullability::Unknown);
    let mut exceptional_exit = analysis.uncaught_exceptions.clone();
    let mut normal_exit = analysis.entry_states.get(&cfg.exit).cloned();
    let switch_local_ids: HashSet<VariableId> = switch
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::LOCAL_VAR_DECL)
        .filter(|node| {
            !node
                .ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor != switch)
                .any(|ancestor| ancestor.kind().is_type_declaration())
        })
        .flat_map(|declaration| {
            local_declarators(&declaration)
                .into_iter()
                .filter_map(move |declarator| {
                    resolver.resolve_declarator(
                        &declarator.name,
                        &declaration,
                        declarator.name_start,
                    )
                })
        })
        .collect();
    let project_exit = |exit: &mut NullabilityEnv| {
        // Pattern identities are lexical to their arms. Preserve scoped IDs
        // already visible on entry (for example an enclosing foreach binding),
        // but do not carry switch-local identities into the parent analysis.
        // Ordinary arm-block locals use the legacy name-interned identity, so
        // explicitly project those too when they were not live on entry.
        exit.retain(|variable| {
            env.contains(variable)
                || (!resolver.is_scoped_binding(variable) && !switch_local_ids.contains(&variable))
        });
    };
    if let Some(exit) = &mut normal_exit {
        project_exit(exit);
    }
    for exit in exceptional_exit.iter_mut() {
        project_exit(&mut exit.env);
    }
    AnalyzedSwitchExpression {
        cfg,
        analysis,
        value,
        normal_exit,
        exceptional_exit,
    }
}

/// Determine an expression's nullability under the supplied flow state.
///
/// Exposed for file-level property and method-summary inference so those
/// passes share the exact same expression semantics as method analysis.
pub fn expression_nullability<R: VariableResolver>(
    node: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
) -> Nullability {
    expr_nullability(node, env, resolver, method_context)
}

/// Analyze the return nullability of a lambda expression.
///
/// Lambda expressions can have two forms:
/// 1. Expression body: `() -> "default"` - directly returns the expression
/// 2. Block body: `() -> { return "default"; }` - has a block with return statements
///
/// For block bodies with complex control flow (try-catch, if-else with multiple returns),
/// we build a CFG and analyze ALL return paths, joining their nullability values.
/// This correctly handles cases like:
/// ```java
/// () -> {
///     try { return fetchData(); }           // NonNull path
///     catch (Exception e) { return null; }  // Null path
/// }
/// // Result: Nullable (NonNull.join_single(Null) = Nullable)
/// ```
fn analyze_lambda_return<R: VariableResolver>(
    lambda: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
) -> Nullability {
    // Lambda structure: LAMBDA_EXPR contains either:
    // - An expression directly (expression body)
    // - A BLOCK_STMT (block body with statements)

    for child in lambda.children() {
        // Check for expression body (not a block)
        // Expression body is any expression that isn't a block statement
        if is_expression(child.kind()) && child.kind() != SyntaxKind::BLOCK_STMT {
            return expr_nullability(&child, env, resolver, method_context);
        }

        // Check for block body - use CFG-based analysis to handle all return paths
        if child.kind() == SyntaxKind::BLOCK_STMT {
            return analyze_lambda_return_cfg(&child, env, resolver, method_context);
        }
    }

    // Conservative default: if we can't determine the return value, assume Unknown
    Nullability::Unknown
}

struct InvokedLambdaAnalysis {
    return_value: Option<Nullability>,
    normal_exit: Option<NullabilityEnv>,
    exceptional_exit: ExceptionalExits,
    writes: SmallVec<[VariableId; 2]>,
}

#[derive(Clone, Copy)]
enum InvokedSupplierReturn {
    Returned(Nullability),
    Abrupt,
}

fn retain_caller_visible_bindings(env: &mut NullabilityEnv, caller: &NullabilityEnv) {
    env.retain(|variable| caller.contains(variable));
}

fn retain_caller_visible_exception_bindings(exits: &mut ExceptionalExits, caller: &NullabilityEnv) {
    for exit in exits.iter_mut() {
        retain_caller_visible_bindings(&mut exit.env, caller);
    }
}

fn lambda_return_context(method_context: &MethodContext) -> MethodContext {
    let mut context = method_context.clone();
    // The enclosing method's source-level return conversion and declaration
    // contract do not apply to a nested functional-interface body. Optional's
    // supplier result is a reference value; its nullability is summarized by
    // the caller below rather than diagnosed as an enclosing-method return.
    context.return_nullability = Nullability::Unknown;
    context.return_is_primitive = false;
    context
}

fn analyze_invoked_lambda<R: VariableResolver>(
    lambda: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
    call_sites: &mut CallSiteAccumulator<'_>,
) -> InvokedLambdaAnalysis {
    for child in lambda.children() {
        if is_expression(child.kind()) && child.kind() != SyntaxKind::BLOCK_STMT {
            let mut flow = evaluate_expression_chain(
                &child,
                env,
                resolver,
                method_context,
                diagnostics,
                call_sites,
            );
            let return_value = flow.normal_exit.as_ref().map(|_| flow.value);
            if let Some(normal_exit) = &mut flow.normal_exit {
                retain_caller_visible_bindings(normal_exit, env);
            }
            retain_caller_visible_exception_bindings(&mut flow.exceptional_exit, env);
            flow.writes.retain(|variable| env.contains(*variable));
            return InvokedLambdaAnalysis {
                return_value,
                normal_exit: flow.normal_exit,
                exceptional_exit: flow.exceptional_exit,
                writes: flow.writes,
            };
        }
        if child.kind() == SyntaxKind::BLOCK_STMT {
            let cfg = if method_context.exception_types.is_resolved() {
                ControlFlowGraph::build_with_exception_types(
                    &child,
                    &method_context.exception_types,
                )
            } else {
                ControlFlowGraph::build_with_external_type_shadows(
                    &child,
                    &method_context.exception_type_shadows,
                )
            };
            let lambda_context = lambda_return_context(method_context);
            let (analysis, mut normal_exit) =
                analyze_internal(&cfg, env.clone(), resolver, &lambda_context, true);
            if let Some(class_name) = call_sites.class_name {
                call_sites.merge(collect_call_site_nullability(
                    &cfg,
                    &analysis.entry_states,
                    resolver,
                    &lambda_context,
                    class_name,
                ));
            }
            let return_value = accepted_return_nullability(&analysis);
            if let Some(normal_exit) = &mut normal_exit {
                retain_caller_visible_bindings(normal_exit, env);
            }
            let mut exceptional_exit = analysis.uncaught_exceptions.clone();
            retain_caller_visible_exception_bindings(&mut exceptional_exit, env);
            let writes = if normal_exit.is_some() {
                conservative_assignment_writes(&child, resolver)
                    .into_iter()
                    .filter(|variable| env.contains(*variable))
                    .collect()
            } else {
                SmallVec::new()
            };
            diagnostics.extend(analysis.diagnostics.iter().cloned());
            return InvokedLambdaAnalysis {
                return_value,
                normal_exit,
                exceptional_exit,
                writes,
            };
        }
    }
    InvokedLambdaAnalysis {
        return_value: None,
        normal_exit: None,
        exceptional_exit: ExceptionalExits::new(),
        writes: SmallVec::new(),
    }
}

/// CFG-based lambda return analysis.
///
/// Builds a control flow graph for the lambda body and analyzes ALL return
/// terminators, joining their nullability values. This correctly handles:
/// - Try-catch with different return values in each branch
/// - If-else with different return values
/// - Multiple return statements
/// - Nested control flow
fn analyze_lambda_return_cfg<R: VariableResolver>(
    lambda_body: &SyntaxNode,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
) -> Nullability {
    let cfg = if method_context.exception_types.is_resolved() {
        ControlFlowGraph::build_with_exception_types(lambda_body, &method_context.exception_types)
    } else {
        ControlFlowGraph::build_with_external_type_shadows(
            lambda_body,
            &method_context.exception_type_shadows,
        )
    };
    let lambda_context = lambda_return_context(method_context);
    let analysis = analyze(&cfg, env.clone(), resolver, &lambda_context);
    // Return summaries are completion facts, not structural reachability
    // facts. Every operation that can throw contributes an exceptional lane;
    // replaying unreachable catch blocks here invents values from impossible
    // paths (for example a ClassCastException catch around `return "ok"`).
    accepted_return_nullability(&analysis).unwrap_or(Nullability::Unknown)
}

fn maybe_emit_redundant_check<R: VariableResolver>(
    condition: &NullCheckCondition,
    condition_expr: &super::cfg::ExprRef,
    env: &NullabilityEnv,
    resolver: &R,
    method_context: &MethodContext,
    diagnostics: &mut Vec<NullabilityDiagnostic>,
) {
    let var_id = match condition {
        NullCheckCondition::IsNull(var) | NullCheckCondition::IsNonNull(var) => Some(*var),
        _ => None,
    };
    let Some(var_id) = var_id else {
        return;
    };
    if resolver.variable_name(var_id).is_some_and(|name| {
        method_context.param_names.contains(name)
            && !method_context.explicit_nonnull_params.contains(name)
    }) {
        return;
    }
    let known = env.get(var_id);
    let is_redundant = match condition {
        NullCheckCondition::IsNull(_) => known.is_definitely_null(),
        NullCheckCondition::IsNonNull(_) => known == Nullability::NonNull,
        _ => false,
    };
    if is_redundant {
        let range = condition_expr.range();
        diagnostics.push(NullabilityDiagnostic {
            kind: DiagnosticKind::RedundantNullCheck,
            location: SourceLocation {
                start: range.start,
                end: range.end,
            },
            variable_name: "value".to_string(),
            nullability: known,
        });
    }
}

pub(crate) fn resolve_variable_from_expr<R: VariableResolver>(
    node: &SyntaxNode,
    resolver: &R,
) -> Option<(VariableId, String)> {
    let name = extract_name_from_expr(node)?;
    resolver
        .resolve_variable_node(&name, node)
        .map(|id| (id, name))
}

/// Field annotations that indicate non-null semantics.
///
/// This includes:
/// - Dependency injection annotations (fields guaranteed non-null by DI framework)
/// - Lombok @NonNull (generates null checks in constructors)
/// - Standard nullability annotations (@NotNull, @Nonnull)
const NONNULL_FIELD_ANNOTATIONS: &[&str] = &[
    // Dependency injection (guaranteed non-null by framework)
    "Autowired", // Spring Framework
    "Inject",    // Jakarta/javax CDI
    "Resource",  // Jakarta/javax resource injection
    "Value",     // Spring Framework @Value (configuration injection)
    // Lombok and nullability annotations
    "NonNull", // Lombok, various nullability frameworks
    "Nonnull", // JSR-305, Checker Framework
    "NotNull", // JetBrains, Jakarta Bean Validation, Android
];

/// Check if a field has an annotation that indicates non-null semantics.
///
/// This includes dependency injection annotations (@Autowired, @Inject, @Resource)
/// and nullability annotations (@NonNull, @NotNull, @Nonnull).
fn has_nonnull_field_annotation(name: &str, method_context: &MethodContext) -> bool {
    method_context
        .field_annotations
        .get(name)
        .is_some_and(|annotations| {
            annotations.iter().any(|ann| {
                NONNULL_FIELD_ANNOTATIONS.iter().any(|pattern| {
                    // Match both simple name (e.g., "NonNull") and fully qualified name
                    // (e.g., "lombok.NonNull" or "org.jetbrains.annotations.NotNull")
                    ann == *pattern || ann.ends_with(&format!(".{pattern}"))
                })
            })
        })
}

/// Determine if a field should be treated as NonNull because it's:
/// - Assigned in a constructor, OR
/// - Annotated with a non-null annotation (@NonNull, @NotNull, @Nonnull, @Autowired, @Inject, @Resource)
///
/// Fields with non-null annotations are guaranteed to be non-null either by:
/// - Dependency injection frameworks (Spring, CDI)
/// - Lombok's generated constructor code (@NonNull with @RequiredArgsConstructor)
/// - Explicit developer intent (@NotNull, @Nonnull)
fn is_constructor_injected_field(name: &str, method_context: &MethodContext) -> bool {
    let Some(field_name) = direct_field_name(name) else {
        return false;
    };

    // Exclude local variables and parameters (those are tracked via the environment)
    if !name.starts_with("this.")
        && (method_context.local_names.contains(field_name)
            || method_context.param_names.contains(field_name))
    {
        return false;
    }

    // Check if field was assigned in constructor
    if !method_context.constructor_fields_after_explicit_invocation
        && method_context
            .constructor_assigned_fields
            .contains(field_name)
    {
        return true;
    }

    // Check if field has non-null annotation
    if has_nonnull_field_annotation(field_name, method_context) {
        return true;
    }

    false
}

fn declared_field_state(name: &str, method_context: &MethodContext) -> Option<Nullability> {
    let field_name = direct_field_name(name)?;
    if !name.starts_with("this.")
        && (method_context.local_names.contains(field_name)
            || method_context.param_names.contains(field_name))
    {
        return None;
    }
    method_context
        .nullable_fields
        .contains(field_name)
        .then_some(Nullability::Nullable)
}

fn direct_field_name(name: &str) -> Option<&str> {
    if let Some(field) = name.strip_prefix("this.") {
        (!field.contains('.') && !field.contains('[')).then_some(field)
    } else {
        (!name.contains('.') && !name.contains('[')).then_some(name)
    }
}

fn method_call_arguments(node: &SyntaxNode) -> Vec<SyntaxNode> {
    node.children()
        .find(|child| child.kind() == SyntaxKind::ARG_LIST)
        .map_or_else(Vec::new, |arguments| argument_expressions(&arguments))
}

fn recognized_call_owner(node: &SyntaxNode, method_context: &MethodContext) -> Option<String> {
    let offset = method_call_name_offset(node)?;
    if method_context.call_declarations.contains_key(&offset) {
        return None;
    }
    let owner = method_context.call_owners.get(&offset)?;
    canonical_library_owner(owner, node, method_context).map(str::to_owned)
}

fn canonical_library_owner(
    owner: &str,
    usage: &SyntaxNode,
    method_context: &MethodContext,
) -> Option<&'static str> {
    match owner {
        "java.util.Objects" | "Objects" => Some("java.util.Objects"),
        "com.google.common.base.Preconditions" | "Preconditions" => {
            Some("com.google.common.base.Preconditions")
        }
        "java.util.Optional" | "Optional" => Some("java.util.Optional"),
        "java.util.stream.Stream" | "Stream" => Some("java.util.stream.Stream"),
        "java.util.List" | "List" => Some("java.util.List"),
        "java.util.Set" | "Set" => Some("java.util.Set"),
        "java.util.Map" | "Map" => Some("java.util.Map"),
        "java.util.Collection" | "Collection" => Some("java.util.Collection"),
        "java.lang.Iterable" | "Iterable" => Some("java.lang.Iterable"),
        "java.util.Collections" | "Collections" => Some("java.util.Collections"),
        "java.util.Arrays" | "Arrays" => Some("java.util.Arrays"),
        "java.lang.String" => Some("java.lang.String"),
        "String"
            if method_context
                .exception_types
                .is_unshadowed_java_lang_type(usage, owner, "String") =>
        {
            Some("java.lang.String")
        }
        "java.lang.Class" | "Class" => Some("java.lang.Class"),
        _ => None,
    }
}

fn analyze_condition_in_context<R: VariableResolver>(
    condition: &ExprRef,
    resolver: &R,
    method_context: &MethodContext,
) -> Option<NullCheckCondition> {
    analyze_condition_with_call_owners(condition, resolver, &|call| {
        recognized_call_owner(call, method_context)
    })
}

fn is_nonnull_enforcing_call(node: &SyntaxNode, method_context: &MethodContext) -> bool {
    let (Some(name), _) = method_call_info(node) else {
        return false;
    };
    let Some(owner) = recognized_call_owner(node, method_context) else {
        return false;
    };
    match owner.as_str() {
        "java.util.Objects" => name == "requireNonNull",
        "com.google.common.base.Preconditions" => name == "checkNotNull",
        _ => false,
    }
}

fn condition_enforcing_exception(
    node: &SyntaxNode,
    method_context: &MethodContext,
) -> Option<ExceptionKind> {
    let (Some(name), _) = method_call_info(node) else {
        return None;
    };
    if recognized_call_owner(node, method_context)
        .is_none_or(|owner| owner != "com.google.common.base.Preconditions")
    {
        return None;
    }
    match name.as_str() {
        "checkArgument" => Some(ExceptionKind::ILLEGAL_ARGUMENT),
        "checkState" => Some(ExceptionKind::ILLEGAL_STATE),
        _ => None,
    }
}

fn is_nonnull_returning_method(
    owner: Option<&str>,
    name: &str,
    call: &SyntaxNode,
    method_context: &MethodContext,
) -> bool {
    let Some(owner) = owner.and_then(|owner| canonical_library_owner(owner, call, method_context))
    else {
        return false;
    };

    match owner {
        "java.util.Objects" => matches!(
            name,
            "requireNonNull" | "requireNonNullElse" | "requireNonNullElseGet"
        ),
        "com.google.common.base.Preconditions" => name == "checkNotNull",
        "java.util.Optional" => {
            matches!(name, "of" | "ofNullable" | "empty" | "orElseThrow")
        }
        "java.util.stream.Stream" => {
            matches!(name, "of" | "ofNullable" | "empty" | "builder")
                || (name == "collect" && uses_jdk_collector(call))
        }
        "java.util.List" | "java.util.Set" | "java.util.Map" => matches!(
            name,
            "of" | "copyOf" | "entrySet" | "keySet" | "values" | "stream" | "iterator"
        ),
        "java.util.Collection" | "java.lang.Iterable" => {
            matches!(name, "stream" | "iterator")
        }
        "java.util.Collections" => matches!(
            name,
            "singletonList"
                | "singleton"
                | "singletonMap"
                | "emptyList"
                | "emptySet"
                | "emptyMap"
                | "unmodifiableCollection"
                | "unmodifiableList"
                | "unmodifiableSet"
                | "unmodifiableMap"
        ),
        "java.util.Arrays" => name == "asList",
        "java.lang.String" => name == "valueOf",
        "java.lang.Class" => matches!(name, "getName" | "getSimpleName"),
        _ => name == "getClass",
    }
}

fn uses_jdk_collector(call: &SyntaxNode) -> bool {
    method_call_arguments(call).first().is_some_and(|argument| {
        let text = argument.text().to_string();
        let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
        compact.starts_with("Collectors.")
            && matches!(
                compact
                    .strip_prefix("Collectors.")
                    .and_then(|rest| rest.split('(').next()),
                Some(
                    "toList"
                        | "toSet"
                        | "toMap"
                        | "toCollection"
                        | "joining"
                        | "groupingBy"
                        | "groupingByConcurrent"
                        | "partitioningBy"
                )
            )
    })
}

fn is_optional_call(node: &SyntaxNode, method_context: &MethodContext) -> bool {
    recognized_call_owner(node, method_context).as_deref() == Some("java.util.Optional")
}

fn is_node_within(node: &SyntaxNode, container: &SyntaxNode) -> bool {
    let node_range = node.text_range();
    let container_range = container.text_range();
    node_range.start() >= container_range.start() && node_range.end() <= container_range.end()
}

fn extract_name_from_expr(node: &SyntaxNode) -> Option<String> {
    stable_access_path(node)
}

fn method_call_info(node: &SyntaxNode) -> (Option<String>, Option<SyntaxNode>) {
    // With nested expression trees, METHOD_CALL_EXPR contains:
    //   receiver_expr DOT IDENT ARG_LIST  (for receiver.method())
    //   NAME_EXPR ARG_LIST                (for method())
    let mut receiver_node = None;
    let mut found_dot = false;
    let mut last_name_expr = None;
    let mut last_ident_token = None;

    for elem in node.children_with_tokens() {
        match elem {
            NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DOT {
                    found_dot = true;
                } else if token.kind().is_identifier() {
                    last_ident_token = Some(token.text().to_string());
                }
            }
            NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::ARG_LIST {
                    break;
                }
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
                    receiver_node = Some(child.clone());
                }
                if child.kind() == SyntaxKind::NAME_EXPR {
                    if let Some(name) = extract_name_from_expr(&child) {
                        last_name_expr = Some(name);
                    }
                }
            }
        }
    }

    let receiver = if found_dot { receiver_node } else { None };
    let method_name = if found_dot {
        last_ident_token
    } else {
        last_name_expr.or(last_ident_token)
    };
    (method_name, receiver)
}

/// Return the byte offset of a call's method-name token.
///
/// A chained expression's nested `METHOD_CALL_EXPR` nodes can all start at the
/// receiver. The method token is the stable, unique identity for each call.
pub(crate) fn method_call_name_offset(node: &SyntaxNode) -> Option<u32> {
    let arguments_start = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ARG_LIST)?
        .text_range()
        .start();
    node.descendants_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier() && token.text_range().end() <= arguments_start)
        .last()
        .map(|token| u32::from(token.text_range().start()))
}

/// Return a call's method name, receiver expression, and argument expressions.
pub fn method_call_descriptor(
    node: &SyntaxNode,
) -> (Option<String>, Option<SyntaxNode>, Vec<SyntaxNode>) {
    let (name, receiver) = method_call_info(node);
    (name, receiver, method_call_arguments(node))
}

fn method_call_receiver(node: &SyntaxNode) -> Option<SyntaxNode> {
    let (_, receiver) = method_call_info(node);
    receiver
}

fn field_access_receiver(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children().next()
}

pub(crate) fn field_access_name_offset(node: &SyntaxNode) -> Option<u32> {
    (node.kind() == SyntaxKind::FIELD_ACCESS_EXPR)
        .then(|| {
            node.descendants_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .filter(|token| token.kind().is_identifier())
                .last()
                .map(|token| u32::from(token.text_range().start()))
        })
        .flatten()
}

fn assignment_target(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.prev_sibling()
}

fn assignment_operator(node: &SyntaxNode) -> Option<SyntaxKind> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .map(|token| token.kind())
        .find(|kind| {
            matches!(
                kind,
                SyntaxKind::EQ
                    | SyntaxKind::PLUS_EQ
                    | SyntaxKind::MINUS_EQ
                    | SyntaxKind::STAR_EQ
                    | SyntaxKind::SLASH_EQ
                    | SyntaxKind::PERCENT_EQ
                    | SyntaxKind::AMP_EQ
                    | SyntaxKind::PIPE_EQ
                    | SyntaxKind::CARET_EQ
                    | SyntaxKind::SHL_EQ
                    | SyntaxKind::SHR_EQ
                    | SyntaxKind::USHR_EQ
            )
        })
}

fn transparent_lvalue(node: &SyntaxNode) -> SyntaxNode {
    let mut current = node.clone();
    while current.kind() == SyntaxKind::PAREN_EXPR {
        let Some(inner) = current
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
        else {
            break;
        };
        current = inner;
    }
    current
}

fn find_assignment_value_expr(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.children()
        .filter(|child| is_expression(child.kind()))
        .last()
}

fn find_initializer_expr(node: &SyntaxNode) -> Option<SyntaxNode> {
    if let Some(assign) = node
        .children()
        .find(|child| child.kind() == SyntaxKind::ASSIGNMENT_EXPR)
    {
        return find_assignment_value_expr(&assign);
    }

    node.children()
        .filter(|child| is_expression(child.kind()))
        .last()
}

struct LocalDeclarator {
    name: String,
    name_start: u32,
    name_end: u32,
    array_suffix: bool,
    initializer: Option<SyntaxNode>,
}

fn local_declarators(node: &SyntaxNode) -> Vec<LocalDeclarator> {
    let Some(type_end) = node
        .children()
        .find(|child| {
            matches!(
                child.kind(),
                SyntaxKind::CLASS_TYPE
                    | SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::VAR_TYPE
                    | SyntaxKind::ARRAY_TYPE
            )
        })
        .map(|child| child.text_range().end())
    else {
        return Vec::new();
    };

    let mut declarators = Vec::new();
    let mut current = None;
    let mut in_initializer = false;
    for element in node.children_with_tokens() {
        if element.text_range().start() < type_end {
            continue;
        }
        match element {
            NodeOrToken::Token(token) if token.kind().is_identifier() && current.is_none() => {
                current = Some(LocalDeclarator {
                    name: token.text().to_string(),
                    name_start: token.text_range().start().into(),
                    name_end: token.text_range().end().into(),
                    array_suffix: false,
                    initializer: None,
                });
            }
            NodeOrToken::Token(token)
                if token.kind() == SyntaxKind::L_BRACKET
                    && current.is_some()
                    && !in_initializer =>
            {
                if let Some(declarator) = current.as_mut() {
                    declarator.array_suffix = true;
                }
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::EQ => {
                in_initializer = current.is_some();
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => {
                if let Some(declarator) = current.take() {
                    declarators.push(declarator);
                }
                in_initializer = false;
            }
            NodeOrToken::Node(child) if in_initializer && is_expression(child.kind()) => {
                if let Some(declarator) = current.as_mut() {
                    // Precede-style chains are direct siblings; the last
                    // expression node is the initializer's complete head.
                    declarator.initializer = Some(child);
                }
            }
            _ => {}
        }
    }
    if let Some(declarator) = current {
        declarators.push(declarator);
    }
    declarators
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
            | SyntaxKind::ARRAY_INIT_EXPR
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
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::SWITCH_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
    )
}

fn assert_expression_heads(assert_stmt: &SyntaxNode) -> (Option<SyntaxNode>, Option<SyntaxNode>) {
    let mut condition = None;
    let mut message = None;
    let mut in_message = false;
    for element in assert_stmt.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::COLON => in_message = true,
            NodeOrToken::Node(expression) if is_expression(expression.kind()) => {
                if in_message {
                    message = Some(expression);
                } else {
                    // Precede-style continuations are sibling nodes. The last
                    // expression before `:` is the condition's complete head.
                    condition = Some(expression);
                }
            }
            _ => {}
        }
    }
    (condition, message)
}

/// Narrow environment based on a null check condition
fn narrow_environment(
    env: &NullabilityEnv,
    condition: &NullCheckCondition,
    is_true: bool,
) -> NullabilityEnv {
    let mut result = env.clone();

    match condition {
        NullCheckCondition::IsNull(var) => {
            result.set(
                *var,
                if is_true {
                    Nullability::Null
                } else {
                    Nullability::NonNull
                },
            );
        }

        NullCheckCondition::IsNonNull(var) => {
            result.set(
                *var,
                if is_true {
                    Nullability::NonNull
                } else {
                    Nullability::Null
                },
            );
        }

        NullCheckCondition::And(left, right) => {
            if is_true {
                // Both must be true
                result = narrow_environment(&result, left, true);
                result = narrow_environment(&result, right, true);
            }
            // If false, we can't narrow (at least one is false, but we don't know which)
        }

        NullCheckCondition::Or(left, right) => {
            if !is_true {
                // Both must be false
                result = narrow_environment(&result, left, false);
                result = narrow_environment(&result, right, false);
            }
            // If true, we can't narrow (at least one is true, but we don't know which)
        }

        NullCheckCondition::Not(inner) => {
            result = narrow_environment(&result, inner, !is_true);
        }

        NullCheckCondition::TypePattern { binding, selector } => {
            if is_true {
                result.set(*binding, Nullability::NonNull);
                if let Some(selector) = selector {
                    result.set(*selector, Nullability::NonNull);
                }
            }
        }

        // A `Top` operand carries no nullability information — narrow nothing.
        NullCheckCondition::Top => {}
    }

    result
}

/// Apply facts learned from a boolean value captured before later call
/// arguments ran. A later argument may replace a binding mentioned by the
/// condition; in that case the captured truth says nothing about the current
/// binding and must not overwrite its post-argument state.
fn apply_captured_condition_narrowing<R: VariableResolver>(
    current: &NullabilityEnv,
    captured: &NullabilityEnv,
    condition: &NullCheckCondition,
    is_true: bool,
    later_writes: &[VariableId],
    resolver: &R,
) -> NullabilityEnv {
    let captured = narrow_environment(captured, condition, is_true);
    let mut result = current.clone();
    let mut variables = SmallVec::<[VariableId; 2]>::new();
    collect_condition_variables(condition, &mut variables);
    for variable in variables {
        let invalidated = later_writes.iter().any(|written| {
            *written == variable
                || resolver
                    .dependent_variables_for(*written)
                    .contains(&variable)
        });
        if !invalidated {
            result.set(variable, captured.get(variable));
        }
    }
    result
}

fn collect_condition_variables(
    condition: &NullCheckCondition,
    variables: &mut SmallVec<[VariableId; 2]>,
) {
    match condition {
        NullCheckCondition::IsNull(variable) | NullCheckCondition::IsNonNull(variable) => {
            if !variables.contains(variable) {
                variables.push(*variable);
            }
        }
        NullCheckCondition::And(left, right) | NullCheckCondition::Or(left, right) => {
            collect_condition_variables(left, variables);
            collect_condition_variables(right, variables);
        }
        NullCheckCondition::Not(inner) => collect_condition_variables(inner, variables),
        NullCheckCondition::TypePattern { binding, selector } => {
            if !variables.contains(binding) {
                variables.push(*binding);
            }
            if let Some(selector) = selector {
                if !variables.contains(selector) {
                    variables.push(*selector);
                }
            }
        }
        NullCheckCondition::Top => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::cfg::ControlFlowGraph;
    use super::super::patterns::SimpleResolver;
    use super::*;
    use hyperion_parser::syntax::SyntaxKind;
    use hyperion_parser::{lexer::Lexer, parser::Parser};

    #[test]
    fn resolver_keeps_scoped_maps_out_of_the_common_case() {
        assert!(
            std::mem::size_of::<NullabilityResolver>() <= 128,
            "scoped foreach metadata should stay behind one optional pointer: {} bytes",
            std::mem::size_of::<NullabilityResolver>()
        );
    }

    #[test]
    fn lane_worklist_keys_remain_two_compact_ids() {
        assert_eq!(
            std::mem::size_of::<FlowPoint>(),
            8,
            "pending completion details belong in the arena, not every worklist key"
        );
        assert_eq!(
            std::mem::size_of::<ResolvedMemberUse>(),
            4,
            "resolved dispatch, qualifier, type, effects, nullability, and conversion-plan ID must stay packed"
        );
        assert!(std::mem::size_of::<FlowLane>() <= 56);
        assert!(std::mem::size_of::<ExceptionalExit>() <= 80);
        assert_eq!(
            std::mem::size_of::<ExceptionalExits>(),
            std::mem::size_of::<Vec<ExceptionalExit>>(),
            "rare exceptional completions must not reserve an 80-byte inline element"
        );
        assert!(
            std::mem::size_of::<ExpressionFlow>() <= 104,
            "expression flow regressed to {} bytes",
            std::mem::size_of::<ExpressionFlow>()
        );
        assert!(
            std::mem::size_of::<NullabilityAnalysis>() <= 152,
            "retained analysis regressed to {} bytes",
            std::mem::size_of::<NullabilityAnalysis>()
        );
    }

    fn build_cfg_and_analyze(code: &str) -> NullabilityAnalysis {
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = hyperion_parser::syntax::SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::BLOCK_STMT)
            .expect("No block found");

        let cfg = ControlFlowGraph::build(&method_body);
        let resolver = SimpleResolver::new();
        let method_context = MethodContext::default();
        analyze(&cfg, NullabilityEnv::new(), &resolver, &method_context)
    }

    #[test]
    fn test_simple_analysis() {
        let analysis = build_cfg_and_analyze("class A { void test() { int x = 1; } }");
        assert!(!analysis.entry_states.is_empty());
    }

    #[test]
    fn unreachable_calls_do_not_contribute_argument_summaries() {
        let code = "class A { void test() { return; consume(null); } }";
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let resolver = SimpleResolver::new();
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert!(
            summaries.keys().all(|key| key.method_name != "consume"),
            "calls after unconditional completion must not affect contracts"
        );
    }

    #[test]
    fn every_return_chain_operand_contributes_to_argument_summaries() {
        let code = r#"
            class A {
                String test() {
                    return consume("") + consume("") + consume(null);
                }
                String consume(Object value) { return ""; }
            }
        "#;
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let resolver = SimpleResolver::new();
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert_eq!(
            summaries.get(&MethodCallKey {
                class_name: "A".to_string(),
                method_name: "consume".to_string(),
                arity: 1,
            }),
            Some(&vec![Some(Nullability::Nullable)])
        );
    }

    #[test]
    fn switch_call_summaries_follow_yield_reachability_and_arm_state() {
        let code = r#"
            class A {
                Object test(int selector) {
                    Object value = new Object();
                    return switch (selector) {
                        default -> {
                            value = null;
                            yield consume(value);
                            consume(new Object());
                        }
                    };
                }
                Object consume(Object value) { return value; }
            }
        "#;
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let mut resolver = SimpleResolver::new();
        resolver.declare("value");
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert_eq!(
            summaries.get(&MethodCallKey {
                class_name: "A".to_string(),
                method_name: "consume".to_string(),
                arity: 1,
            }),
            Some(&vec![Some(Nullability::Null)]),
            "the call after yield is unreachable and must not widen the reachable arm call"
        );
    }

    #[test]
    fn parent_call_summary_waits_for_switch_arguments_to_complete() {
        let code = r#"
            class A {
                void test(int selector) {
                    consume(switch (selector) {
                        default -> throw new IllegalStateException();
                    });
                }
                void consume(Object value) {}
            }
        "#;
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let resolver = SimpleResolver::new();
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert!(
            summaries.keys().all(|key| key.method_name != "consume"),
            "an invocation never occurs when an argument cannot complete"
        );
    }

    #[test]
    fn call_summary_arguments_observe_left_to_right_switch_effects() {
        let code = r#"
            class A {
                void test(int selector) {
                    Object value = new Object();
                    consume(
                        switch (selector) {
                            default -> { value = null; yield value; }
                        },
                        value
                    );
                }
                void consume(Object first, Object second) {}
            }
        "#;
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let mut resolver = SimpleResolver::new();
        resolver.declare("value");
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert_eq!(
            summaries.get(&MethodCallKey {
                class_name: "A".to_string(),
                method_name: "consume".to_string(),
                arity: 2,
            }),
            Some(&vec![Some(Nullability::Null), Some(Nullability::Null)])
        );
    }

    #[test]
    fn catch_calls_contribute_conservative_argument_summaries() {
        let code = r#"
            class A {
                void test() {
                    try {
                        risky();
                    } catch (RuntimeException exception) {
                        consume(null);
                    }
                }
            }
        "#;
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let resolver = SimpleResolver::new();
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert_eq!(
            summaries.get(&MethodCallKey {
                class_name: "A".to_string(),
                method_name: "consume".to_string(),
                arity: 1,
            }),
            Some(&vec![Some(Nullability::Null)])
        );
    }

    #[test]
    fn finally_calls_after_return_contribute_argument_summaries() {
        let code = r#"
            class A {
                void test() {
                    try {
                        return;
                    } finally {
                        consume(null);
                    }
                }
            }
        "#;
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let resolver = SimpleResolver::new();
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert_eq!(
            summaries.get(&MethodCallKey {
                class_name: "A".to_string(),
                method_name: "consume".to_string(),
                arity: 1,
            }),
            Some(&vec![Some(Nullability::Null)])
        );
    }

    #[test]
    fn unreachable_calls_after_return_in_catch_do_not_contribute() {
        let code = r#"
            class A {
                void test() {
                    try {
                        risky();
                    } catch (RuntimeException exception) {
                        return;
                        consume(null);
                    }
                }
            }
        "#;
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let method_body = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::BLOCK_STMT)
            .expect("method body");
        let cfg = ControlFlowGraph::build(&method_body);
        let resolver = SimpleResolver::new();
        let context = MethodContext::default();
        let analysis = analyze(&cfg, NullabilityEnv::new(), &resolver, &context);

        let summaries =
            collect_call_site_nullability(&cfg, &analysis.entry_states, &resolver, &context, "A");
        assert!(
            summaries.keys().all(|key| key.method_name != "consume"),
            "a compound catch node must not bypass lowered reachability"
        );
    }

    #[test]
    fn test_narrow_after_null_check() {
        let mut env = NullabilityEnv::new();
        env.set(VariableId(0), Nullability::Unknown);

        let condition = NullCheckCondition::IsNonNull(VariableId(0));
        let narrowed = narrow_environment(&env, &condition, true);

        assert_eq!(narrowed.get(VariableId(0)), Nullability::NonNull);
    }

    #[test]
    fn test_narrow_else_branch() {
        let mut env = NullabilityEnv::new();
        env.set(VariableId(0), Nullability::Unknown);

        let condition = NullCheckCondition::IsNonNull(VariableId(0));
        let narrowed = narrow_environment(&env, &condition, false);

        assert_eq!(narrowed.get(VariableId(0)), Nullability::Null);
    }

    #[test]
    fn test_narrow_compound_and() {
        let mut env = NullabilityEnv::new();
        env.set(VariableId(0), Nullability::Unknown);
        env.set(VariableId(1), Nullability::Unknown);

        let condition = NullCheckCondition::And(
            Box::new(NullCheckCondition::IsNonNull(VariableId(0))),
            Box::new(NullCheckCondition::IsNonNull(VariableId(1))),
        );

        let narrowed = narrow_environment(&env, &condition, true);

        assert_eq!(narrowed.get(VariableId(0)), Nullability::NonNull);
        assert_eq!(narrowed.get(VariableId(1)), Nullability::NonNull);
    }

    #[test]
    fn test_narrow_compound_or_false() {
        let mut env = NullabilityEnv::new();
        env.set(VariableId(0), Nullability::Unknown);
        env.set(VariableId(1), Nullability::Unknown);

        // !(a != null || b != null) means both are null
        let condition = NullCheckCondition::Or(
            Box::new(NullCheckCondition::IsNonNull(VariableId(0))),
            Box::new(NullCheckCondition::IsNonNull(VariableId(1))),
        );

        let narrowed = narrow_environment(&env, &condition, false);

        assert_eq!(narrowed.get(VariableId(0)), Nullability::Null);
        assert_eq!(narrowed.get(VariableId(1)), Nullability::Null);
    }
}
