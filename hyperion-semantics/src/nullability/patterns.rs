// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Null check pattern recognition for narrowing

use super::cfg::ExprRef;
use super::environment::VariableId;
use super::stable_access_path;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Compact Java value category needed by flow operations whose syntax implies
/// boxing or unboxing. Unknown remains conservative; callers must not invent
/// an implicit dereference without a known boxed source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[doc(hidden)]
pub enum ValueKind {
    #[default]
    Unknown,
    Reference,
    StringReference,
    PrimitiveBoolean,
    /// Integral primitive or `char`.
    PrimitiveNumeric,
    PrimitiveFloating,
    BoxedBoolean,
    /// Integral wrapper or `Character`.
    BoxedNumeric,
    BoxedFloating,
}

/// Recognized null check conditions
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum NullCheckCondition {
    /// `x == null`
    IsNull(VariableId),

    /// `x != null`
    IsNonNull(VariableId),

    /// `cond1 && cond2`
    And(Box<Self>, Box<Self>),

    /// `cond1 || cond2`
    Or(Box<Self>, Box<Self>),

    /// `!condition`
    Not(Box<Self>),

    /// Successful `selector instanceof Type binding`. The fresh binding is
    /// definitely non-null, and a stable selector path is non-null as well.
    /// On the false path the binding is out of scope rather than null.
    TypePattern {
        binding: VariableId,
        selector: Option<VariableId>,
    },

    /// A sub-condition that carries no nullability information — e.g. a plain
    /// boolean operand in a compound condition (`flag`, `x.isReady()`). It
    /// narrows nothing, but lets the recognizable operands of an `&&`/`||`
    /// still contribute, so `x != null && x.isReady()` narrows `x`.
    Top,
}

impl NullCheckCondition {
    /// Negate the condition
    pub fn negate(self) -> Self {
        match self {
            Self::IsNull(v) => Self::IsNonNull(v),
            Self::IsNonNull(v) => Self::IsNull(v),
            Self::And(l, r) => Self::Or(Box::new(l.negate()), Box::new(r.negate())),
            Self::Or(l, r) => Self::And(Box::new(l.negate()), Box::new(r.negate())),
            Self::Not(inner) => *inner,
            pattern @ Self::TypePattern { .. } => Self::Not(Box::new(pattern)),
            Self::Top => Self::Top,
        }
    }
}

/// Context for variable resolution during pattern analysis
pub trait VariableResolver {
    /// Resolve a variable name to its ID
    fn resolve_variable(&self, name: &str) -> Option<VariableId>;

    /// Resolve a variable use or declaration by its syntax identity.
    ///
    /// The node-aware form distinguishes nested access paths that begin at
    /// the same byte offset (`service` and `service.child`) and lexical
    /// bindings that shadow a same-named field. Name-only resolvers may keep
    /// the default implementation.
    fn resolve_variable_node(&self, name: &str, _node: &SyntaxNode) -> Option<VariableId> {
        self.resolve_variable(name)
    }

    /// Resolve one declarator within a declaration that may introduce several
    /// variables. The byte offset distinguishes `a` and `b` in `Object a, b`;
    /// name-only resolvers retain their existing behavior.
    fn resolve_declarator(
        &self,
        name: &str,
        declaration: &SyntaxNode,
        _name_offset: u32,
    ) -> Option<VariableId> {
        self.resolve_variable_node(name, declaration)
    }

    /// Whether this ID denotes a lexical binding rather than a field-like
    /// name. Used when a local shadows a field carrying declaration contracts.
    fn is_scoped_binding(&self, _variable: VariableId) -> bool {
        false
    }

    /// Resolve an ID back to its source name when available.
    fn variable_name(&self, _variable: VariableId) -> Option<&str> {
        None
    }

    /// Declared Java value category for this binding, when known.
    fn value_kind(&self, _variable: VariableId) -> ValueKind {
        ValueKind::Unknown
    }

    /// Declared Java type spelling for this binding, when known.
    fn declared_type_name(&self, _variable: VariableId) -> Option<&str> {
        None
    }

    /// Variables whose stable access paths depend on `prefix`.
    fn dependent_variables(&self, _prefix: &str) -> Vec<VariableId> {
        Vec::new()
    }

    /// Variables whose stable access paths depend on the resolved variable.
    ///
    /// Production resolvers override this to keep invalidation within the
    /// lexical binding that owns the path. The default preserves the older
    /// name-based behavior for simple resolvers.
    fn dependent_variables_for(&self, variable: VariableId) -> Vec<VariableId> {
        self.variable_name(variable)
            .map_or_else(Vec::new, |name| self.dependent_variables(name))
    }
}

/// Simple resolver that maps names to IDs
#[derive(Default)]
pub struct SimpleResolver {
    variables: std::collections::HashMap<String, VariableId>,
    next_id: u32,
}

impl SimpleResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn declare(&mut self, name: &str) -> VariableId {
        let id = VariableId(self.next_id);
        self.next_id += 1;
        self.variables.insert(name.to_string(), id);
        id
    }
}

impl VariableResolver for SimpleResolver {
    fn resolve_variable(&self, name: &str) -> Option<VariableId> {
        self.variables.get(name).copied()
    }

    fn dependent_variables(&self, prefix: &str) -> Vec<VariableId> {
        let dot_prefix = format!("{prefix}.");
        let index_prefix = format!("{prefix}[");
        self.variables
            .iter()
            .filter_map(|(name, id)| {
                (name.starts_with(&dot_prefix) || name.starts_with(&index_prefix)).then_some(*id)
            })
            .collect()
    }

    fn variable_name(&self, variable: VariableId) -> Option<&str> {
        self.variables
            .iter()
            .find_map(|(name, id)| (*id == variable).then_some(name.as_str()))
    }
}

/// Analyze an expression to extract null check conditions
pub fn analyze_condition<R: VariableResolver>(
    expr: &ExprRef,
    resolver: &R,
) -> Option<NullCheckCondition> {
    analyze_node(&expr.node, resolver, &|_| None)
}

pub(crate) fn analyze_condition_with_call_owners<R, F>(
    expr: &ExprRef,
    resolver: &R,
    call_owner: &F,
) -> Option<NullCheckCondition>
where
    R: VariableResolver,
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    analyze_node(&expr.node, resolver, call_owner)
}

fn analyze_node<R, F>(node: &SyntaxNode, resolver: &R, call_owner: &F) -> Option<NullCheckCondition>
where
    R: VariableResolver,
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    match node.kind() {
        SyntaxKind::BINARY_EXPR => analyze_binary(node, resolver, call_owner),
        SyntaxKind::UNARY_EXPR => analyze_unary(node, resolver, call_owner),
        SyntaxKind::PAREN_EXPR => {
            // Unwrap parentheses: the inner expression is a direct child. A
            // same-precedence chain inside the parens is a run of sibling
            // nodes whose LAST node is the chain head (left-associative, its
            // left operand is the preceding sibling), so take the last
            // expression child.
            node.children()
                .filter(|c| is_expression(c.kind()))
                .last()
                .and_then(|inner| analyze_node(&inner, resolver, call_owner))
        }
        SyntaxKind::METHOD_CALL_EXPR => analyze_call(node, resolver, call_owner),
        SyntaxKind::PATTERN_INSTANCEOF_EXPR => analyze_type_pattern(node, resolver),
        _ => {
            // For non-expression nodes, search direct children for an
            // analyzable expression.
            node.children()
                .find_map(|child| analyze_node(&child, resolver, call_owner))
        }
    }
}

fn analyze_type_pattern<R: VariableResolver>(
    node: &SyntaxNode,
    resolver: &R,
) -> Option<NullCheckCondition> {
    let pattern = node
        .descendants()
        .find(|child| child.kind() == SyntaxKind::TYPE_PATTERN)?;
    let binding_name = pattern
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier())
        .last()?
        .text()
        .to_string();
    let binding = resolver.resolve_variable_node(&binding_name, &pattern)?;
    let selector = continuation_left_operand(node)
        .or_else(|| node.children().find(|child| is_expression(child.kind())))
        .and_then(|selector| get_variable_id(&selector, resolver));
    Some(NullCheckCondition::TypePattern { binding, selector })
}

fn analyze_binary<R, F>(
    node: &SyntaxNode,
    resolver: &R,
    call_owner: &F,
) -> Option<NullCheckCondition>
where
    R: VariableResolver,
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    // Find operator
    let op = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| {
            matches!(
                t.kind(),
                SyntaxKind::EQ_EQ
                    | SyntaxKind::BANG_EQ
                    | SyntaxKind::AMP_AMP
                    | SyntaxKind::PIPE_PIPE
            )
        })?;

    let expr_children: Vec<SyntaxNode> = node
        .children()
        .filter(|c| is_expression(c.kind()))
        .collect();

    // Same-precedence chains parse as continuation nodes: `a && b && c`
    // produces sibling BINARY_EXPR nodes where the second is `[&&, c]` — its
    // left operand is the preceding sibling expression.
    let (left, right) = match expr_children.as_slice() {
        [left, right, ..] => (left.clone(), right.clone()),
        [right] => (continuation_left_operand(node)?, right.clone()),
        [] => return None,
    };

    match op.kind() {
        SyntaxKind::EQ_EQ => {
            // x == null or null == x
            if is_null_literal(&right) {
                get_variable_id(&left, resolver).map(NullCheckCondition::IsNull)
            } else if is_null_literal(&left) {
                get_variable_id(&right, resolver).map(NullCheckCondition::IsNull)
            } else {
                None
            }
        }
        SyntaxKind::BANG_EQ => {
            // x != null or null != x
            if is_null_literal(&right) {
                get_variable_id(&left, resolver)
                    .or_else(|| nonnull_enforcing_argument_id(&left, resolver, call_owner))
                    .map(NullCheckCondition::IsNonNull)
            } else if is_null_literal(&left) {
                get_variable_id(&right, resolver)
                    .or_else(|| nonnull_enforcing_argument_id(&right, resolver, call_owner))
                    .map(NullCheckCondition::IsNonNull)
            } else {
                None
            }
        }
        SyntaxKind::AMP_AMP => {
            // cond1 && cond2. An operand that isn't itself a null check carries
            // no nullability info (`Top`) rather than discarding the whole
            // compound, so `x != null && x.isReady()` still narrows `x`.
            let left_cond =
                analyze_node(&left, resolver, call_owner).unwrap_or(NullCheckCondition::Top);
            let right_cond =
                analyze_node(&right, resolver, call_owner).unwrap_or(NullCheckCondition::Top);
            Some(NullCheckCondition::And(
                Box::new(left_cond),
                Box::new(right_cond),
            ))
        }
        SyntaxKind::PIPE_PIPE => {
            // cond1 || cond2. As with `&&`, an unrecognized operand becomes
            // `Top` so the recognizable side still contributes on the
            // both-false branch (`if (x == null || x.isEmpty()) return;`).
            let left_cond =
                analyze_node(&left, resolver, call_owner).unwrap_or(NullCheckCondition::Top);
            let right_cond =
                analyze_node(&right, resolver, call_owner).unwrap_or(NullCheckCondition::Top);
            Some(NullCheckCondition::Or(
                Box::new(left_cond),
                Box::new(right_cond),
            ))
        }
        _ => None,
    }
}

fn nonnull_enforcing_argument_id<R, F>(
    expression: &SyntaxNode,
    resolver: &R,
    call_owner: &F,
) -> Option<VariableId>
where
    R: VariableResolver,
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    (expression.kind() == SyntaxKind::METHOD_CALL_EXPR
        && is_known_nonnull_enforcer(expression, call_owner))
    .then(|| extract_first_argument(expression))
    .flatten()
    .and_then(|argument| get_variable_id(&argument, resolver))
}

fn analyze_unary<R, F>(
    node: &SyntaxNode,
    resolver: &R,
    call_owner: &F,
) -> Option<NullCheckCondition>
where
    R: VariableResolver,
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    let op = node
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .find(|t| t.kind() == SyntaxKind::BANG)?;

    let operand = node.children().find(|c| is_expression(c.kind()))?;

    if op.kind() == SyntaxKind::BANG {
        let inner = analyze_node(&operand, resolver, call_owner)?;
        Some(NullCheckCondition::Not(Box::new(inner)))
    } else {
        None
    }
}

fn analyze_call<R, F>(node: &SyntaxNode, resolver: &R, call_owner: &F) -> Option<NullCheckCondition>
where
    R: VariableResolver,
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    // Check for Objects.requireNonNull(x), Objects.isNull(x), Objects.nonNull(x), etc.
    let method_name = extract_method_name(node)?;

    match method_name.as_str() {
        "requireNonNull" | "checkNotNull" if is_known_nonnull_enforcer(node, call_owner) => {
            // First argument is the checked variable
            let arg = extract_first_argument(node)?;
            get_variable_id(&arg, resolver).map(NullCheckCondition::IsNonNull)
        }
        "checkArgument" | "checkState"
            if has_resolved_call_owner(node, "Preconditions", call_owner) =>
        {
            // Extract first argument (the boolean condition) and recursively analyze it
            // Preconditions.checkArgument(user != null, "message") throws if condition is false
            // After the call, the condition must be true
            let arg = extract_first_argument(node)?;

            // Recursively analyze the condition
            analyze_node(&arg, resolver, call_owner)
        }
        // Objects.nonNull(x) returns true if x != null
        // Used in stream filters: stream.filter(Objects::nonNull)
        // Or as condition: if (Objects.nonNull(user)) { ... }
        "nonNull" if has_resolved_call_owner(node, "Objects", call_owner) => {
            let arg = extract_first_argument(node)?;
            get_variable_id(&arg, resolver).map(NullCheckCondition::IsNonNull)
        }
        // Objects.isNull(x) returns true if x == null
        // if (Objects.isNull(user)) { /* user is null here */ }
        // if (!Objects.isNull(user)) { /* user is non-null here */ }
        "isNull" if has_resolved_call_owner(node, "Objects", call_owner) => {
            let arg = extract_first_argument(node)?;
            get_variable_id(&arg, resolver).map(NullCheckCondition::IsNull)
        }
        _ => None,
    }
}

fn is_known_nonnull_enforcer<F>(call: &SyntaxNode, call_owner: &F) -> bool
where
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    match extract_method_name(call).as_deref() {
        Some("requireNonNull") => has_resolved_call_owner(call, "Objects", call_owner),
        Some("checkNotNull") => has_resolved_call_owner(call, "Preconditions", call_owner),
        _ => false,
    }
}

fn has_resolved_call_owner<F>(call: &SyntaxNode, expected: &str, call_owner: &F) -> bool
where
    F: Fn(&SyntaxNode) -> Option<String> + ?Sized,
{
    call_owner(call)
        .as_deref()
        .and_then(|owner| owner.rsplit('.').next())
        == Some(expected)
}

/// Extract the first argument from a method call node
fn extract_first_argument(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.descendants()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)?
        .children()
        .find(|c| is_expression(c.kind()))
}

fn extract_method_name(call: &SyntaxNode) -> Option<String> {
    // With nested trees, METHOD_CALL_EXPR contains the IDENT as a direct child token.
    // For `Objects.requireNonNull(user)`:
    //   METHOD_CALL_EXPR -> [NAME_EXPR("Objects"), DOT, IDENT("requireNonNull"), ARG_LIST]
    // For simple calls like `method()`:
    //   METHOD_CALL_EXPR -> [NAME_EXPR("method"), ARG_LIST]
    // We want the last IDENT token before the ARG_LIST.
    let mut last_ident = None;
    for child in call.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                last_ident = Some(t.text().to_string());
            }
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::ARG_LIST => {
                break;
            }
            rowan::NodeOrToken::Node(n) if n.kind() == SyntaxKind::NAME_EXPR => {
                // For simple calls, extract the IDENT from NAME_EXPR
                for elem in n.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = elem {
                        if t.kind() == SyntaxKind::IDENT {
                            last_ident = Some(t.text().to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    last_ident
}

fn is_null_literal(node: &SyntaxNode) -> bool {
    node.kind() == SyntaxKind::LITERAL_EXPR && node.text() == "null"
}

fn get_variable_id<R: VariableResolver>(node: &SyntaxNode, resolver: &R) -> Option<VariableId> {
    let name = stable_access_path(node)?;
    resolver.resolve_variable_node(&name, node)
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

/// If `node` is a precede-style continuation (`BINARY_EXPR [op, operand]` or
/// `ASSIGNMENT_EXPR [op, value]`), return its left operand: the preceding
/// sibling expression. Returns `None` for self-contained and unrelated nodes.
pub(crate) fn continuation_left_operand(node: &SyntaxNode) -> Option<SyntaxNode> {
    if !matches!(
        node.kind(),
        SyntaxKind::BINARY_EXPR | SyntaxKind::ASSIGNMENT_EXPR
    ) {
        return None;
    }
    let mut exprs = node.children().filter(|c| is_expression(c.kind()));
    exprs.next()?;
    if exprs.next().is_some() {
        return None;
    }
    node.prev_sibling().filter(|s| is_expression(s.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_negate_is_null() {
        let cond = NullCheckCondition::IsNull(VariableId(0));
        match cond.negate() {
            NullCheckCondition::IsNonNull(v) => assert_eq!(v, VariableId(0)),
            _ => panic!("Expected IsNonNull"),
        }
    }

    #[test]
    fn test_negate_and() {
        let cond = NullCheckCondition::And(
            Box::new(NullCheckCondition::IsNull(VariableId(0))),
            Box::new(NullCheckCondition::IsNonNull(VariableId(1))),
        );
        // !(A && B) = !A || !B
        match cond.negate() {
            NullCheckCondition::Or(l, r) => {
                assert!(matches!(*l, NullCheckCondition::IsNonNull(VariableId(0))));
                assert!(matches!(*r, NullCheckCondition::IsNull(VariableId(1))));
            }
            _ => panic!("Expected Or"),
        }
    }

    #[test]
    fn test_double_negation() {
        let cond = NullCheckCondition::IsNull(VariableId(0));
        let negated = cond.clone().negate().negate();
        assert!(matches!(negated, NullCheckCondition::IsNull(VariableId(0))));
    }
}
