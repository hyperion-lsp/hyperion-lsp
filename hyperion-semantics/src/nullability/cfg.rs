// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Control Flow Graph construction for nullability analysis

use super::{annotation_name, has_nonnull_annotation, has_nullable_annotation, Nullability};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::sync::Arc;

/// java.lang names for which the local evaluator makes identity-sensitive
/// assumptions. Workspace declarations/imports with one of these simple names
/// must disable those assumptions for the same semantic revision.
pub(crate) const IMPLICIT_JAVA_LANG_TYPE_NAMES: &[&str] = &[
    "Object",
    "String",
    "Throwable",
    "Exception",
    "Error",
    "AssertionError",
    "RuntimeException",
    "ArithmeticException",
    "ArrayIndexOutOfBoundsException",
    "ArrayStoreException",
    "ClassCastException",
    "IllegalArgumentException",
    "IllegalStateException",
    "IndexOutOfBoundsException",
    "NegativeArraySizeException",
    "NullPointerException",
    "SecurityException",
    "UnsupportedOperationException",
    "IOException",
];

/// Unique identifier for a basic block
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u32);

impl BlockId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Compact reference to the lexically active catch group for a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct HandlerScopeId(NonZeroU32);

/// Compact reference to the innermost lexically active cleanup body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[doc(hidden)]
pub struct CleanupScopeId(NonZeroU32);

impl HandlerScopeId {
    fn from_index(index: usize) -> Self {
        Self(NonZeroU32::new(u32::try_from(index + 1).expect("handler arena exceeds u32")).unwrap())
    }

    const fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

impl CleanupScopeId {
    fn from_index(index: usize) -> Self {
        Self(NonZeroU32::new(u32::try_from(index + 1).expect("cleanup arena exceeds u32")).unwrap())
    }

    const fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

#[derive(Debug)]
pub(crate) struct CatchArm {
    pub(crate) exception_types: Box<[String]>,
    pub(crate) target: BlockId,
}

#[derive(Debug)]
pub(crate) struct HandlerScope {
    pub(crate) parent: Option<HandlerScopeId>,
    /// Cleanup scope retained on entry to this handler. Any deeper cleanup
    /// must complete before the catch can run.
    pub(crate) cleanup_scope: Option<CleanupScopeId>,
    pub(crate) catches: Box<[CatchArm]>,
}

#[derive(Debug)]
pub(crate) struct CleanupScope {
    pub(crate) parent: Option<CleanupScopeId>,
    /// Lexically enclosing cleanup body, if this cleanup was declared while
    /// another `finally` was executing. This differs from `parent`: an abrupt
    /// completion must not execute an already-running outer cleanup again.
    pub(crate) region_parent: Option<CleanupScopeId>,
    pub(crate) entry: BlockId,
    pub(crate) kind: CleanupKind,
}

/// The Java construct represented by a shared cleanup scope.
///
/// `finally` and resource closing have the same control-flow shape, but differ
/// when a cleanup itself throws: a resource-close failure is suppressed while
/// unwinding an existing exception, whereas a `finally` failure replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CleanupKind {
    Finally,
    Resource,
}

/// One possible destination after Java's ordered catch selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ExceptionDestination {
    pub(crate) target: Option<BlockId>,
    pub(crate) cleanup_scope: Option<CleanupScopeId>,
}

/// Source-local exception identities available while lowering one compilation
/// unit. This is intentionally narrower than the workspace type graph: it
/// proves relationships only when the syntax in hand makes them certain and
/// otherwise leaves catch routing conservative.
#[derive(Debug)]
struct LocalTypeBinding {
    simple_name: String,
    canonical_name: String,
    declaration_start: u32,
    scope_start: u32,
    scope_end: u32,
}

#[derive(Debug, Default)]
struct ExceptionTypeContext {
    direct_superclasses: HashMap<String, String>,
    direct_interfaces: HashMap<String, Box<[String]>>,
    source_types: HashSet<String>,
    type_names_by_range: HashMap<(u32, u32), String>,
    top_level_types: HashMap<String, Option<String>>,
    member_types: HashMap<String, HashMap<String, Option<String>>>,
    local_types: Vec<LocalTypeBinding>,
    fallible_source_constructors: HashSet<String>,
    shadowed_java_lang_names: HashSet<String>,
    package: Option<String>,
}

/// Opaque, immutable exception-type facts shared by every CFG built for one
/// compilation-unit revision.
#[derive(Debug, Clone, Default)]
#[doc(hidden)]
pub struct ExceptionTypeRevision(Option<Arc<ExceptionTypeContext>>);

impl ExceptionTypeContext {
    fn from_scope(scope: &SyntaxNode) -> Self {
        Self::from_scope_with_external_shadows(scope, HashSet::new())
    }

    fn from_scope_with_external_shadows(
        scope: &SyntaxNode,
        shadowed_java_lang_names: HashSet<String>,
    ) -> Self {
        let root = scope.ancestors().last().unwrap_or_else(|| scope.clone());
        let mut context = Self {
            shadowed_java_lang_names,
            package: compilation_unit_package(&root),
            ..Self::default()
        };
        // Explicit single-type imports outrank java.lang. Record that before
        // resolving superclass constructor effects; otherwise an imported
        // type named `RuntimeException` would briefly be mistaken for the
        // known-infallible JDK constructor chain.
        for import in root
            .children()
            .filter(|node| node.kind() == SyntaxKind::IMPORT_DECL)
        {
            let text = import.text().to_string();
            let imported = text
                .trim()
                .trim_start_matches("import")
                .trim()
                .trim_start_matches("static")
                .trim()
                .trim_end_matches(';')
                .trim();
            if imported.ends_with(".*")
                || imported.starts_with("java.lang.")
                || imported == "java.io.IOException"
            {
                continue;
            }
            if let Some(simple) = imported.rsplit('.').next() {
                context.shadowed_java_lang_names.insert(simple.to_string());
            }
        }
        let mut declarations: Vec<_> = root
            .descendants()
            .filter(|node| node.kind().is_type_declaration())
            .collect();
        declarations.sort_by_key(|declaration| {
            declaration
                .ancestors()
                .filter(|ancestor| ancestor.kind().is_type_declaration())
                .count()
        });

        for declaration in &declarations {
            let Some(source_name) = super::qualified_type_name(declaration) else {
                continue;
            };
            let simple_name = source_name.rsplit('.').next().unwrap_or(&source_name);
            let owner = nearest_enclosing_type(declaration);
            let owner_name = owner.as_ref().and_then(|owner| {
                context
                    .type_names_by_range
                    .get(&syntax_range_key(owner))
                    .cloned()
            });
            let is_member = owner
                .as_ref()
                .is_some_and(|owner| declaration.parent().as_ref() == Some(owner));
            let canonical_name = match (&owner_name, is_member) {
                (Some(owner), true) => format!("{owner}.{simple_name}"),
                (Some(owner), false) => format!(
                    "{owner}.<local@{}>.{simple_name}",
                    u32::from(declaration.text_range().start())
                ),
                (None, _) => simple_name.to_owned(),
            };
            context
                .type_names_by_range
                .insert(syntax_range_key(declaration), canonical_name.clone());
            context.source_types.insert(canonical_name.clone());
            if declaration_has_fallible_initialization(declaration) {
                context
                    .fallible_source_constructors
                    .insert(canonical_name.clone());
            }

            if let Some(owner_name) = owner_name {
                if is_member {
                    insert_unambiguous(
                        context.member_types.entry(owner_name).or_default(),
                        simple_name.to_owned(),
                        canonical_name,
                    );
                } else if let Some(scope) = declaration
                    .ancestors()
                    .skip(1)
                    .take_while(|ancestor| owner.as_ref() != Some(ancestor))
                    .find(|ancestor| ancestor.kind() == SyntaxKind::BLOCK_STMT)
                {
                    context.local_types.push(LocalTypeBinding {
                        simple_name: simple_name.to_owned(),
                        canonical_name,
                        declaration_start: declaration.text_range().start().into(),
                        scope_start: scope.text_range().start().into(),
                        scope_end: scope.text_range().end().into(),
                    });
                }
            } else {
                insert_unambiguous(
                    &mut context.top_level_types,
                    simple_name.to_owned(),
                    canonical_name,
                );
            }
        }

        // Resolve each extends clause at its own declaration site. Reusing the
        // analyzed method's visible-name map is wrong for `A.Boom extends
        // Base` when another enclosing type also declares `Base`.
        for declaration in &declarations {
            let Some(owner) = context
                .type_names_by_range
                .get(&syntax_range_key(declaration))
                .cloned()
            else {
                continue;
            };
            if let Some(superclass) = direct_superclass_type(declaration) {
                let written = superclass.text().to_string().trim().to_owned();
                let resolved = context
                    .resolve_type_at(&superclass, &written)
                    .unwrap_or(written);
                context.direct_superclasses.insert(owner.clone(), resolved);
            }

            let interfaces: Box<[_]> = direct_interface_nodes(declaration)
                .into_iter()
                .map(|interface| {
                    let written = interface.text().to_string().trim().to_owned();
                    context
                        .resolve_type_at(&interface, &written)
                        .unwrap_or(written)
                })
                .collect();
            if !interfaces.is_empty() {
                context.direct_interfaces.insert(owner, interfaces);
            }
        }

        // Construction runs superclass initialization before this class's
        // instance initializers and constructor body. Propagate that effect
        // once through the source hierarchy so an otherwise-empty subclass
        // does not incorrectly look infallible. An unresolved external
        // superclass stays conservative; the small fixed java.lang hierarchy
        // below is the only external chain proved harmless here.
        let mut changed = true;
        while changed {
            changed = false;
            for declaration in &declarations {
                let Some(owner) = context
                    .type_names_by_range
                    .get(&syntax_range_key(declaration))
                else {
                    continue;
                };
                if context.fallible_source_constructors.contains(owner) {
                    continue;
                }
                let Some(superclass) = context.direct_superclasses.get(owner) else {
                    continue;
                };
                let superclass_may_throw = if context.source_types.contains(superclass) {
                    context.fallible_source_constructors.contains(superclass)
                } else {
                    java_exception_name(superclass, &context).is_none()
                        && !is_unshadowed_java_lang_reference_type(superclass, &context)
                };
                if superclass_may_throw {
                    changed |= context.fallible_source_constructors.insert(owner.clone());
                }
            }
        }

        context
    }

    fn resolve_type_at(&self, usage: &SyntaxNode, written: &str) -> Option<String> {
        if self.source_types.contains(written) {
            return Some(written.to_owned());
        }
        if let Some(package) = &self.package {
            if let Some(unqualified) = written
                .strip_prefix(package)
                .and_then(|name| name.strip_prefix('.'))
            {
                if self.source_types.contains(unqualified) {
                    return Some(unqualified.to_owned());
                }
            }
        }

        if let Some((first, rest)) = written.split_once('.') {
            let owner = self.resolve_simple_type_at(usage, first)?;
            let candidate = format!("{owner}.{rest}");
            return self.source_types.contains(&candidate).then_some(candidate);
        }
        self.resolve_simple_type_at(usage, written)
    }

    fn resolve_source_type<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        self.source_types.contains(name).then_some(name)
    }

    fn resolve_simple_type_at(&self, usage: &SyntaxNode, simple_name: &str) -> Option<String> {
        let offset: u32 = usage.text_range().start().into();
        let mut local = self
            .local_types
            .iter()
            .filter(|binding| {
                binding.simple_name == simple_name
                    && binding.declaration_start <= offset
                    && binding.scope_start <= offset
                    && offset < binding.scope_end
            })
            .min_by_key(|binding| {
                (
                    binding.scope_end - binding.scope_start,
                    std::cmp::Reverse(binding.declaration_start),
                )
            });
        if let Some(binding) = local.take() {
            return Some(binding.canonical_name.clone());
        }

        for owner in usage
            .ancestors()
            .filter(|ancestor| ancestor.kind().is_type_declaration())
            .filter_map(|ancestor| self.type_names_by_range.get(&syntax_range_key(&ancestor)))
        {
            if owner.rsplit('.').next() == Some(simple_name) {
                return Some(owner.clone());
            }
            if let Some(member) = self.resolve_inherited_member_type(owner, simple_name) {
                return Some(member.clone());
            }
        }
        self.top_level_types
            .get(simple_name)
            .and_then(Option::as_ref)
            .cloned()
    }

    fn resolve_inherited_member_type(&self, owner: &str, simple_name: &str) -> Option<&String> {
        let mut frontier = vec![owner];
        let mut visited = HashSet::new();
        while !frontier.is_empty() {
            let mut resolved = None;
            let mut next = Vec::new();
            for candidate_owner in frontier {
                if !visited.insert(candidate_owner) {
                    continue;
                }
                match self
                    .member_types
                    .get(candidate_owner)
                    .and_then(|members| members.get(simple_name))
                {
                    Some(Some(member)) => {
                        if resolved.is_some_and(|previous| previous != member) {
                            return None;
                        }
                        resolved = Some(member);
                    }
                    // Preserve ambiguity rather than selecting an arbitrary
                    // declaration or falling through to a more distant owner.
                    Some(None) => return None,
                    None => {
                        if let Some(superclass) = self.direct_superclasses.get(candidate_owner) {
                            next.push(superclass.as_str());
                        }
                        if let Some(interfaces) = self.direct_interfaces.get(candidate_owner) {
                            next.extend(interfaces.iter().map(String::as_str));
                        }
                    }
                }
            }
            if resolved.is_some() {
                return resolved;
            }
            frontier = next;
        }
        None
    }

    fn source_is_subtype(&self, thrown: &str, caught: &str) -> Option<bool> {
        let thrown = self.resolve_source_type(thrown)?;
        let caught = self.resolve_source_type(caught)?;
        if thrown == caught {
            return Some(true);
        }
        let mut current = thrown;
        let mut remaining = self.direct_superclasses.len();
        while remaining > 0 {
            remaining -= 1;
            let Some(superclass) = self.direct_superclasses.get(current) else {
                return Some(false);
            };
            let Some(resolved) = self.resolve_source_type(superclass) else {
                return Some(false);
            };
            if resolved == caught {
                return Some(true);
            }
            current = resolved;
        }
        Some(false)
    }

    fn source_superclass_frontier<'a>(&'a self, thrown: &'a str) -> Option<&'a str> {
        let mut current = self.resolve_source_type(thrown)?;
        let mut remaining = self.direct_superclasses.len().saturating_add(1);
        while remaining > 0 {
            remaining -= 1;
            let superclass = self.direct_superclasses.get(current)?;
            if let Some(resolved) = self.resolve_source_type(superclass) {
                current = resolved;
            } else {
                return Some(superclass);
            }
        }
        None
    }

    fn shadows_java_lang(&self, simple_name: &str) -> bool {
        self.source_types.contains(simple_name)
            || self.shadowed_java_lang_names.contains(simple_name)
    }

    fn is_resolved_external_type(&self, name: &str) -> bool {
        if self.resolve_source_type(name).is_some() {
            return false;
        }
        if !name.contains('.') {
            return self.shadowed_java_lang_names.contains(name);
        }
        java_exception_name(name, self).is_none()
    }
}

impl ExceptionTypeRevision {
    pub(crate) fn from_scope_with_external_shadows(
        scope: &SyntaxNode,
        shadows: &HashSet<String>,
    ) -> Self {
        Self(Some(Arc::new(
            ExceptionTypeContext::from_scope_with_external_shadows(scope, shadows.clone()),
        )))
    }

    fn context(&self) -> Option<Arc<ExceptionTypeContext>> {
        self.0.clone()
    }

    pub(crate) const fn is_resolved(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn is_same_revision(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }

    pub(crate) fn resolve_type_at(&self, usage: &SyntaxNode, written: &str) -> Option<String> {
        self.0
            .as_deref()
            .and_then(|context| context.resolve_type_at(usage, written))
    }

    pub(crate) fn source_constructor_may_throw(
        &self,
        usage: &SyntaxNode,
        written: &str,
    ) -> Option<bool> {
        let context = self.0.as_deref()?;
        if anonymous_class_initialization_may_throw(usage) {
            return Some(true);
        }
        let resolved = context.resolve_type_at(usage, written)?;
        Some(context.fallible_source_constructors.contains(&resolved))
    }

    pub(crate) fn is_unshadowed_java_lang_type(
        &self,
        usage: &SyntaxNode,
        written: &str,
        expected_simple: &str,
    ) -> bool {
        let Some(context) = self.0.as_deref() else {
            return false;
        };
        if context.resolve_type_at(usage, written).is_some() {
            return false;
        }
        written == format!("java.lang.{expected_simple}")
            || (written == expected_simple && !context.shadows_java_lang(expected_simple))
    }
}

fn declaration_has_fallible_initialization(declaration: &SyntaxNode) -> bool {
    declaration.children().any(|member| match member.kind() {
        SyntaxKind::CONSTRUCTOR_DECL => immediate_syntax_may_throw(&member),
        SyntaxKind::FIELD_DECL
            if !super::extraction::has_attached_modifier(&member, SyntaxKind::STATIC_KW) =>
        {
            immediate_syntax_may_throw(&member)
        }
        SyntaxKind::INSTANCE_INIT => immediate_syntax_may_throw(&member),
        _ => false,
    })
}

fn anonymous_class_initialization_may_throw(creation: &SyntaxNode) -> bool {
    creation.children().any(|member| match member.kind() {
        SyntaxKind::FIELD_DECL
            if !super::extraction::has_attached_modifier(&member, SyntaxKind::STATIC_KW) =>
        {
            immediate_syntax_may_throw(&member)
        }
        SyntaxKind::INSTANCE_INIT => immediate_syntax_may_throw(&member),
        _ => false,
    })
}

fn immediate_syntax_may_throw(node: &SyntaxNode) -> bool {
    fn visit(node: &SyntaxNode, root: bool) -> bool {
        if !root
            && (node.kind().is_type_declaration()
                || matches!(
                    node.kind(),
                    SyntaxKind::LAMBDA_EXPR
                        | SyntaxKind::METHOD_DECL
                        | SyntaxKind::CONSTRUCTOR_DECL
                ))
        {
            return false;
        }
        if !root
            && matches!(
                node.kind(),
                SyntaxKind::THROW_STMT
                    | SyntaxKind::METHOD_CALL_EXPR
                    | SyntaxKind::NEW_EXPR
                    | SyntaxKind::ARRAY_ACCESS_EXPR
                    | SyntaxKind::FIELD_ACCESS_EXPR
                    | SyntaxKind::CAST_EXPR
                    | SyntaxKind::BINARY_EXPR
            )
        {
            return true;
        }
        node.children().any(|child| visit(&child, false))
    }
    visit(node, true)
}

fn insert_unambiguous<K: std::hash::Hash + Eq>(
    bindings: &mut HashMap<K, Option<String>>,
    key: K,
    value: String,
) {
    use std::collections::hash_map::Entry;
    match bindings.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(Some(value));
        }
        Entry::Occupied(mut entry) if entry.get().as_ref() != Some(&value) => {
            entry.insert(None);
        }
        Entry::Occupied(_) => {}
    }
}

fn syntax_range_key(node: &SyntaxNode) -> (u32, u32) {
    (
        node.text_range().start().into(),
        node.text_range().end().into(),
    )
}

fn compilation_unit_package(root: &SyntaxNode) -> Option<String> {
    root.children()
        .find(|child| child.kind() == SyntaxKind::PACKAGE_DECL)
        .map(|package| {
            package
                .text()
                .to_string()
                .trim()
                .trim_start_matches("package")
                .trim()
                .trim_end_matches(';')
                .trim()
                .to_owned()
        })
}

fn nearest_enclosing_type(declaration: &SyntaxNode) -> Option<SyntaxNode> {
    declaration
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.kind().is_type_declaration())
}

fn direct_superclass_type(declaration: &SyntaxNode) -> Option<SyntaxNode> {
    if declaration.kind() != SyntaxKind::CLASS_DECL {
        return None;
    }
    let mut after_extends = false;
    for element in declaration.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::EXTENDS_KW => {
                after_extends = true;
            }
            rowan::NodeOrToken::Node(node)
                if after_extends && node.kind() == SyntaxKind::CLASS_TYPE =>
            {
                return Some(node);
            }
            _ => {}
        }
    }
    None
}

fn direct_interface_nodes(declaration: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut in_inheritance_clause = false;
    let extends_interfaces = declaration.kind() == SyntaxKind::INTERFACE_DECL;
    let mut interfaces = Vec::new();
    for element in declaration.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::IMPLEMENTS_KW => {
                in_inheritance_clause = true;
            }
            rowan::NodeOrToken::Token(token)
                if extends_interfaces && token.kind() == SyntaxKind::EXTENDS_KW =>
            {
                in_inheritance_clause = true;
            }
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::L_BRACE => break,
            rowan::NodeOrToken::Node(node)
                if in_inheritance_clause && node.kind() == SyntaxKind::CLASS_TYPE =>
            {
                interfaces.push(node);
            }
            _ => {}
        }
    }
    interfaces
}

/// Ordered dataflow operation in a basic block.
///
/// The variants keep compound declaration bindings distinct from recursively
/// analyzed syntax. This prevents consumers from accidentally walking an
/// enhanced-for body before its CFG block is reached, while keeping the common
/// representation compact (no owned binding names on ordinary statements).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum StatementRef {
    /// Analyze the referenced syntax as an ordinary statement or expression.
    Analyze(SyntaxNode),
    /// Assign the declared variable its state on entry to a lexical body.
    Bind {
        declaration: SyntaxNode,
        nullability: Nullability,
    },
    /// Evaluate an expression and require its result to be non-null.
    RequireNonNull(SyntaxNode),
    /// Close one successfully initialized try-with-resources resource.
    ///
    /// Java skips this operation when the resource value is null. The
    /// analysis also distinguishes a close failure while unwinding a primary
    /// exception (suppressed) from a close failure on a normal/static lane.
    CloseResource(SyntaxNode),
}

impl StatementRef {
    /// Syntax backing this operation.
    pub const fn node(&self) -> &SyntaxNode {
        match self {
            Self::Analyze(node) | Self::RequireNonNull(node) | Self::CloseResource(node) => node,
            Self::Bind { declaration, .. } => declaration,
        }
    }

    /// Byte range in source.
    pub fn range(&self) -> std::ops::Range<u32> {
        let range = self.node().text_range();
        range.start().into()..range.end().into()
    }

    /// Syntax kind backing this operation.
    pub fn kind(&self) -> SyntaxKind {
        self.node().kind()
    }
}

/// Reference to an expression for conditions
#[derive(Debug, Clone)]
pub struct ExprRef {
    /// The syntax node (for condition analysis)
    pub node: SyntaxNode,
}

impl ExprRef {
    pub const fn new(node: SyntaxNode) -> Self {
        Self { node }
    }

    /// Select the head of a direct expression sequence owned by `owner`.
    ///
    /// The parser represents a long same-precedence chain as sibling
    /// expression nodes. Its final sibling is the head; selecting the first
    /// silently drops the later operands from terminator analysis.
    fn from_direct_expression_sequence(owner: &SyntaxNode) -> Option<Self> {
        owner
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
            .map(Self::new)
    }

    /// Byte range in source.
    pub fn range(&self) -> std::ops::Range<u32> {
        u32::from(self.node.text_range().start())..u32::from(self.node.text_range().end())
    }
}

/// How a basic block terminates
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Terminator {
    /// Unconditional jump to another block
    Goto(BlockId),

    /// Conditional branch (if/while/for condition)
    Branch {
        condition: ExprRef,
        then_block: BlockId,
        else_block: BlockId,
    },

    /// A two-way control-flow split without a boolean source expression.
    ///
    /// Enhanced-for loops use this to represent either entering the next
    /// iteration or completing without pretending that the iterable expression
    /// is a loop condition which is re-evaluated on every iteration.
    BranchUnknown {
        then_block: BlockId,
        else_block: BlockId,
    },

    /// Return from method (with optional return expression)
    Return(Option<ExprRef>),

    /// Evaluate and capture a return operand before crossed cleanup bodies.
    ReturnThroughCleanup {
        value: ExprRef,
        target: BlockId,
        resume: BlockId,
        stop_cleanup: Option<CleanupScopeId>,
    },

    /// Complete a captured return after every crossed cleanup completed.
    ResumeReturn,

    /// Throw exception
    Throw(ExprRef),

    /// Produce a value from the nearest enclosing switch expression.
    Yield {
        value: ExprRef,
        /// Immediate successor after evaluating the value.
        target: BlockId,
        /// Reached only if every intervening cleanup completes normally.
        resume: BlockId,
        /// Innermost cleanup retained by the yield target.
        stop_cleanup: Option<CleanupScopeId>,
    },

    /// Enter the shared cleanup chain for an abrupt completion without a
    /// source operand (bare return, break, or continue).
    EnterCleanup {
        target: BlockId,
        resume: BlockId,
        stop_cleanup: Option<CleanupScopeId>,
    },

    /// Resume a previously evaluated yield after its cleanup chain.
    ResumeYield(BlockId),

    /// Recover an incomplete/invalid switch-expression arm without inventing
    /// a source value. Analyses join `Unknown` while keeping later editor code
    /// reachable.
    MissingYield(BlockId),

    /// Break out of loop/switch (target block)
    Break(BlockId),

    /// Continue to loop header
    Continue(BlockId),

    /// Normal exit (end of method, fall-through)
    Exit,

    /// Normal end of one shared `finally` body. Ordinary flow continues at
    /// `normal_target`; a pending abrupt completion instead advances through
    /// the cleanup arena and never takes that normal edge.
    CleanupEnd {
        cleanup: CleanupScopeId,
        normal_target: BlockId,
    },

    /// Parser-recovery or semantically impossible path with no value.
    Unreachable,

    /// Switch statement with multiple targets
    Switch {
        scrutinee: ExprRef,
        /// Non-default labels. `None` denotes a non-expression label such as a
        /// type pattern; the default target is represented separately.
        cases: Vec<(Option<ExprRef>, BlockId)>,
        default: Option<BlockId>,
    },
}

/// A basic block - sequence of statements with single entry/exit
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BasicBlock {
    pub id: BlockId,
    /// Statements in this block (in order)
    pub statements: Vec<StatementRef>,
    /// How this block ends
    pub terminator: Terminator,
    /// Lexically active handler and cleanup scopes. These two compact IDs
    /// replace a cloned catch vector on every potentially throwing block.
    pub(crate) handler_scope: Option<HandlerScopeId>,
    pub(crate) cleanup_scope: Option<CleanupScopeId>,
}

/// Control flow graph for a method body
#[derive(Debug)]
pub struct ControlFlowGraph {
    /// All basic blocks
    pub blocks: HashMap<BlockId, BasicBlock>,
    /// Entry block
    pub entry: BlockId,
    /// Exit block (may have multiple predecessors)
    pub exit: BlockId,
    /// Next block ID to allocate
    next_id: u32,
    pub(crate) handler_scopes: Vec<HandlerScope>,
    pub(crate) cleanup_scopes: Vec<CleanupScope>,
    /// Innermost source `finally` body containing each block. Kept out of
    /// `BasicBlock` so the common block representation remains compact.
    block_cleanup_regions: Vec<Option<CleanupScopeId>>,
    exception_types: Arc<ExceptionTypeContext>,
}

impl ControlFlowGraph {
    /// Create empty CFG
    fn new(exception_types: Arc<ExceptionTypeContext>) -> Self {
        Self {
            blocks: HashMap::new(),
            entry: BlockId(0),
            exit: BlockId(1),
            next_id: 2,
            handler_scopes: Vec::new(),
            cleanup_scopes: Vec::new(),
            block_cleanup_regions: Vec::new(),
            exception_types,
        }
    }

    /// Allocate a new block ID
    const fn alloc_block(&mut self) -> BlockId {
        let id = BlockId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Add a block to the CFG
    fn add_block(&mut self, block: BasicBlock, cleanup_region: Option<CleanupScopeId>) {
        let block_index = block.id.0 as usize;
        if self.block_cleanup_regions.len() <= block_index {
            self.block_cleanup_regions.resize(block_index + 1, None);
        }
        self.block_cleanup_regions[block_index] = cleanup_region;
        self.blocks.insert(block.id, block);
    }

    pub(crate) fn block_cleanup_region(&self, block: BlockId) -> Option<CleanupScopeId> {
        self.block_cleanup_regions
            .get(block.0 as usize)
            .copied()
            .flatten()
    }

    pub(crate) fn exception_type_revision(&self) -> ExceptionTypeRevision {
        ExceptionTypeRevision(Some(Arc::clone(&self.exception_types)))
    }

    pub(crate) fn block_is_within_cleanup_region(
        &self,
        block: BlockId,
        region: CleanupScopeId,
    ) -> bool {
        let mut current = self.block_cleanup_region(block);
        while let Some(candidate) = current {
            if candidate == region {
                return true;
            }
            current = self.cleanup_scope(candidate).region_parent;
        }
        false
    }

    /// Get predecessors of a block (including exception edges)
    pub fn predecessors(&self, block_id: BlockId) -> Vec<BlockId> {
        self.blocks
            .keys()
            .filter_map(|id| {
                self.conservative_successors(*id)
                    .contains(&block_id)
                    .then_some(*id)
            })
            .collect()
    }

    /// Get successors of a block (terminator successors only, for normal control flow)
    pub fn successors(&self, block_id: BlockId) -> Vec<BlockId> {
        self.blocks
            .get(&block_id)
            .map(|b| b.terminator.successors())
            .unwrap_or_default()
    }

    /// Get every statically possible successor of a block.
    ///
    /// This is the conservative graph view: it includes normal terminator
    /// edges, resumable cleanup continuations, and every catch destination
    /// that the block's handler scope could select. Exact dataflow routes a
    /// concrete exceptional completion through the handler arena instead.
    pub fn all_successors(&self, block_id: BlockId) -> Vec<BlockId> {
        self.conservative_successors(block_id)
    }

    /// Successors used for conservative structural reachability.
    ///
    /// These fallback edges deliberately do not participate in dataflow
    /// transfer: they cover implicit exceptional entry without pretending to
    /// know the pending exception. They keep structural analyses conservative.
    pub(crate) fn conservative_successors(&self, block_id: BlockId) -> Vec<BlockId> {
        let mut successors = self.successors(block_id);
        let Some(block) = self.blocks.get(&block_id) else {
            return successors;
        };
        match &block.terminator {
            Terminator::ReturnThroughCleanup {
                target,
                resume,
                stop_cleanup,
                ..
            }
            | Terminator::Yield {
                target,
                resume,
                stop_cleanup,
                ..
            }
            | Terminator::EnterCleanup {
                target,
                resume,
                stop_cleanup,
            } if target != resume && self.static_completion_can_resume(*target, *stop_cleanup) => {
                successors.push(*resume);
            }
            _ => {}
        }
        // Structural consumers (notably lambda return summaries) deliberately
        // retain every lexically possible catch arm even when the nullability
        // evaluator can prove this particular block non-throwing. Forward
        // dataflow remains exact and does not use these fallback edges.
        if block.handler_scope.is_some() {
            for destination in self.exception_destinations(block.handler_scope, None) {
                if let Some(target) = self.first_exception_target(block.cleanup_scope, destination)
                {
                    successors.push(target);
                }
            }
        }
        successors
    }

    fn static_completion_can_resume(
        &self,
        first_entry: BlockId,
        stop_cleanup: Option<CleanupScopeId>,
    ) -> bool {
        let mut cleanup = self
            .cleanup_scopes
            .iter()
            .position(|cleanup| cleanup.entry == first_entry)
            .map(CleanupScopeId::from_index);
        while cleanup != stop_cleanup {
            let Some(current) = cleanup else {
                return false;
            };
            if !self.cleanup_can_complete_normally(current) {
                return false;
            }
            cleanup = self.cleanup_scope(current).parent;
        }
        true
    }

    fn cleanup_can_complete_normally(&self, cleanup: CleanupScopeId) -> bool {
        let mut visited = HashSet::new();
        let mut pending = vec![self.cleanup_scope(cleanup).entry];
        while let Some(block_id) = pending.pop() {
            if !visited.insert(block_id) {
                continue;
            }
            let Some(block) = self.blocks.get(&block_id) else {
                continue;
            };
            if matches!(
                block.terminator,
                Terminator::CleanupEnd {
                    cleanup: completed,
                    ..
                } if completed == cleanup
            ) {
                return true;
            }
            pending.extend(block.terminator.successors());
        }
        false
    }

    /// Whether `block_id` is entered only through exceptional/cleanup flow
    /// which is currently represented for structural analyses but not forward
    /// dataflow. Consumers may use a conservative empty state for these entry
    /// points without treating every state-less normal successor as reachable.
    pub(crate) fn is_conservative_entry(&self, block_id: BlockId) -> bool {
        self.blocks.iter().any(|(source, block)| {
            self.conservative_successors(*source).contains(&block_id)
                && !block.terminator.successors().contains(&block_id)
        })
    }

    pub(crate) fn handler_scope(&self, id: HandlerScopeId) -> &HandlerScope {
        &self.handler_scopes[id.index()]
    }

    pub(crate) fn cleanup_scope(&self, id: CleanupScopeId) -> &CleanupScope {
        &self.cleanup_scopes[id.index()]
    }

    pub(crate) fn exception_destinations(
        &self,
        mut scope: Option<HandlerScopeId>,
        exception_type: Option<&str>,
    ) -> Vec<ExceptionDestination> {
        let mut destinations = Vec::new();
        while let Some(scope_id) = scope {
            let handler = self.handler_scope(scope_id);
            let mut exhaustive = false;
            for catch in &handler.catches {
                let mut possible = false;
                for caught_type in &catch.exception_types {
                    let compatibility = exception_type.and_then(|thrown| {
                        known_exception_assignability(thrown, caught_type, &self.exception_types)
                    });
                    match compatibility {
                        Some(false) => {}
                        Some(true) => {
                            possible = true;
                            exhaustive = true;
                            break;
                        }
                        None => {
                            possible = true;
                            if java_exception_name(caught_type, &self.exception_types)
                                == Some("Throwable")
                            {
                                exhaustive = true;
                                break;
                            }
                        }
                    }
                }
                if possible {
                    destinations.push(ExceptionDestination {
                        target: Some(catch.target),
                        cleanup_scope: handler.cleanup_scope,
                    });
                }
                if exhaustive {
                    break;
                }
            }
            if exhaustive {
                return destinations;
            }
            scope = handler.parent;
        }
        destinations.push(ExceptionDestination {
            target: None,
            cleanup_scope: None,
        });
        destinations
    }

    /// Route a thrown value whose static type is `upper_bound`. Runtime values
    /// may be any subtype of that bound: subtype catches are possible, while a
    /// supertype catch is exhaustive. Treating the bound as an exact runtime
    /// identity would incorrectly skip narrower catches.
    pub(crate) fn exception_destinations_for_upper_bound(
        &self,
        mut scope: Option<HandlerScopeId>,
        upper_bound: &str,
    ) -> Vec<ExceptionDestination> {
        let mut destinations = Vec::new();
        while let Some(scope_id) = scope {
            let handler = self.handler_scope(scope_id);
            let mut exhaustive = false;
            for catch in &handler.catches {
                let mut possible = false;
                for caught_type in &catch.exception_types {
                    let bound_is_caught = known_exception_assignability(
                        upper_bound,
                        caught_type,
                        &self.exception_types,
                    );
                    let catch_within_bound = known_exception_assignability(
                        caught_type,
                        upper_bound,
                        &self.exception_types,
                    );
                    match (bound_is_caught, catch_within_bound) {
                        (Some(true), _) => {
                            possible = true;
                            exhaustive = true;
                            break;
                        }
                        (Some(false), Some(false)) => {}
                        _ => possible = true,
                    }
                    if java_exception_name(caught_type, &self.exception_types) == Some("Throwable")
                    {
                        possible = true;
                        exhaustive = true;
                        break;
                    }
                }
                if possible {
                    destinations.push(ExceptionDestination {
                        target: Some(catch.target),
                        cleanup_scope: handler.cleanup_scope,
                    });
                }
                if exhaustive {
                    break;
                }
            }
            if exhaustive {
                return destinations;
            }
            scope = handler.parent;
        }
        destinations.push(ExceptionDestination {
            target: None,
            cleanup_scope: None,
        });
        destinations
    }

    pub(crate) fn first_exception_target(
        &self,
        active_cleanup: Option<CleanupScopeId>,
        destination: ExceptionDestination,
    ) -> Option<BlockId> {
        if active_cleanup != destination.cleanup_scope {
            return active_cleanup.map(|cleanup| self.cleanup_scope(cleanup).entry);
        }
        destination.target
    }

    /// Build CFG from a method body syntax node
    pub fn build(method_body: &SyntaxNode) -> Self {
        CfgBuilder::new(method_body).build(method_body)
    }

    pub(crate) fn build_with_external_type_shadows(
        method_body: &SyntaxNode,
        shadows: &HashSet<String>,
    ) -> Self {
        CfgBuilder::with_external_type_shadows(method_body, shadows).build(method_body)
    }

    pub(crate) fn build_with_exception_types(
        method_body: &SyntaxNode,
        revision: &ExceptionTypeRevision,
    ) -> Self {
        CfgBuilder::with_exception_types(method_body, revision).build(method_body)
    }

    /// Build the one-operation CFG used by a field initializer. Treating the
    /// initializer itself as the operation (rather than walking its children
    /// as if they were statements) preserves the outer dereference/call.
    pub(crate) fn build_expression_with_exception_types(
        expression: &SyntaxNode,
        revision: &ExceptionTypeRevision,
    ) -> Self {
        let mut builder = CfgBuilder::with_exception_types(expression, revision);
        builder.add_stmt(expression);
        builder.finish_graph()
    }

    pub(crate) fn build_switch_expression_with_external_type_shadows(
        switch: &SyntaxNode,
        shadows: &HashSet<String>,
    ) -> Self {
        CfgBuilder::with_external_type_shadows(switch, shadows).build_switch_expression(switch)
    }

    pub(crate) fn build_switch_expression_with_exception_types(
        switch: &SyntaxNode,
        revision: &ExceptionTypeRevision,
    ) -> Self {
        CfgBuilder::with_exception_types(switch, revision).build_switch_expression(switch)
    }
}

impl Terminator {
    /// Get all successor block IDs
    pub fn successors(&self) -> Vec<BlockId> {
        match self {
            Self::Goto(b)
            | Self::MissingYield(b)
            | Self::ResumeYield(b)
            | Self::Break(b)
            | Self::Continue(b) => vec![*b],
            Self::CleanupEnd { normal_target, .. } => vec![*normal_target],
            Self::Yield { target, .. }
            | Self::EnterCleanup { target, .. }
            | Self::ReturnThroughCleanup { target, .. } => vec![*target],
            Self::Branch {
                then_block,
                else_block,
                ..
            } => {
                vec![*then_block, *else_block]
            }
            Self::BranchUnknown {
                then_block,
                else_block,
            } => vec![*then_block, *else_block],
            Self::Return(_)
            | Self::ResumeReturn
            | Self::Throw(_)
            | Self::Exit
            | Self::Unreachable => vec![],
            Self::Switch { cases, default, .. } => {
                let mut succs: Vec<_> = cases.iter().map(|(_, b)| *b).collect();
                if let Some(d) = default {
                    succs.push(*d);
                }
                succs
            }
        }
    }
}

/// Exception handler context for try-catch-finally
#[derive(Clone)]
struct ExceptionHandler {
    id: HandlerScopeId,
}

#[derive(Clone)]
struct CleanupContext {
    id: CleanupScopeId,
}

#[derive(Clone, Copy)]
struct JumpTarget {
    break_target: BlockId,
    continue_target: Option<BlockId>,
    /// Finally contexts which lexically enclose both the jump and its target.
    /// Only contexts deeper than this boundary are exited by the jump.
    cleanup_depth: usize,
}

#[derive(Clone, Copy)]
struct YieldTarget {
    merge_block: BlockId,
    cleanup_depth: usize,
}

#[derive(Clone)]
struct LabelTarget {
    name: String,
    jump: JumpTarget,
}

/// One switch clause and the block allocated for its body.
///
/// Keeping the syntax node paired with the block avoids recovering identity
/// from an optional label expression later. Pattern labels have no expression
/// child, and therefore cannot be distinguished by the old range lookup.
struct SwitchArm {
    clause: SyntaxNode,
    block: BlockId,
    guarded_body: Option<BlockId>,
    bindings: Vec<(SyntaxNode, Nullability)>,
    labels: Vec<ExprRef>,
    guard: Option<ExprRef>,
    is_default: bool,
    is_arrow: bool,
}

/// One resource whose initializer completed normally and therefore owns an
/// implicit close operation. The arena scope carries the control-flow links;
/// this lowering-only record retains the source syntax for dataflow transfer.
struct ResourceCleanup {
    id: CleanupScopeId,
    entry: BlockId,
    resource: SyntaxNode,
}

/// Builder for constructing CFG from AST
struct CfgBuilder {
    cfg: ControlFlowGraph,
    /// Enclosing loops and switches. Switches are breakable but not
    /// continuable, so an unlabelled continue searches past them for a loop.
    jump_stack: Vec<JumpTarget>,
    /// Lexically enclosing named statements.
    label_stack: Vec<LabelTarget>,
    /// Current block being built
    current_block: BlockId,
    /// Whether the current open tail is reachable from the construct being
    /// lowered. Abrupt statements create an unreachable recovery tail for the
    /// parser, which must not be mistaken for normal completion merely because
    /// its block has not been inserted yet.
    current_reachable: bool,
    /// Blocks with at least one edge from reachable lowering. Synthetic merge
    /// blocks derive reachability from this set instead of being assumed live
    /// when selected as the current insertion point.
    reachable_blocks: HashSet<BlockId>,
    /// Statements accumulated for current block
    current_stmts: Vec<StatementRef>,
    /// Stack of active exception handlers, outermost first.
    exception_handlers: Vec<ExceptionHandler>,
    /// Lexically active finally bodies, outermost first.
    cleanup_stack: Vec<CleanupContext>,
    /// Source cleanup bodies currently being lowered. Unlike `cleanup_stack`,
    /// this includes an already-running `finally` so local jumps inside it can
    /// retain an older pending completion.
    cleanup_region_stack: Vec<CleanupScopeId>,
    /// Value merge for each lexically enclosing switch expression.
    yield_stack: Vec<YieldTarget>,
}

impl CfgBuilder {
    fn new(scope: &SyntaxNode) -> Self {
        let cfg = ControlFlowGraph::new(Arc::new(ExceptionTypeContext::from_scope(scope)));
        Self::with_cfg(cfg)
    }

    fn with_external_type_shadows(scope: &SyntaxNode, shadows: &HashSet<String>) -> Self {
        let cfg = ControlFlowGraph::new(Arc::new(
            ExceptionTypeContext::from_scope_with_external_shadows(scope, shadows.clone()),
        ));
        Self::with_cfg(cfg)
    }

    fn with_exception_types(scope: &SyntaxNode, revision: &ExceptionTypeRevision) -> Self {
        let exception_types = revision.context().unwrap_or_else(|| {
            Arc::new(ExceptionTypeContext::from_scope_with_external_shadows(
                scope,
                HashSet::new(),
            ))
        });
        let cfg = ControlFlowGraph::new(exception_types);
        Self::with_cfg(cfg)
    }

    fn with_cfg(cfg: ControlFlowGraph) -> Self {
        let entry = cfg.entry;
        Self {
            cfg,
            jump_stack: Vec::new(),
            label_stack: Vec::new(),
            current_block: entry,
            current_reachable: true,
            reachable_blocks: HashSet::from([entry]),
            current_stmts: Vec::new(),
            exception_handlers: Vec::new(),
            cleanup_stack: Vec::new(),
            cleanup_region_stack: Vec::new(),
            yield_stack: Vec::new(),
        }
    }

    fn build(mut self, method_body: &SyntaxNode) -> ControlFlowGraph {
        // Process all statements in method body
        self.process_block(method_body);

        self.finish_graph()
    }

    fn build_switch_expression(mut self, switch: &SyntaxNode) -> ControlFlowGraph {
        debug_assert_eq!(switch.kind(), SyntaxKind::SWITCH_EXPR);
        self.process_switch(switch);
        self.finish_graph()
    }

    fn finish_graph(mut self) -> ControlFlowGraph {
        // Finalize current block if not already terminated
        if self.current_reachable
            && (!self.current_stmts.is_empty()
                || !self.cfg.blocks.contains_key(&self.current_block))
        {
            self.finish_block(Terminator::Goto(self.cfg.exit));
        }

        // Add exit block
        self.cfg.add_block(
            BasicBlock {
                id: self.cfg.exit,
                statements: Vec::new(),
                terminator: Terminator::Exit,
                handler_scope: None,
                cleanup_scope: None,
            },
            None,
        );

        self.cfg
    }

    /// Finish current block with given terminator
    fn finish_block(&mut self, terminator: Terminator) {
        if self.current_reachable {
            self.reachable_blocks.extend(terminator.successors());
        }
        let block = BasicBlock {
            id: self.current_block,
            statements: std::mem::take(&mut self.current_stmts),
            terminator,
            handler_scope: self.exception_handlers.last().map(|handler| handler.id),
            cleanup_scope: self.cleanup_stack.last().map(|cleanup| cleanup.id),
        };
        self.cfg
            .add_block(block, self.cleanup_region_stack.last().copied());
    }

    fn add_finished_block(&mut self, block: BasicBlock) {
        if self.reachable_blocks.contains(&block.id) {
            self.reachable_blocks.extend(block.terminator.successors());
        }
        self.cfg
            .add_block(block, self.cleanup_region_stack.last().copied());
    }

    /// Finish an abrupt completion through the shared cleanup graph.
    fn finish_abrupt(&mut self, pending: Terminator) {
        self.finish_abrupt_to(0, pending);
    }

    /// Finish an abrupt completion which retains the cleanup contexts shared
    /// with its target. A loop jump inside a try, for example, must not execute
    /// that try's finally until control actually leaves the try.
    fn finish_abrupt_to(&mut self, target_cleanup_depth: usize, pending: Terminator) {
        debug_assert!(target_cleanup_depth <= self.cleanup_stack.len());
        if target_cleanup_depth == self.cleanup_stack.len() {
            self.finish_block(pending);
            return;
        }

        let resume = self.cfg.alloc_block();
        self.finish_through_cleanups(
            target_cleanup_depth,
            pending,
            resume,
            |target, stop_cleanup| Terminator::EnterCleanup {
                target,
                resume,
                stop_cleanup,
            },
        );
    }

    fn finish_through_cleanups(
        &mut self,
        target_cleanup_depth: usize,
        pending: Terminator,
        resume: BlockId,
        enter: impl FnOnce(BlockId, Option<CleanupScopeId>) -> Terminator,
    ) {
        debug_assert!(target_cleanup_depth < self.cleanup_stack.len());
        let first_cleanup = self
            .cleanup_stack
            .last()
            .expect("crossed cleanup must exist")
            .id;
        let stop_cleanup = target_cleanup_depth
            .checked_sub(1)
            .map(|index| self.cleanup_stack[index].id);
        let source_reachable = self.current_reachable;
        self.finish_block(enter(
            self.cfg.cleanup_scope(first_cleanup).entry,
            stop_cleanup,
        ));
        if source_reachable {
            self.reachable_blocks.insert(resume);
        }
        self.add_finished_block(BasicBlock {
            id: resume,
            statements: Vec::new(),
            terminator: pending,
            handler_scope: None,
            cleanup_scope: stop_cleanup,
        });
    }

    /// Start a new block
    fn start_block(&mut self, id: BlockId) {
        self.current_block = id;
        self.current_reachable = self.reachable_blocks.contains(&id);
        self.current_stmts.clear();
    }

    fn start_unreachable_block(&mut self, id: BlockId) {
        self.current_block = id;
        self.current_reachable = false;
        self.current_stmts.clear();
    }

    /// Add a statement to current block
    fn add_stmt(&mut self, node: &SyntaxNode) {
        self.current_stmts.push(StatementRef::Analyze(node.clone()));
    }

    fn add_binding(&mut self, node: &SyntaxNode, nullability: Nullability) {
        self.current_stmts.push(StatementRef::Bind {
            declaration: node.clone(),
            nullability,
        });
    }

    fn add_required_nonnull(&mut self, node: &SyntaxNode) {
        self.current_stmts
            .push(StatementRef::RequireNonNull(node.clone()));
    }

    fn add_close_resource(&mut self, resource: &SyntaxNode) {
        self.current_stmts
            .push(StatementRef::CloseResource(resource.clone()));
    }

    /// Process a block of statements
    fn process_block(&mut self, block: &SyntaxNode) {
        for child in block.children() {
            if !self.current_reachable {
                break;
            }
            self.process_statement(&child);
        }
    }

    /// Process a single statement
    fn process_statement(&mut self, stmt: &SyntaxNode) {
        match stmt.kind() {
            SyntaxKind::IF_STMT => self.process_if(stmt),
            SyntaxKind::WHILE_STMT => self.process_while(stmt, None),
            SyntaxKind::FOR_STMT => self.process_for(stmt, None),
            SyntaxKind::FOREACH_STMT => self.process_foreach(stmt, None),
            SyntaxKind::DO_WHILE_STMT => self.process_do_while(stmt, None),
            SyntaxKind::TRY_STMT | SyntaxKind::TRY_WITH_RESOURCES_STMT => self.process_try(stmt),
            SyntaxKind::SWITCH_STMT | SyntaxKind::SWITCH_EXPR => self.process_switch(stmt),
            SyntaxKind::RETURN_STMT => self.process_return(stmt),
            SyntaxKind::THROW_STMT => self.process_throw(stmt),
            SyntaxKind::YIELD_STMT => self.process_yield(stmt),
            SyntaxKind::BREAK_STMT => self.process_break(stmt),
            SyntaxKind::CONTINUE_STMT => self.process_continue(stmt),
            SyntaxKind::LABELED_STMT => self.process_labeled(stmt),
            SyntaxKind::BLOCK_STMT => self.process_block(stmt),
            SyntaxKind::SYNCHRONIZED_STMT => self.process_synchronized(stmt),
            _ => {
                // Regular statement (assignment, method call, etc.)
                self.add_stmt(stmt);
            }
        }
    }

    fn process_if(&mut self, if_stmt: &SyntaxNode) {
        // Find condition and branches
        let condition = condition_expr_for_stmt(if_stmt);

        let then_block_id = self.cfg.alloc_block();
        let else_block_id = self.cfg.alloc_block();
        let merge_block_id = self.cfg.alloc_block();

        // Finish current block with branch
        if let Some(cond) = condition {
            self.finish_block(Terminator::Branch {
                condition: cond,
                then_block: then_block_id,
                else_block: else_block_id,
            });
        } else {
            // No condition found, treat as unconditional
            self.finish_block(Terminator::Goto(then_block_id));
        }

        // The body statements are the non-expression children: the condition is
        // the only expression child, so the first statement child is the `then`
        // body and the second (if any) is the `else` body. Routing each through
        // `process_statement` handles braced blocks, braceless single statements
        // (`if (x != null) x = foo();`), and `else if` chains uniformly.
        let mut bodies = if_stmt.children().filter(|c| !is_expression(c.kind()));

        // Process then branch
        self.start_block(then_block_id);
        if let Some(then_body) = bodies.next() {
            self.process_statement(&then_body);
        }
        if self.current_reachable {
            self.finish_block(Terminator::Goto(merge_block_id));
        }

        // Process else branch (braced block, braceless statement, or `else if`)
        self.start_block(else_block_id);
        if let Some(else_body) = bodies.next() {
            self.process_statement(&else_body);
        }
        if self.current_reachable {
            self.finish_block(Terminator::Goto(merge_block_id));
        }

        // Continue from merge block
        self.start_block(merge_block_id);
    }

    fn process_while(&mut self, while_stmt: &SyntaxNode, label_index: Option<usize>) {
        let header_block = self.cfg.alloc_block();
        let body_block = self.cfg.alloc_block();
        let exit_block = self.cfg.alloc_block();
        self.set_label_continue_target(label_index, header_block);

        // Finish current block, go to header
        self.finish_block(Terminator::Goto(header_block));

        // Header: evaluate condition
        self.start_block(header_block);
        let condition = condition_expr_for_stmt(while_stmt);

        if let Some(cond) = condition {
            self.finish_block(Terminator::Branch {
                condition: cond,
                then_block: body_block,
                else_block: exit_block,
            });
        } else {
            self.finish_block(Terminator::Goto(body_block));
        }

        // Body
        self.start_block(body_block);
        self.jump_stack.push(JumpTarget {
            break_target: exit_block,
            continue_target: Some(header_block),
            cleanup_depth: self.cleanup_stack.len(),
        });
        // The body is the only non-expression child (the condition is the
        // expression child); handles both braced and braceless `while` bodies.
        if let Some(body) = while_stmt.children().find(|c| !is_expression(c.kind())) {
            self.process_statement(&body);
        }
        self.jump_stack.pop();
        if self.current_reachable {
            self.finish_block(Terminator::Goto(header_block));
        }

        // Exit
        self.start_block(exit_block);
    }

    fn process_for(&mut self, for_stmt: &SyntaxNode, label_index: Option<usize>) {
        // For loops: init; condition; update
        let init_block = self.cfg.alloc_block();
        let header_block = self.cfg.alloc_block();
        let body_block = self.cfg.alloc_block();
        let update_block = self.cfg.alloc_block();
        let exit_block = self.cfg.alloc_block();
        self.set_label_continue_target(label_index, update_block);

        // Go to init
        self.finish_block(Terminator::Goto(init_block));

        // Segment the for-header by its top-level semicolons: `init; cond;
        // update`. Positional picks (`first expression = condition`) are wrong
        // for expression-list initializers — `for (x = fetch(); ...)` puts
        // NAME_EXPR(x) and ASSIGNMENT_EXPR before the condition.
        let (init_nodes, condition, update_nodes) = for_header_sections(for_stmt);

        // Init block (for initializer)
        self.start_block(init_block);
        for init in &init_nodes {
            self.add_stmt(init);
        }
        self.finish_block(Terminator::Goto(header_block));

        // Header: condition check
        self.start_block(header_block);
        if let Some(cond) = condition.map(ExprRef::new) {
            self.finish_block(Terminator::Branch {
                condition: cond,
                then_block: body_block,
                else_block: exit_block,
            });
        } else {
            // No condition = infinite loop (for(;;))
            self.finish_block(Terminator::Goto(body_block));
        }

        // Body
        self.start_block(body_block);
        self.jump_stack.push(JumpTarget {
            break_target: exit_block,
            continue_target: Some(update_block),
            cleanup_depth: self.cleanup_stack.len(),
        });
        // The body is the last non-expression child. The init (a `LOCAL_VAR_DECL`
        // or `EXPR_STMT`) is also a non-expression child but appears first, so
        // `last()` selects the body for both braced and braceless `for` loops.
        if let Some(body) = for_stmt
            .children()
            .filter(|c| !is_expression(c.kind()))
            .last()
        {
            self.process_statement(&body);
        }
        self.jump_stack.pop();
        if self.current_reachable {
            self.finish_block(Terminator::Goto(update_block));
        }

        // Update block
        self.start_block(update_block);
        for update in &update_nodes {
            self.add_stmt(update);
        }
        self.finish_block(Terminator::Goto(header_block));

        // Exit
        self.start_block(exit_block);
    }

    fn process_foreach(&mut self, foreach_stmt: &SyntaxNode, label_index: Option<usize>) {
        let iterable_block = self.cfg.alloc_block();
        let header_block = self.cfg.alloc_block();
        let body_block = self.cfg.alloc_block();
        let exit_block = self.cfg.alloc_block();
        self.set_label_continue_target(label_index, header_block);

        self.finish_block(Terminator::Goto(iterable_block));

        // Java evaluates the iterable exactly once, before the loop starts.
        // Keeping it outside the back-edge also preserves assignment effects in
        // expressions such as `for (T value : (items = load()))`.
        self.start_block(iterable_block);
        if let Some(iterable) = foreach_stmt
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
        {
            self.add_required_nonnull(&iterable);
        }
        self.finish_block(Terminator::Goto(header_block));

        // Iteration may execute zero or more times. There is no boolean source
        // expression to narrow, so represent the split explicitly.
        self.start_block(header_block);
        self.finish_block(Terminator::BranchUnknown {
            then_block: body_block,
            else_block: exit_block,
        });

        self.start_block(body_block);
        if let Some(nullability) = foreach_binding(foreach_stmt) {
            self.add_binding(foreach_stmt, nullability);
        }
        self.jump_stack.push(JumpTarget {
            break_target: exit_block,
            continue_target: Some(header_block),
            cleanup_depth: self.cleanup_stack.len(),
        });
        if let Some(body) = foreach_stmt
            .children()
            .filter(|child| !is_expression(child.kind()))
            .last()
        {
            self.process_statement(&body);
        }
        self.jump_stack.pop();
        if self.current_reachable {
            self.finish_block(Terminator::Goto(header_block));
        }

        self.start_block(exit_block);
    }

    fn process_synchronized(&mut self, synchronized_stmt: &SyntaxNode) {
        if let Some(monitor) = synchronized_stmt
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
        {
            self.add_required_nonnull(&monitor);
        }
        if let Some(body) = synchronized_stmt
            .children()
            .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
        {
            self.process_block(&body);
        }
    }

    fn process_do_while(&mut self, do_while_stmt: &SyntaxNode, label_index: Option<usize>) {
        let body_block = self.cfg.alloc_block();
        let condition_block = self.cfg.alloc_block();
        let exit_block = self.cfg.alloc_block();
        self.set_label_continue_target(label_index, condition_block);

        // Go directly to body (do-while executes at least once)
        self.finish_block(Terminator::Goto(body_block));

        // Body
        self.start_block(body_block);
        self.jump_stack.push(JumpTarget {
            break_target: exit_block,
            continue_target: Some(condition_block),
            cleanup_depth: self.cleanup_stack.len(),
        });
        // The body is the only non-expression child (the condition is the
        // expression child); handles both braced and braceless `do` bodies.
        if let Some(body) = do_while_stmt.children().find(|c| !is_expression(c.kind())) {
            self.process_statement(&body);
        }
        self.jump_stack.pop();
        if self.current_reachable {
            self.finish_block(Terminator::Goto(condition_block));
        }

        // Condition
        self.start_block(condition_block);
        let condition = condition_expr_for_stmt(do_while_stmt);

        if let Some(cond) = condition {
            self.finish_block(Terminator::Branch {
                condition: cond,
                then_block: body_block,
                else_block: exit_block,
            });
        } else {
            self.finish_block(Terminator::Goto(body_block));
        }

        // Exit
        self.start_block(exit_block);
    }

    fn process_try(&mut self, try_stmt: &SyntaxNode) {
        let merge_block = self.cfg.alloc_block();

        let finally_body = try_stmt
            .children()
            .find(|child| child.kind() == SyntaxKind::FINALLY_CLAUSE)
            .and_then(|finally| {
                finally
                    .children()
                    .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
            });

        // Create finally block if present
        let finally_block = finally_body.as_ref().map(|_| self.cfg.alloc_block());

        // Create separate blocks for each catch clause
        let catch_clauses: Vec<_> = try_stmt
            .children()
            .filter(|c| c.kind() == SyntaxKind::CATCH_CLAUSE)
            .collect();

        let catch_blocks: Vec<(Vec<String>, BlockId)> = catch_clauses
            .iter()
            .map(|catch| {
                let block_id = self.cfg.alloc_block();
                let mut exception_types =
                    extract_catch_exception_types(catch, &self.cfg.exception_types);
                if exception_types.is_empty() {
                    exception_types.push("java.lang.Throwable".to_string());
                }
                (exception_types, block_id)
            })
            .collect();
        // Finish the incoming block before installing this handler. Statements
        // before the `try` are outside its protected region and must not gain
        // edges to these catch clauses.
        let try_block_id = self.cfg.alloc_block();
        self.finish_block(Terminator::Goto(try_block_id));
        if let Some(finally_id) = finally_block {
            // The shared cleanup may be entered only by an abrupt completion;
            // it still has to be lowered even when the protected body has no
            // normal fallthrough.
            if self.reachable_blocks.contains(&try_block_id) {
                self.reachable_blocks.insert(finally_id);
            }
        }

        if let (Some(_), Some(entry)) = (&finally_body, finally_block) {
            let id = CleanupScopeId::from_index(self.cfg.cleanup_scopes.len());
            self.cfg.cleanup_scopes.push(CleanupScope {
                parent: self.cleanup_stack.last().map(|cleanup| cleanup.id),
                region_parent: self.cleanup_region_stack.last().copied(),
                entry,
                kind: CleanupKind::Finally,
            });
            self.cleanup_stack.push(CleanupContext { id });
        }

        // Install the handler only while constructing the protected body.
        let handler_id = HandlerScopeId::from_index(self.cfg.handler_scopes.len());
        self.cfg.handler_scopes.push(HandlerScope {
            parent: self.exception_handlers.last().map(|handler| handler.id),
            cleanup_scope: self.cleanup_stack.last().map(|cleanup| cleanup.id),
            catches: catch_blocks
                .iter()
                .map(|(types, target)| CatchArm {
                    exception_types: types.clone().into_boxed_slice(),
                    target: *target,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        });
        self.exception_handlers
            .push(ExceptionHandler { id: handler_id });
        if self.current_reachable {
            self.reachable_blocks
                .extend(catch_blocks.iter().map(|(_, target)| *target));
        }
        self.start_block(try_block_id);

        // Resource initializers execute left-to-right inside the protected
        // region. A resource becomes active only on its initializer's normal
        // edge, so a failing initializer closes earlier resources but never
        // the resource which failed to initialize.
        let mut resource_cleanups = Vec::new();
        if let Some(resources) = try_stmt
            .children()
            .find(|child| child.kind() == SyntaxKind::RESOURCE_SPEC)
        {
            for resource in resources.children() {
                self.process_statement(&resource);
                if !self.current_reachable {
                    break;
                }

                let after_initializer = self.cfg.alloc_block();
                self.finish_block(Terminator::Goto(after_initializer));

                let entry = self.cfg.alloc_block();
                let id = CleanupScopeId::from_index(self.cfg.cleanup_scopes.len());
                self.cfg.cleanup_scopes.push(CleanupScope {
                    parent: self.cleanup_stack.last().map(|cleanup| cleanup.id),
                    region_parent: self.cleanup_region_stack.last().copied(),
                    entry,
                    kind: CleanupKind::Resource,
                });
                self.cleanup_stack.push(CleanupContext { id });
                resource_cleanups.push(ResourceCleanup {
                    id,
                    entry,
                    resource,
                });

                // Once initialization succeeds, any later abrupt completion
                // may enter this cleanup even if no normal body edge does.
                self.reachable_blocks.insert(entry);
                self.start_block(after_initializer);
            }
        }

        // Process try body - statements will get exception edges via finish_block
        if let Some(try_body) = try_stmt
            .children()
            .find(|c| c.kind() == SyntaxKind::BLOCK_STMT)
        {
            self.process_block(&try_body);
        }

        // Normal exit closes the most recently acquired resource first.
        let after_resources = finally_block.unwrap_or(merge_block);
        let try_exit_target = resource_cleanups
            .last()
            .map_or(after_resources, |cleanup| cleanup.entry);
        if self.current_reachable {
            self.finish_block(Terminator::Goto(try_exit_target));
        }

        // Lower the shared close bodies from inner to outer while the try's
        // handler is still installed. A close failure is therefore caught by
        // this try after all earlier resources have also been attempted.
        for index in (0..resource_cleanups.len()).rev() {
            let resource_cleanup = &resource_cleanups[index];
            let cleanup = self
                .cleanup_stack
                .pop()
                .expect("resource must own a cleanup scope");
            debug_assert_eq!(cleanup.id, resource_cleanup.id);
            let normal_target = index.checked_sub(1).map_or(after_resources, |previous| {
                resource_cleanups[previous].entry
            });

            self.start_block(resource_cleanup.entry);
            self.add_close_resource(&resource_cleanup.resource);
            self.finish_block(Terminator::CleanupEnd {
                cleanup: cleanup.id,
                normal_target,
            });
        }

        // Pop exception handler context - catch blocks don't throw to themselves
        self.exception_handlers.pop();

        // Process each catch clause in its own block
        for (i, catch) in catch_clauses.iter().enumerate() {
            let (_, block_id) = &catch_blocks[i];
            self.start_block(*block_id);

            self.add_binding(catch, Nullability::NonNull);

            // Process catch body
            if let Some(body) = catch
                .children()
                .find(|c| c.kind() == SyntaxKind::BLOCK_STMT)
            {
                self.process_block(&body);
            }

            // Exit from catch -> finally (if present) or merge
            let catch_exit_target = finally_block.unwrap_or(merge_block);
            if self.current_reachable {
                self.finish_block(Terminator::Goto(catch_exit_target));
            }
        }

        // Process finally block (if present)
        if let Some(finally_id) = finally_block {
            let cleanup = self
                .cleanup_stack
                .pop()
                .expect("finally block must own a cleanup scope");
            self.start_block(finally_id);
            self.cleanup_region_stack.push(cleanup.id);
            if let Some(body) = &finally_body {
                self.process_block(body);
            }
            if self.current_reachable {
                self.finish_block(Terminator::CleanupEnd {
                    cleanup: cleanup.id,
                    normal_target: merge_block,
                });
            }
            let popped = self.cleanup_region_stack.pop();
            debug_assert_eq!(popped, Some(cleanup.id));
        }

        // Continue from merge block
        self.start_block(merge_block);
    }

    fn process_switch(&mut self, switch_stmt: &SyntaxNode) {
        let is_expression_switch = switch_stmt.kind() == SyntaxKind::SWITCH_EXPR;
        let scrutinee = ExprRef::from_direct_expression_sequence(switch_stmt);

        let merge_block = self.cfg.alloc_block();
        let arms: Vec<_> = switch_stmt
            .children()
            .filter(|c| c.kind() == SyntaxKind::CASE_CLAUSE)
            .map(|clause| {
                let is_default = clause
                    .children_with_tokens()
                    .any(|element| element.kind() == SyntaxKind::DEFAULT_KW);
                let is_arrow = clause
                    .children_with_tokens()
                    .any(|element| element.kind() == SyntaxKind::ARROW);
                let body_start = clause.children_with_tokens().find_map(|element| {
                    matches!(element.kind(), SyntaxKind::COLON | SyntaxKind::ARROW)
                        .then(|| element.text_range().start())
                });
                let guard = switch_guard_condition(&clause);
                let labels = body_start.map_or_else(Vec::new, |body_start| {
                    clause
                        .children()
                        .filter(|child| {
                            is_expression(child.kind())
                                && child.text_range().end() <= body_start
                                && guard.as_ref().is_none_or(|guard| guard.node != *child)
                        })
                        .map(ExprRef::new)
                        .collect()
                });
                let bindings = switch_label_bindings(&clause);
                SwitchArm {
                    clause,
                    block: self.cfg.alloc_block(),
                    guarded_body: guard.as_ref().map(|_| self.cfg.alloc_block()),
                    bindings,
                    labels,
                    guard,
                    is_default,
                    is_arrow,
                }
            })
            .collect();
        let cases: Vec<_> = arms
            .iter()
            .flat_map(|arm| {
                if arm.labels.is_empty() {
                    (!arm.is_default)
                        .then_some((None, arm.block))
                        .into_iter()
                        .collect::<Vec<_>>()
                } else {
                    arm.labels
                        .iter()
                        .cloned()
                        .map(|label| (Some(label), arm.block))
                        .collect()
                }
            })
            .collect();
        let explicit_default = arms.iter().find(|arm| arm.is_default).map(|arm| arm.block);
        let no_match_block =
            (is_expression_switch && explicit_default.is_none()).then(|| self.cfg.alloc_block());

        // Finish current block with switch
        if let Some(scrutinee) = scrutinee {
            self.finish_block(Terminator::Switch {
                scrutinee,
                cases,
                // A switch statement without `default` can match no label and
                // completes normally. A switch expression has no value on an
                // unmatched recovery path, so it has no normal successor.
                default: explicit_default.or((!is_expression_switch).then_some(merge_block)),
            });
        } else {
            self.finish_block(if is_expression_switch {
                Terminator::MissingYield(merge_block)
            } else {
                Terminator::Goto(merge_block)
            });
        }

        // Process each case body
        self.jump_stack.push(JumpTarget {
            break_target: merge_block,
            continue_target: None,
            cleanup_depth: self.cleanup_stack.len(),
        });
        if is_expression_switch {
            self.yield_stack.push(YieldTarget {
                merge_block,
                cleanup_depth: self.cleanup_stack.len(),
            });
        }

        for (index, arm) in arms.iter().enumerate() {
            self.start_block(arm.block);

            // A successful pattern match creates lexical bindings before its
            // guard is evaluated. Top-level type patterns exclude null; nested
            // record component patterns may be null-matching and stay Unknown
            // until their component type is available. The resolver keys these
            // operations and references by syntax identity, so names repeated
            // in other arms or shadowing fields remain independent.
            for (binding, nullability) in &arm.bindings {
                self.add_binding(binding, *nullability);
            }

            if let (Some(guard), Some(body)) = (&arm.guard, arm.guarded_body) {
                let guard_failed = arms
                    .get(index + 1)
                    .map(|next| next.block)
                    .or(explicit_default)
                    .or(no_match_block)
                    .unwrap_or(merge_block);
                self.finish_block(Terminator::Branch {
                    condition: guard.clone(),
                    then_block: body,
                    else_block: guard_failed,
                });
                self.start_block(body);
            }

            // Only children after `:`/`->` belong to the body. This excludes
            // every label form (constant, pattern, multiple labels, and guarded
            // patterns) without guessing which direct expression was the label.
            let body_start = arm.clause.children_with_tokens().find_map(|element| {
                matches!(element.kind(), SyntaxKind::COLON | SyntaxKind::ARROW)
                    .then(|| element.text_range().end())
            });
            if let Some(body_start) = body_start {
                let body: Vec<_> = arm
                    .clause
                    .children()
                    .filter(|child| child.text_range().start() >= body_start)
                    .collect();
                let direct_value = (is_expression_switch && arm.is_arrow)
                    .then(|| {
                        body.iter()
                            .rev()
                            .find(|child| is_expression(child.kind()))
                            .cloned()
                    })
                    .flatten();
                if let Some(value) = direct_value {
                    self.process_yield_value(&value);
                } else {
                    for statement in body {
                        self.process_statement(&statement);
                    }
                }
            }

            if self.current_reachable {
                // Traditional colon clauses fall through. Arrow clauses are
                // isolated and complete the switch after their body.
                let target = if arm.is_arrow {
                    merge_block
                } else {
                    arms.get(index + 1)
                        .map_or_else(|| no_match_block.unwrap_or(merge_block), |next| next.block)
                };
                self.finish_block(if is_expression_switch && arm.is_arrow {
                    Terminator::MissingYield(target)
                } else if is_expression_switch && target == merge_block {
                    Terminator::MissingYield(target)
                } else {
                    Terminator::Goto(target)
                });
            }
        }

        self.jump_stack.pop();
        if is_expression_switch {
            self.yield_stack.pop();
        }
        if let Some(no_match) = no_match_block {
            self.add_finished_block(BasicBlock {
                id: no_match,
                statements: Vec::new(),
                terminator: Terminator::Unreachable,
                handler_scope: self.exception_handlers.last().map(|handler| handler.id),
                cleanup_scope: self.cleanup_stack.last().map(|cleanup| cleanup.id),
            });
        }
        self.start_block(merge_block);
    }

    fn process_return(&mut self, return_stmt: &SyntaxNode) {
        let expr = ExprRef::from_direct_expression_sequence(return_stmt);
        if let Some(value) = expr {
            if self.cleanup_stack.is_empty() {
                self.finish_block(Terminator::Return(Some(value)));
            } else {
                self.isolate_abrupt_operand();
                let resume = self.cfg.alloc_block();
                self.finish_through_cleanups(
                    0,
                    Terminator::ResumeReturn,
                    resume,
                    |cleanup, stop_cleanup| Terminator::ReturnThroughCleanup {
                        value,
                        target: cleanup,
                        resume,
                        stop_cleanup,
                    },
                );
            }
        } else {
            self.finish_abrupt(Terminator::Return(None));
        }
        // Start unreachable block for any following code
        let unreachable = self.cfg.alloc_block();
        self.start_unreachable_block(unreachable);
    }

    fn process_throw(&mut self, throw_stmt: &SyntaxNode) {
        let Some(expr) = ExprRef::from_direct_expression_sequence(throw_stmt) else {
            // Keep malformed editor input analyzable without manufacturing an
            // exception value or panicking CFG construction.
            self.add_stmt(throw_stmt);
            return;
        };
        self.isolate_abrupt_operand();
        self.finish_block(Terminator::Throw(expr));
        let unreachable = self.cfg.alloc_block();
        self.start_unreachable_block(unreachable);
    }

    /// Put a value-bearing abrupt completion in its own block. Statements
    /// already accumulated retain their ordinary implicit-exception edges;
    /// the new terminator can then own its cleanup-aware dispatch route.
    fn isolate_abrupt_operand(&mut self) {
        if self.current_stmts.is_empty() {
            return;
        }
        let operand_block = self.cfg.alloc_block();
        self.finish_block(Terminator::Goto(operand_block));
        self.start_block(operand_block);
    }

    fn process_yield(&mut self, yield_stmt: &SyntaxNode) {
        if self.yield_stack.is_empty() {
            self.add_stmt(yield_stmt);
            return;
        }
        let Some(value) = ExprRef::from_direct_expression_sequence(yield_stmt) else {
            self.add_stmt(yield_stmt);
            return;
        };
        self.finish_yield(value);
    }

    fn process_yield_value(&mut self, value: &SyntaxNode) {
        self.finish_yield(ExprRef::new(value.clone()));
    }

    fn finish_yield(&mut self, value: ExprRef) {
        let target = *self
            .yield_stack
            .last()
            .expect("yield lowering requires an enclosing switch expression");
        if self.cleanup_stack.len() == target.cleanup_depth {
            self.finish_block(Terminator::Yield {
                value,
                target: target.merge_block,
                resume: target.merge_block,
                stop_cleanup: self.cleanup_stack.last().map(|cleanup| cleanup.id),
            });
        } else {
            let resume = self.cfg.alloc_block();
            self.finish_through_cleanups(
                target.cleanup_depth,
                Terminator::ResumeYield(target.merge_block),
                resume,
                |cleanup, stop_cleanup| Terminator::Yield {
                    value,
                    target: cleanup,
                    resume,
                    stop_cleanup,
                },
            );
        }
        let unreachable = self.cfg.alloc_block();
        self.start_unreachable_block(unreachable);
    }

    fn process_break(&mut self, break_stmt: &SyntaxNode) {
        let label = label_name(break_stmt);
        let target = if let Some(label) = label {
            self.label_stack
                .iter()
                .rev()
                .find(|target| target.name == label)
                .map(|target| (target.jump.break_target, target.jump.cleanup_depth))
        } else {
            self.jump_stack
                .last()
                .map(|target| (target.break_target, target.cleanup_depth))
        };
        let (target, cleanup_depth) = target.unwrap_or((self.cfg.exit, 0));
        let completion = {
            if self
                .yield_stack
                .last()
                .is_some_and(|yield_target| yield_target.merge_block == target)
            {
                Terminator::MissingYield(target)
            } else {
                Terminator::Break(target)
            }
        };
        self.finish_abrupt_to(cleanup_depth, completion);
        let unreachable = self.cfg.alloc_block();
        self.start_unreachable_block(unreachable);
    }

    fn process_continue(&mut self, continue_stmt: &SyntaxNode) {
        let label = label_name(continue_stmt);
        let target = if let Some(label) = label {
            self.label_stack
                .iter()
                .rev()
                .find(|target| target.name == label)
                .and_then(|target| {
                    target
                        .jump
                        .continue_target
                        .map(|block| (block, target.jump.cleanup_depth))
                })
        } else {
            self.jump_stack.iter().rev().find_map(|target| {
                target
                    .continue_target
                    .map(|block| (block, target.cleanup_depth))
            })
        };
        let (target, cleanup_depth) = target.unwrap_or((self.cfg.exit, 0));
        self.finish_abrupt_to(cleanup_depth, Terminator::Continue(target));
        let unreachable = self.cfg.alloc_block();
        self.start_unreachable_block(unreachable);
    }

    fn process_labeled(&mut self, labeled_stmt: &SyntaxNode) {
        let (Some(name), Some(body)) = (label_name(labeled_stmt), labeled_stmt.children().last())
        else {
            self.add_stmt(labeled_stmt);
            return;
        };

        let exit_block = self.cfg.alloc_block();
        let label_index = self.label_stack.len();
        self.label_stack.push(LabelTarget {
            name,
            jump: JumpTarget {
                break_target: exit_block,
                continue_target: None,
                cleanup_depth: self.cleanup_stack.len(),
            },
        });

        match body.kind() {
            SyntaxKind::WHILE_STMT => self.process_while(&body, Some(label_index)),
            SyntaxKind::FOR_STMT => self.process_for(&body, Some(label_index)),
            SyntaxKind::FOREACH_STMT => self.process_foreach(&body, Some(label_index)),
            SyntaxKind::DO_WHILE_STMT => self.process_do_while(&body, Some(label_index)),
            _ => self.process_statement(&body),
        }

        self.label_stack.pop();
        if self.current_reachable {
            self.finish_block(Terminator::Goto(exit_block));
        }
        self.start_block(exit_block);
    }

    fn set_label_continue_target(&mut self, label_index: Option<usize>, target: BlockId) {
        if let Some(label) = label_index.and_then(|index| self.label_stack.get_mut(index)) {
            label.jump.continue_target = Some(target);
        }
    }
}

fn label_name(statement: &SyntaxNode) -> Option<String> {
    statement
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|token| token.kind().is_identifier())
        .map(|token| token.text().to_string())
}

/// Check if a syntax kind is an expression
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
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::CONSTRUCTOR_REF_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::SWITCH_EXPR
            | SyntaxKind::CLASS_LITERAL_EXPR
    )
}

pub(crate) fn condition_expr_for_stmt(stmt: &SyntaxNode) -> Option<ExprRef> {
    // Find the condition expression between ( and ) in if/while/do-while.
    //
    // 3+ same-precedence operands parse as sibling continuation nodes
    // (`a && b && c` → `BINARY_EXPR [a, &&, b]` followed by
    // `BINARY_EXPR [&&, c]`, pinned by the parser's
    // `same_precedence_produces_sibling_binary_exprs`). The chain is
    // left-associative, so the LAST expression node before `)` is the head of
    // the whole condition — its left operand is the preceding sibling
    // For a single-node condition this is the node itself.
    let mut found_lparen = false;
    let mut last_expr: Option<SyntaxNode> = None;

    for child in stmt.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::L_PAREN && !found_lparen => {
                found_lparen = true;
            }
            rowan::NodeOrToken::Node(n) if found_lparen && is_expression(n.kind()) => {
                last_expr = Some(n.clone());
            }
            rowan::NodeOrToken::Token(t) if found_lparen && t.kind() == SyntaxKind::R_PAREN => {
                break;
            }
            _ => {}
        }
    }

    last_expr.map(ExprRef::new)
}

/// Split a `FOR_STMT` header into its three semicolon-separated sections.
///
/// Returns `(init_nodes, condition_head, update_nodes)` where the sections
/// are the direct child nodes between `(` and `)`, segmented by the header's
/// top-level `;` tokens. The condition is the LAST expression node of its
/// section — same-precedence chains parse as sibling continuation nodes whose
/// head is the last sibling (see `condition_expr_for_stmt`).
fn for_header_sections(
    for_stmt: &SyntaxNode,
) -> (Vec<SyntaxNode>, Option<SyntaxNode>, Vec<SyntaxNode>) {
    let mut found_lparen = false;
    let mut section = 0usize;
    let mut init_nodes = Vec::new();
    let mut condition_head: Option<SyntaxNode> = None;
    let mut update_nodes = Vec::new();

    for child in for_stmt.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::L_PAREN && !found_lparen => {
                found_lparen = true;
            }
            rowan::NodeOrToken::Token(t) if found_lparen && t.kind() == SyntaxKind::R_PAREN => {
                break;
            }
            rowan::NodeOrToken::Token(t) if found_lparen && t.kind() == SyntaxKind::SEMICOLON => {
                section += 1;
            }
            rowan::NodeOrToken::Node(n) if found_lparen => match section {
                0 => init_nodes.push(n.clone()),
                1 => {
                    if is_expression(n.kind()) {
                        condition_head = Some(n.clone());
                    }
                }
                _ => update_nodes.push(n.clone()),
            },
            _ => {}
        }
    }

    (init_nodes, condition_head, update_nodes)
}

/// Extract the enhanced-for variable and the state assigned at the start of
/// each iteration. Reference elements may be null even when the iterable is
/// non-null; primitive elements cannot be.
fn foreach_binding(foreach_stmt: &SyntaxNode) -> Option<Nullability> {
    let (_, header_end) = foreach_binding_header(foreach_stmt)?;

    let is_primitive = foreach_stmt
        .children()
        .any(|child| child.kind() == SyntaxKind::PRIMITIVE_TYPE)
        && !foreach_stmt
            .children()
            .any(|child| child.kind() == SyntaxKind::ARRAY_TYPE);
    let annotations: Vec<String> = foreach_stmt
        .children()
        .filter(|child| {
            child.kind() == SyntaxKind::ANNOTATION
                && u32::from(child.text_range().end()) <= header_end
        })
        .filter_map(|annotation| annotation_name(&annotation))
        .collect();
    Some(if is_primitive {
        Nullability::NonNull
    } else if has_nonnull_annotation(&annotations) {
        Nullability::NonNull
    } else if has_nullable_annotation(&annotations) {
        Nullability::Nullable
    } else {
        Nullability::Unknown
    })
}

pub(super) fn foreach_binding_name(foreach_stmt: &SyntaxNode) -> Option<String> {
    foreach_binding_header(foreach_stmt).map(|(name, _)| name)
}

/// Extract a named switch-pattern binding and its state after a successful
/// match. Top-level type patterns never match null. A type pattern nested in a
/// record pattern can be *null-matching* when it is unconditional for the
/// record component type; without that type relation here, Unknown is the
/// sound state.
pub(super) fn switch_pattern_binding(pattern: &SyntaxNode) -> Option<(String, Nullability)> {
    if pattern.kind() != SyntaxKind::TYPE_PATTERN {
        return None;
    }
    let name = pattern
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier())
        .map(|token| token.text().to_string())
        .find(|name| name != "_")?;
    let case = pattern
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.kind() == SyntaxKind::CASE_CLAUSE)?;
    let nested_in_record = pattern
        .ancestors()
        .skip(1)
        .take_while(|ancestor| ancestor != &case)
        .any(|ancestor| ancestor.kind() == SyntaxKind::RECORD_PATTERN);
    let is_scalar_primitive = pattern
        .children()
        .any(|child| child.kind() == SyntaxKind::PRIMITIVE_TYPE)
        && !pattern
            .children()
            .any(|child| child.kind() == SyntaxKind::ARRAY_TYPE);
    let is_inferred = pattern
        .children()
        .any(|child| child.kind() == SyntaxKind::VAR_TYPE);
    Some((
        name,
        if is_inferred || (nested_in_record && !is_scalar_primitive) {
            Nullability::Unknown
        } else {
            Nullability::NonNull
        },
    ))
}

/// Named bindings declared by the case label itself, excluding patterns that
/// occur later in a `when` guard or the arm body.
pub(super) fn switch_label_bindings(clause: &SyntaxNode) -> Vec<(SyntaxNode, Nullability)> {
    let Some(separator_start) = clause.children_with_tokens().find_map(|element| {
        matches!(element.kind(), SyntaxKind::COLON | SyntaxKind::ARROW)
            .then(|| element.text_range().start())
    }) else {
        return Vec::new();
    };
    clause
        .descendants()
        .filter(|node| {
            node.kind() == SyntaxKind::TYPE_PATTERN
                && node.text_range().end() <= separator_start
                && !node
                    .ancestors()
                    .skip(1)
                    .take_while(|ancestor| ancestor != clause)
                    .any(|ancestor| ancestor.kind() == SyntaxKind::WHEN_CLAUSE)
        })
        .filter_map(|node| {
            switch_pattern_binding(&node).map(|(_, nullability)| (node, nullability))
        })
        .collect()
}

pub(super) fn switch_guard_condition(clause: &SyntaxNode) -> Option<ExprRef> {
    clause
        .children()
        .find(|child| child.kind() == SyntaxKind::WHEN_CLAUSE)
        .and_then(|when_clause| {
            when_clause
                .children()
                .filter(|child| is_expression(child.kind()))
                .last()
                .map(ExprRef::new)
        })
}

fn foreach_binding_header(foreach_stmt: &SyntaxNode) -> Option<(String, u32)> {
    let mut in_header = false;
    let mut variable_name = None;
    let mut header_end = None;

    for child in foreach_stmt.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::L_PAREN => {
                in_header = true;
            }
            rowan::NodeOrToken::Token(token) if in_header && token.kind() == SyntaxKind::COLON => {
                header_end = Some(token.text_range().start().into());
                break;
            }
            rowan::NodeOrToken::Token(token) if in_header && token.kind().is_identifier() => {
                variable_name = Some(token.text().to_string());
            }
            _ => {}
        }
    }
    Some((variable_name?, header_end?))
}

/// Extract the exception type from a catch clause.
///
/// Returns the type name (e.g., "IOException", "Exception") from patterns like:
/// - `catch (IOException e) { ... }`
/// - `catch (Exception | RuntimeException e) { ... }` (multi-catch)
///
/// The catch clause structure is:
/// CATCH_CLAUSE
///   - CATCH_KW
///   - L_PAREN
///   - [FINAL_KW] (optional)
///   - CLASS_TYPE (the exception type)
///   - [PIPE + CLASS_TYPE]* (for multi-catch)
///   - IDENT (the variable name)
///   - R_PAREN
///   - BLOCK_STMT
fn extract_catch_exception_types(
    catch_clause: &SyntaxNode,
    context: &ExceptionTypeContext,
) -> Vec<String> {
    let body_start = catch_clause
        .children()
        .find(|child| child.kind() == SyntaxKind::BLOCK_STMT)
        .map(|body| body.text_range().start());
    catch_clause
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::CLASS_TYPE)
        .take_while(|node| body_start.is_none_or(|start| node.text_range().start() < start))
        .map(|node| {
            let written = node.text().to_string().trim().to_owned();
            context.resolve_type_at(&node, &written).unwrap_or(written)
        })
        .collect()
}

pub(crate) fn catch_binding_name(catch_clause: &SyntaxNode) -> Option<String> {
    let mut in_parameters = false;
    let mut name = None;
    for element in catch_clause.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::L_PAREN => {
                in_parameters = true;
            }
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::R_PAREN => break,
            rowan::NodeOrToken::Token(token) if in_parameters && token.kind().is_identifier() => {
                name = Some(token.text().to_string());
            }
            _ => {}
        }
    }
    name
}

pub(crate) fn explicit_thrown_type(expression: &SyntaxNode) -> Option<String> {
    let constructed = match expression.kind() {
        SyntaxKind::NEW_EXPR => expression.clone(),
        SyntaxKind::PAREN_EXPR => {
            let value = expression
                .children()
                .filter(|child| is_expression(child.kind()))
                .last()?;
            return explicit_thrown_type(&value);
        }
        _ => return None,
    };
    constructed
        .descendants()
        .find(|node| node.kind() == SyntaxKind::CLASS_TYPE)
        .map(|node| node.text().to_string().trim().to_string())
}

/// Best-effort compatibility for syntactically known exception types. `None`
/// means the relationship needs workspace type information and must remain a
/// possible match. The explicit JDK cases let sibling catches retain Java's
/// first-compatible semantics without pretending arbitrary source types are
/// unrelated.
fn known_exception_assignability(
    thrown: &str,
    caught: &str,
    context: &ExceptionTypeContext,
) -> Option<bool> {
    if thrown == caught {
        return Some(true);
    }
    if let Some(assignable) = context.source_is_subtype(thrown, caught) {
        return Some(assignable);
    }
    let thrown_source = context.resolve_source_type(thrown).is_some();
    let caught_source = context.resolve_source_type(caught).is_some();
    let thrown_jdk = java_exception_name(thrown, context).or_else(|| {
        context
            .source_superclass_frontier(thrown)
            .and_then(|frontier| java_exception_name(frontier, context))
    });
    let caught_jdk = java_exception_name(caught, context);

    let thrown_external = context.is_resolved_external_type(thrown);
    let caught_external = context.is_resolved_external_type(caught);
    if thrown_jdk.is_some() && caught_external {
        // The fixed JDK hierarchy cannot acquire an external superclass.
        return Some(false);
    }
    if thrown_external && caught_jdk.is_some() {
        // The external type may extend the JDK catch.
        return None;
    }
    if thrown_external && caught_external {
        return (thrown == caught).then_some(true);
    }

    if let (Some(thrown), Some(caught)) = (thrown_jdk, caught_jdk) {
        return Some(jdk_exception_is_assignable_to(thrown, caught));
    }
    if thrown_source && caught_jdk.is_some() {
        // An unresolved external superclass may still lead to the JDK catch.
        return None;
    }
    if thrown_jdk.is_some() && caught_source {
        // java.lang's fixed hierarchy cannot acquire a source-local superclass.
        return Some(false);
    }
    None
}

fn java_exception_name<'a>(name: &'a str, context: &ExceptionTypeContext) -> Option<&'a str> {
    let (simple, explicitly_jdk) = if let Some(simple) = name.strip_prefix("java.lang.") {
        (simple, true)
    } else if name == "java.io.IOException" {
        ("IOException", true)
    } else if !name.contains('.') {
        (name, false)
    } else {
        return None;
    };
    let known = simple == "Throwable" || jdk_exception_direct_supertype(simple).is_some();
    (known && (explicitly_jdk || !context.shadows_java_lang(simple))).then_some(simple)
}

fn jdk_exception_is_assignable_to(thrown: &str, caught: &str) -> bool {
    let mut current = Some(thrown);
    while let Some(candidate) = current {
        if candidate == caught {
            return true;
        }
        current = jdk_exception_direct_supertype(candidate);
    }
    false
}

fn jdk_exception_direct_supertype(exception: &str) -> Option<&'static str> {
    match exception {
        "Exception" | "Error" => Some("Throwable"),
        "AssertionError" => Some("Error"),
        "RuntimeException" | "IOException" => Some("Exception"),
        "IndexOutOfBoundsException" => Some("RuntimeException"),
        "ArrayIndexOutOfBoundsException" => Some("IndexOutOfBoundsException"),
        "ArithmeticException"
        | "ArrayStoreException"
        | "ClassCastException"
        | "IllegalArgumentException"
        | "IllegalStateException"
        | "NegativeArraySizeException"
        | "NullPointerException"
        | "SecurityException"
        | "UnsupportedOperationException" => Some("RuntimeException"),
        _ => None,
    }
}

fn is_unshadowed_java_lang_reference_type(name: &str, context: &ExceptionTypeContext) -> bool {
    let (simple, explicitly_java_lang) = name
        .strip_prefix("java.lang.")
        .map_or((name, false), |simple| (simple, true));
    matches!(simple, "Object" | "String")
        && (explicitly_java_lang || (!name.contains('.') && !context.shadows_java_lang(simple)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};

    fn build_cfg(code: &str) -> ControlFlowGraph {
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);
        // Find the method body
        let method_body = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::BLOCK_STMT)
            .expect("No block found");
        ControlFlowGraph::build(&method_body)
    }

    #[test]
    fn cfg_operations_remain_compact() {
        assert!(
            std::mem::size_of::<StatementRef>() <= 16,
            "rare binding metadata must not inflate every CFG operation: {} bytes",
            std::mem::size_of::<StatementRef>()
        );
        assert!(
            std::mem::size_of::<ExprRef>() <= 8,
            "expression references should remain one shared syntax handle: {} bytes",
            std::mem::size_of::<ExprRef>()
        );
        assert!(
            std::mem::size_of::<Terminator>() <= 40,
            "value exits must not enlarge every basic-block terminator: {} bytes",
            std::mem::size_of::<Terminator>()
        );
        assert!(
            std::mem::size_of::<BasicBlock>() <= 80,
            "shared scope IDs should keep basic blocks compact: {} bytes",
            std::mem::size_of::<BasicBlock>()
        );
        assert!(
            std::mem::size_of::<HandlerScope>() <= 40,
            "one arena handler group should remain compact: {} bytes",
            std::mem::size_of::<HandlerScope>()
        );
        assert!(
            std::mem::size_of::<CleanupScope>() <= 16,
            "cleanup kind and links should stay in one compact arena record: {} bytes",
            std::mem::size_of::<CleanupScope>()
        );
        assert_eq!(
            std::mem::size_of::<ExceptionTypeRevision>(),
            std::mem::size_of::<usize>(),
            "the shared exception revision should remain one optional Arc"
        );
    }

    #[test]
    fn nested_finally_cfg_growth_is_linear_and_each_body_is_lowered_once() {
        for depth in [1_usize, 4, 8, 16] {
            let mut body = "return;".to_string();
            for index in 0..depth {
                body = format!("try {{ {body} }} finally {{ cleanup{index}(); }}");
            }
            let cfg = build_cfg(&format!("class A {{ void test() {{ {body} }} }}"));
            assert_eq!(
                cfg.cleanup_scopes.len(),
                depth,
                "one cleanup arena entry per source finally"
            );
            assert!(
                cfg.blocks.len() <= depth * 6 + 6,
                "nested finally CFG must grow linearly: depth {depth}, {} blocks",
                cfg.blocks.len()
            );
            for index in 0..depth {
                let marker = format!("cleanup{index}");
                let copies = cfg
                    .blocks
                    .values()
                    .flat_map(|block| &block.statements)
                    .filter(|statement| {
                        statement.node().text().to_string().trim() == format!("{marker}();")
                    })
                    .count();
                assert_eq!(copies, 1, "{marker} was lowered {copies} times");
            }
        }
    }

    #[test]
    fn test_simple_method() {
        let cfg = build_cfg("class A { void test() { int x = 1; } }");
        assert!(cfg.blocks.contains_key(&cfg.entry));
        assert!(cfg.blocks.contains_key(&cfg.exit));
    }

    #[test]
    fn test_if_statement() {
        let cfg = build_cfg("class A { void test(boolean b) { if (b) { x(); } else { y(); } } }");
        // Entry + then + else + merge + exit = at least 5 blocks
        assert!(cfg.blocks.len() >= 4);
    }

    #[test]
    fn test_while_loop() {
        let cfg = build_cfg("class A { void test() { while (true) { x(); } } }");
        // Should have entry, header, body, exit blocks
        assert!(cfg.blocks.len() >= 3);
    }

    #[test]
    fn test_return_terminates() {
        let cfg = build_cfg("class A { void test() { return; x(); } }");
        // The x() should be in unreachable block
        let entry_block = cfg.blocks.get(&cfg.entry).unwrap();
        assert!(matches!(entry_block.terminator, Terminator::Return(_)));
    }

    #[test]
    fn test_predecessors_and_successors() {
        let cfg = build_cfg("class A { void test(boolean b) { if (b) { return; } } }");
        // Entry should have successors
        let entry_succs = cfg.successors(cfg.entry);
        assert!(!entry_succs.is_empty());

        // Check that predecessor/successor relationship is symmetric
        for block_id in cfg.blocks.keys() {
            let succs = cfg.successors(*block_id);
            for succ in succs {
                let preds = cfg.predecessors(succ);
                assert!(preds.contains(block_id));
            }
        }
    }

    #[test]
    fn all_successors_and_predecessors_are_symmetric_for_catch_edges() {
        let cfg = build_cfg(
            r#"
            class A {
                void test() {
                    try {
                        risky();
                    } catch (Exception e) {
                        recover();
                    }
                }
            }
            "#,
        );

        let catch_target = cfg
            .blocks
            .values()
            .find_map(|block| {
                block
                    .handler_scope
                    .and_then(|scope| cfg.handler_scope(scope).catches.first())
                    .map(|catch| catch.target)
            })
            .expect("try statement should install a catch destination");
        let protected_blocks: Vec<_> = cfg
            .blocks
            .values()
            .filter(|block| {
                block.handler_scope.is_some_and(|scope| {
                    cfg.handler_scope(scope)
                        .catches
                        .iter()
                        .any(|catch| catch.target == catch_target)
                })
            })
            .map(|block| block.id)
            .collect();
        assert!(
            !protected_blocks.is_empty(),
            "catch destination should protect at least one block"
        );

        let catch_predecessors = cfg.predecessors(catch_target);
        for protected in protected_blocks {
            assert!(
                cfg.all_successors(protected).contains(&catch_target),
                "protected block {protected:?} should expose its catch destination"
            );
            assert!(
                catch_predecessors.contains(&protected),
                "catch predecessor view should contain protected block {protected:?}"
            );
        }
    }

    #[test]
    fn test_for_loop() {
        let cfg = build_cfg("class A { void test() { for (int i = 0; i < 10; i++) { x(); } } }");
        // Entry + init + header + body + update + exit
        assert!(cfg.blocks.len() >= 5);
    }

    #[test]
    fn test_break_in_loop() {
        let cfg = build_cfg("class A { void test() { while (true) { if (x()) break; } } }");
        // Should have blocks for: entry, header, body, break, exit
        assert!(cfg.blocks.len() >= 4);
    }

    #[test]
    fn test_continue_in_loop() {
        let cfg = build_cfg("class A { void test() { while (true) { if (x()) continue; y(); } } }");
        // Should have blocks for: entry, header, body, continue target
        assert!(cfg.blocks.len() >= 4);
    }

    #[test]
    fn labeled_block_break_reaches_the_following_statement() {
        let cfg = build_cfg("class A { void test() { scope: { break scope; } afterScope(); } }");
        let after_scope = cfg
            .blocks
            .values()
            .find(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement.node().text().to_string().contains("afterScope"))
            })
            .expect("statement after the labeled block");
        let labeled_break_target = cfg
            .blocks
            .values()
            .find_map(|block| match block.terminator {
                Terminator::Break(target) => Some(target),
                _ => None,
            })
            .expect("labeled break");

        assert_eq!(labeled_break_target, after_scope.id);
    }

    #[test]
    fn nested_break_targets_its_named_outer_loop() {
        let cfg = build_cfg(
            r#"
            class A {
                void test(boolean outerCondition, boolean innerCondition) {
                    outer: while (outerCondition) {
                        while (innerCondition) {
                            break outer;
                        }
                        afterInner();
                    }
                    afterOuter();
                }
            }
            "#,
        );
        let after_outer = cfg
            .blocks
            .values()
            .find(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement.node().text().to_string().contains("afterOuter"))
            })
            .expect("statement after the outer loop");
        let labeled_break_target = cfg
            .blocks
            .values()
            .find_map(|block| match block.terminator {
                Terminator::Break(target) if target == after_outer.id => Some(target),
                _ => None,
            });

        assert_eq!(labeled_break_target, Some(after_outer.id));
    }

    #[test]
    fn nested_continue_targets_the_named_for_update() {
        let cfg = build_cfg(
            r#"
            class A {
                void test(boolean outerCondition, boolean innerCondition) {
                    outer: for (; outerCondition; tick()) {
                        while (innerCondition) {
                            continue outer;
                        }
                        afterInner();
                    }
                }
            }
            "#,
        );
        let update = cfg
            .blocks
            .values()
            .find(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement.node().text().to_string().contains("tick"))
            })
            .expect("outer for update block");
        let labeled_continue_target =
            cfg.blocks
                .values()
                .find_map(|block| match block.terminator {
                    Terminator::Continue(target) => Some(target),
                    _ => None,
                });

        assert_eq!(labeled_continue_target, Some(update.id));
    }

    #[test]
    fn continue_inside_switch_targets_the_enclosing_loop() {
        let cfg = build_cfg(
            r#"
            class A {
                void test(boolean repeat, int value) {
                    while (repeat) {
                        switch (value) {
                            default: continue;
                        }
                        afterSwitch();
                    }
                }
            }
            "#,
        );
        let continue_target = cfg
            .blocks
            .values()
            .find_map(|block| match block.terminator {
                Terminator::Continue(target) => Some(target),
                _ => None,
            })
            .expect("continue in switch");

        assert!(matches!(
            cfg.blocks
                .get(&continue_target)
                .map(|block| &block.terminator),
            Some(Terminator::Branch { .. })
        ));
    }

    #[test]
    fn test_switch_statement() {
        let cfg = build_cfg(
            "class A { void test(int x) { switch (x) { case 1: a(); break; case 2: b(); break; default: c(); } } }",
        );
        // Entry + switch + case1 + case2 + default + merge + exit
        assert!(cfg.blocks.len() >= 5);
    }

    #[test]
    fn switch_without_default_dispatches_unmatched_values_to_the_merge() {
        let cfg = build_cfg(
            "class A { void test(int x) { switch (x) { case 1: hit(); } afterSwitch(); } }",
        );
        let merge =
            cfg.blocks
                .values()
                .find(|block| {
                    block.statements.iter().any(|statement| {
                        statement.node().text().to_string().contains("afterSwitch")
                    })
                })
                .expect("switch merge containing the following statement");
        let unmatched = cfg
            .blocks
            .values()
            .find_map(|block| match &block.terminator {
                Terminator::Switch { default, .. } => *default,
                _ => None,
            })
            .expect("unmatched switch target");

        assert_eq!(unmatched, merge.id);
    }

    #[test]
    fn colon_switch_arm_falls_through_to_the_next_arm() {
        let cfg = build_cfg(
            "class A { void test(int x) { switch (x) { case 1: first(); case 2: second(); } } }",
        );
        let first = cfg
            .blocks
            .values()
            .find(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement.node().text().to_string().contains("first"))
            })
            .expect("first case block");
        let second = cfg
            .blocks
            .values()
            .find(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement.node().text().to_string().contains("second"))
            })
            .expect("second case block");

        assert!(matches!(first.terminator, Terminator::Goto(target) if target == second.id));
    }

    #[test]
    fn pattern_switch_arms_keep_distinct_syntax_identity() {
        let cfg = build_cfg(
            r#"
            class A {
                void test(Object value) {
                    switch (value) {
                        case String text: stringArm(); break;
                        case Integer number: integerArm(); break;
                        default: fallback();
                    }
                }
            }
            "#,
        );

        for marker in ["stringArm", "integerArm", "fallback"] {
            let count = cfg
                .blocks
                .values()
                .filter(|block| {
                    block
                        .statements
                        .iter()
                        .any(|statement| statement.node().text().to_string().contains(marker))
                })
                .count();
            assert_eq!(count, 1, "{marker} must be lowered exactly once");
        }
    }

    #[test]
    fn guarded_pattern_branches_before_its_body() {
        let cfg = build_cfg(
            r#"
            class A {
                void test(Object value, Service service) {
                    switch (value) {
                        case String text when service.isReady() -> guardedArm();
                        default -> fallback();
                    }
                }
            }
            "#,
        );
        let (condition, guarded_body) = cfg
            .blocks
            .values()
            .find_map(|block| match &block.terminator {
                Terminator::Branch {
                    condition,
                    then_block,
                    ..
                } if condition
                    .node
                    .text()
                    .to_string()
                    .contains("service.isReady") =>
                {
                    Some((condition, then_block))
                }
                _ => None,
            })
            .expect("guard branch");
        let body = cfg.blocks.get(guarded_body).expect("guarded body");

        assert!(condition
            .node
            .text()
            .to_string()
            .contains("service.isReady"));
        assert!(body.statements.iter().any(|statement| statement
            .node()
            .text()
            .to_string()
            .contains("guardedArm")));
    }

    #[test]
    fn pattern_binding_precedes_its_guard() {
        let cfg = build_cfg(
            r#"
            class A {
                void test(Object value) {
                    switch (value) {
                        case String text when text.isEmpty() -> use(text);
                        default -> {}
                    }
                }
            }
            "#,
        );
        let guard_block = cfg
            .blocks
            .values()
            .find(|block| matches!(&block.terminator, Terminator::Branch { condition, .. } if condition.node.text().to_string().contains("text.isEmpty")))
            .expect("guard block");

        assert!(matches!(
            guard_block.statements.as_slice(),
            [StatementRef::Bind {
                nullability: Nullability::NonNull,
                ..
            }]
        ));
    }

    #[test]
    fn nested_record_component_binding_is_not_assumed_nonnull() {
        let cfg = build_cfg(
            r#"
            record Pair(String value) {}
            class A {
                void test(Object candidate) {
                    switch (candidate) {
                        case Pair(String value) -> use(value);
                        default -> {}
                    }
                }
            }
            "#,
        );
        let component_binding = cfg
            .blocks
            .values()
            .flat_map(|block| &block.statements)
            .find(|statement| {
                matches!(statement, StatementRef::Bind { declaration, .. } if declaration.text().to_string().contains("String value"))
            })
            .expect("record component binding");

        assert!(matches!(
            component_binding,
            StatementRef::Bind {
                nullability: Nullability::Unknown,
                ..
            }
        ));
    }

    #[test]
    fn record_component_bindings_distinguish_primitive_reference_and_unnamed() {
        let cfg = build_cfg(
            r#"
            record Result(int count, String text) {}
            class A {
                void test(Object candidate) {
                    switch (candidate) {
                        case Result(int count, String text) -> use(count, text);
                        case Result(int _, var _) -> {}
                        default -> {}
                    }
                }
            }
            "#,
        );
        let bindings: Vec<_> = cfg
            .blocks
            .values()
            .flat_map(|block| &block.statements)
            .filter_map(|statement| match statement {
                StatementRef::Bind {
                    declaration,
                    nullability,
                } => Some((declaration.text().to_string(), *nullability)),
                _ => None,
            })
            .collect();

        assert_eq!(
            bindings.len(),
            2,
            "unnamed patterns must not create bindings"
        );
        assert!(bindings.contains(&("int count".to_string(), Nullability::NonNull)));
        assert!(bindings.contains(&("String text".to_string(), Nullability::Unknown)));
    }

    #[test]
    fn nested_default_does_not_reclassify_the_enclosing_case() {
        let cfg = build_cfg(
            r#"
            class A {
                void test(int outer, int inner) {
                    switch (outer) {
                        case 1:
                            switch (inner) { default: nested(); }
                            outerArm();
                            break;
                        default: fallback();
                    }
                }
            }
            "#,
        );
        let outer_switch = cfg
            .blocks
            .values()
            .find(|block| matches!(&block.terminator, Terminator::Switch { cases, .. } if !cases.is_empty()))
            .expect("outer switch");

        assert!(matches!(
            &outer_switch.terminator,
            Terminator::Switch { cases, default: Some(default), .. }
                if cases.len() == 1 && cases[0].1 != *default
        ));
    }

    #[test]
    fn test_try_catch_finally() {
        let cfg = build_cfg(
            "class A { void test() { try { a(); } catch (Exception e) { b(); } finally { c(); } } }",
        );
        // Entry + try + catch + finally + merge + exit
        assert!(cfg.blocks.len() >= 4);
    }

    #[test]
    fn test_nested_if() {
        let cfg =
            build_cfg("class A { void test(boolean a, boolean b) { if (a) { if (b) { x(); } } } }");
        // Multiple blocks for nested control flow
        assert!(cfg.blocks.len() >= 5);
    }

    #[test]
    fn test_do_while() {
        let cfg = build_cfg("class A { void test() { do { x(); } while (true); } }");
        // Entry + body + condition + exit
        assert!(cfg.blocks.len() >= 3);
    }

    // =========================================================================
    // Exception edge tests
    // =========================================================================

    #[test]
    fn test_try_catch_separate_blocks() {
        let cfg = build_cfg(
            r#"
            class A {
                void test() {
                    try {
                        risky();
                    } catch (IOException e) {
                        handleIO();
                    } catch (Exception e) {
                        handleGeneric();
                    }
                }
            }
        "#,
        );
        // Should have: entry, try, catch_io, catch_generic, merge, exit
        assert!(
            cfg.blocks.len() >= 5,
            "Expected at least 5 blocks, got {}",
            cfg.blocks.len()
        );

        // The protected block references one ordered handler group rather than
        // cloning both catches into every block.
        let blocks_with_exceptions: Vec<_> = cfg
            .blocks
            .values()
            .filter(|b| b.handler_scope.is_some())
            .collect();

        assert!(
            !blocks_with_exceptions.is_empty(),
            "try block should have exception edges"
        );

        let handler = cfg.handler_scope(
            blocks_with_exceptions[0]
                .handler_scope
                .expect("protected block must name its handler"),
        );
        assert_eq!(
            handler.catches.len(),
            2,
            "the handler group should retain both ordered catch arms"
        );
    }

    #[test]
    fn test_try_catch_exception_edge_types() {
        let cfg = build_cfg(
            r#"
            class A {
                void test() {
                    try {
                        risky();
                    } catch (IOException e) {
                        handle();
                    }
                }
            }
        "#,
        );

        // Find a protected block and inspect its shared handler group.
        let try_block = cfg
            .blocks
            .values()
            .find(|b| b.handler_scope.is_some())
            .expect("should have a protected block");
        let handler = cfg.handler_scope(try_block.handler_scope.unwrap());

        assert_eq!(handler.catches.len(), 1);
        assert!(
            handler.catches[0].exception_types[0].contains("IOException"),
            "Exception type should be IOException, got: {}",
            handler.catches[0].exception_types[0]
        );
    }

    #[test]
    fn test_catch_edges_begin_at_the_protected_region() {
        let cfg = build_cfg(
            r#"
            class A {
                void test() {
                    before();
                    try {
                        risky();
                    } catch (IOException e) {
                        handle();
                    }
                }
            }
        "#,
        );

        let before_block = cfg
            .blocks
            .values()
            .find(|block| {
                block
                    .statements
                    .iter()
                    .any(|statement| statement.node().text().to_string().contains("before"))
            })
            .expect("the pre-try call should be represented in a block");
        assert!(
            before_block.handler_scope.is_none(),
            "statements before a try are outside its protected region"
        );
    }

    #[test]
    fn test_try_finally_all_paths() {
        let cfg = build_cfg(
            r#"
            class A {
                void test() {
                    try {
                        risky();
                    } finally {
                        cleanup();
                    }
                }
            }
        "#,
        );

        // Should have blocks: entry, try, finally, merge, exit
        assert!(
            cfg.blocks.len() >= 4,
            "Expected at least 4 blocks, got {}",
            cfg.blocks.len()
        );

        // Try block should go to finally on normal exit
        // (No catch means no exception edges, but finally should be reachable)
    }

    #[test]
    fn test_nested_try_catch() {
        let cfg = build_cfg(
            r#"
            class A {
                void test() {
                    try {
                        try {
                            inner();
                        } catch (RuntimeException e) {
                            handleInner();
                        }
                    } catch (Exception e) {
                        handleOuter();
                    }
                }
            }
        "#,
        );

        // Inner try should have exception edges to inner catch
        // Outer try should have exception edges to outer catch
        // Both try blocks should exist
        assert!(
            cfg.blocks.len() >= 6,
            "Expected at least 6 blocks for nested try-catch"
        );

        let protected_handler_count = cfg
            .blocks
            .values()
            .filter_map(|b| b.handler_scope)
            .collect::<HashSet<_>>()
            .len();

        assert!(
            protected_handler_count >= 2,
            "Should have at least 2 shared handler scopes (inner and outer try)"
        );
    }

    #[test]
    fn test_catch_blocks_are_predecessors() {
        let cfg = build_cfg(
            r#"
            class A {
                void test() {
                    try {
                        risky();
                    } catch (Exception e) {
                        handle();
                    }
                }
            }
        "#,
        );

        // Find the catch block (has a predecessor that has it as an exception edge target)
        let catch_block_id = cfg
            .blocks
            .values()
            .find_map(|block| {
                block
                    .handler_scope
                    .and_then(|scope| cfg.handler_scope(scope).catches.first())
                    .map(|catch| catch.target)
            })
            .expect("should have a catch arm");

        // The catch block should have predecessors (the try block via exception edge)
        let preds = cfg.predecessors(catch_block_id);
        assert!(
            !preds.is_empty(),
            "catch block should have predecessors via exception edges"
        );
    }
}
