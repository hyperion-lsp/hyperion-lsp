// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Context building for usage extraction - tracks enclosing class and variable types

use crate::{import_resolver::ImportResolver, FileId, ImportMap};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use tracing::debug;

use super::{type_spelling::resolve_parameterized_type_in_owner, Usage};

/// Context for extraction - tracks enclosing class/interface information and variable types
#[derive(Clone, Default)]
pub struct ExtractionContext {
    /// Fully qualified name of the enclosing class (e.g., "com.example.Service")
    pub enclosing_class_fqn: Option<String>,
    /// Map of variable/field names to their resolved FQN types
    /// e.g., "user" -> "com.example.model.User"
    pub variable_types: HashMap<String, String>,
    /// Field types are retained separately so scope-aware local lookup can
    /// fall back to members without consulting the method-wide compatibility
    /// map above.
    pub field_types: Arc<HashMap<String, String>>,
    /// Names of fields declared on the enclosing class
    pub field_names: HashSet<String>,
    /// Names of local bindings (parameters, locals, foreach vars) that shadow fields
    pub local_bindings: HashSet<String>,
    /// Direct local declarations grouped by their immediate syntax parent.
    /// The map is built once for a file instead of walking every earlier
    /// statement for every receiver or identifier use.
    scope_declarations: Rc<ScopeDeclarations>,
}

type ScopeDeclarations = HashMap<SyntaxNode, HashMap<String, Vec<SyntaxNode>>>;

impl ExtractionContext {
    pub(super) fn for_root(root: &SyntaxNode) -> Self {
        let mut scopes = ScopeDeclarations::new();
        for declaration in root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::LOCAL_VAR_DECL)
        {
            let Some(parent) = declaration.parent() else {
                continue;
            };
            for name in direct_declared_names(&declaration) {
                scopes
                    .entry(parent.clone())
                    .or_default()
                    .entry(name)
                    .or_default()
                    .push(declaration.clone());
            }
        }
        Self {
            scope_declarations: Rc::new(scopes),
            ..Self::default()
        }
    }
}

/// Build extraction context from a class/interface/enum declaration
pub fn build_class_context(
    node: &SyntaxNode,
    imports: &ImportMap,
    resolver: &ImportResolver,
    parent: &ExtractionContext,
) -> ExtractionContext {
    // Find the class name (first IDENT token)
    let mut class_name: Option<String> = None;
    for elem in node.children_with_tokens() {
        if let rowan::NodeOrToken::Token(token) = elem {
            if token.kind().is_identifier() {
                class_name = Some(token.text().to_string());
                break;
            }
        }
    }

    let enclosing_class_fqn = class_name.map(|name| {
        if let Some(enclosing) = parent.enclosing_class_fqn.as_deref() {
            format!("{enclosing}.{name}")
        } else if let Some(ref pkg) = imports.current_package {
            format!("{pkg}.{name}")
        } else {
            name
        }
    });

    // Extract field types from the class body
    // Pre-allocate for average class size (~20 fields is a reasonable estimate)
    let mut variable_types = HashMap::with_capacity(20);
    let mut field_names = HashSet::with_capacity(20);
    for child in node.descendants() {
        if child.kind() == SyntaxKind::FIELD_DECL
            && child
                .ancestors()
                .skip(1)
                .find(|ancestor| ancestor.kind().is_type_declaration())
                .is_some_and(|owner| owner == *node)
        {
            if let Some((var_name, type_fqn)) =
                extract_var_name_and_type(&child, resolver, enclosing_class_fqn.as_deref())
            {
                debug!("Field type: {} -> {}", var_name, type_fqn);
                field_names.insert(var_name.clone());
                variable_types.insert(var_name, type_fqn);
            }
        }
    }
    let field_types = Arc::new(variable_types.clone());

    ExtractionContext {
        enclosing_class_fqn,
        variable_types,
        field_types,
        field_names,
        local_bindings: HashSet::new(),
        scope_declarations: parent.scope_declarations.clone(),
    }
}

/// Resolve a name at its exact use site using Java's lexical declaration
/// order. Only declarations in the current/ancestor scope and preceding the
/// use are considered; locals from sibling blocks or later statements cannot
/// leak into overload selection.
pub fn resolve_visible_variable_type(
    name_expr: &SyntaxNode,
    name: &str,
    ctx: &ExtractionContext,
    resolver: &ImportResolver,
) -> Option<String> {
    if let Some(declaration) = visible_local_declaration(name_expr, name, ctx) {
        return extract_declared_type(&declaration, resolver, ctx.enclosing_class_fqn.as_deref());
    }
    if let Some(field_type) = ctx.field_types.get(name) {
        return Some(field_type.clone());
    }

    // The syntax-derived field map deliberately contains only members declared
    // by the current type. Resolve the remaining names through Java member
    // lookup so inherited superclass and interface fields retain their
    // declaration's receiver type. This also preserves nearest-class hiding
    // and rejects ambiguous interface fields.
    let owner = ctx.enclosing_class_fqn.as_deref()?;
    let field = resolver.resolve_value_member(owner, name)?;
    let field_type = resolver
        .project_member_type(owner, &field)
        .or_else(|| field.return_type.as_deref().map(str::to_owned))?;
    receiver_type(&field_type)
}

fn receiver_type(raw: &str) -> Option<String> {
    // Array member access is resolved on the element/component spelling, but
    // generic arguments remain semantically significant to later chain
    // segments. Erasing `Factory<Service>` here makes an inherited field look
    // like raw `Factory`, so `field.service().target()` loses `Service` before
    // projecting the return type of `service()`.
    let receiver = raw.trim().trim_end_matches("[]").trim();
    (!receiver.is_empty()).then(|| receiver.to_string())
}

/// Whether a local/parameter declaration, rather than a field or import, owns
/// this spelling at the exact use site.
pub(super) fn is_local_binding_visible_with_context(
    name_expr: &SyntaxNode,
    name: &str,
    ctx: &ExtractionContext,
) -> bool {
    visible_local_declaration(name_expr, name, ctx).is_some()
}

fn direct_declared_names(node: &SyntaxNode) -> Vec<String> {
    let before = match node.kind() {
        SyntaxKind::FOREACH_STMT => node
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::COLON => {
                    Some(token.text_range().start())
                }
                _ => None,
            }),
        SyntaxKind::LAMBDA_EXPR => node
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::ARROW => {
                    Some(token.text_range().start())
                }
                _ => None,
            }),
        SyntaxKind::CATCH_CLAUSE => node
            .children_with_tokens()
            .find_map(|element| match element {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::R_PAREN => {
                    Some(token.text_range().start())
                }
                _ => None,
            }),
        _ => None,
    };

    node.children_with_tokens()
        .filter_map(|element| match element {
            rowan::NodeOrToken::Token(token)
                if token.kind().is_identifier()
                    && before.is_none_or(|end| token.text_range().start() < end) =>
            {
                Some(token.text().to_owned())
            }
            rowan::NodeOrToken::Node(parameter) if parameter.kind() == SyntaxKind::PARAM => {
                parameter
                    .children_with_tokens()
                    .filter_map(|element| element.into_token())
                    .filter(|token| token.kind().is_identifier())
                    .last()
                    .map(|token| token.text().to_owned())
            }
            _ => None,
        })
        .collect()
}

fn directly_declares(node: &SyntaxNode, name: &str) -> bool {
    direct_declared_names(node)
        .iter()
        .any(|declared| declared == name)
}

fn parameter_list_declares(owner: &SyntaxNode, name: &str) -> bool {
    owner
        .children()
        .filter(|child| child.kind() == SyntaxKind::PARAM_LIST)
        .flat_map(|parameters| parameters.children())
        .filter(|parameter| parameter.kind() == SyntaxKind::PARAM)
        .any(|parameter| directly_declares(&parameter, name))
}

fn pattern_binding_when(
    expression: &SyntaxNode,
    name: &str,
    when_true: bool,
) -> Option<SyntaxNode> {
    match expression.kind() {
        SyntaxKind::PATTERN_INSTANCEOF_EXPR if when_true => expression
            .descendants()
            .find(|node| node.kind() == SyntaxKind::TYPE_PATTERN && directly_declares(node, name)),
        SyntaxKind::PAREN_EXPR => expression
            .children()
            .find_map(|child| pattern_binding_when(&child, name, when_true)),
        SyntaxKind::UNARY_EXPR
            if expression.children_with_tokens().any(|element| {
                element
                    .into_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::BANG)
            }) =>
        {
            expression
                .children()
                .find_map(|child| pattern_binding_when(&child, name, !when_true))
        }
        SyntaxKind::BINARY_EXPR => {
            let operator = expression
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .find(|token| matches!(token.kind(), SyntaxKind::AMP_AMP | SyntaxKind::PIPE_PIPE))?
                .kind();
            let conjunctive = (operator == SyntaxKind::AMP_AMP && when_true)
                || (operator == SyntaxKind::PIPE_PIPE && !when_true);
            conjunctive.then(|| {
                expression
                    .children()
                    .find_map(|child| pattern_binding_when(&child, name, when_true))
            })?
        }
        _ => None,
    }
}

fn expression_before_token(node: &SyntaxNode, boundary: SyntaxKind) -> Option<SyntaxNode> {
    let boundary = node
        .children_with_tokens()
        .find_map(|element| {
            element
                .into_token()
                .filter(|token| token.kind() == boundary)
        })?
        .text_range()
        .start();
    node.children()
        .filter(|child| child.text_range().end() <= boundary)
        .last()
}

fn visible_pattern_declaration(
    ancestor: &SyntaxNode,
    child_on_path: &SyntaxNode,
    use_start: rowan::TextSize,
    name: &str,
) -> Option<SyntaxNode> {
    if ancestor.kind() == SyntaxKind::BINARY_EXPR {
        let operator = ancestor
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| matches!(token.kind(), SyntaxKind::AMP_AMP | SyntaxKind::PIPE_PIPE));
        let expressions = ancestor.children().collect::<Vec<_>>();
        if let (Some(operator), [left, right]) = (operator, expressions.as_slice()) {
            if child_on_path == right {
                return pattern_binding_when(left, name, operator.kind() == SyntaxKind::AMP_AMP);
            }
        }
    }

    if matches!(
        ancestor.kind(),
        SyntaxKind::IF_STMT | SyntaxKind::WHILE_STMT
    ) {
        let condition = expression_before_token(ancestor, SyntaxKind::R_PAREN)?;
        let close = ancestor
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::R_PAREN)?
            .text_range()
            .end();
        if use_start <= close {
            return None;
        }
        if ancestor.kind() == SyntaxKind::WHILE_STMT {
            return pattern_binding_when(&condition, name, true);
        }
        let else_start = ancestor
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| token.kind() == SyntaxKind::ELSE_KW)
            .map(|token| token.text_range().start());
        return pattern_binding_when(
            &condition,
            name,
            else_start.is_none_or(|else_start| use_start < else_start),
        );
    }

    if ancestor.kind() == SyntaxKind::TERNARY_EXPR {
        let expressions = ancestor.children().collect::<Vec<_>>();
        if let [condition, when_true, when_false] = expressions.as_slice() {
            if child_on_path == when_true {
                return pattern_binding_when(condition, name, true);
            }
            if child_on_path == when_false {
                return pattern_binding_when(condition, name, false);
            }
        }
    }
    if ancestor.kind() == SyntaxKind::CASE_CLAUSE {
        return ancestor.descendants().find(|pattern| {
            matches!(
                pattern.kind(),
                SyntaxKind::TYPE_PATTERN | SyntaxKind::RECORD_PATTERN
            ) && pattern.text_range().end() <= use_start
                && directly_declares(pattern, name)
        });
    }
    None
}

fn visible_local_declaration(
    name_expr: &SyntaxNode,
    name: &str,
    ctx: &ExtractionContext,
) -> Option<SyntaxNode> {
    let use_start = name_expr.text_range().start();
    let mut child_on_path = name_expr.clone();
    for ancestor in name_expr.ancestors().skip(1) {
        if ancestor.kind() == SyntaxKind::LOCAL_VAR_DECL
            && directly_declares(&ancestor, name)
            && ancestor
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| token.kind().is_identifier() && token.text() == name)
                .any(|token| token.text_range().start() < use_start)
        {
            return Some(ancestor);
        }

        if let Some(declaration) = ctx
            .scope_declarations
            .get(&ancestor)
            .and_then(|by_name| by_name.get(name))
            .and_then(|declarations| {
                declarations.iter().find(|declaration| {
                    declaration.text_range().start() < child_on_path.text_range().start()
                })
            })
        {
            return Some(declaration.clone());
        }

        match ancestor.kind() {
            SyntaxKind::FOREACH_STMT => {
                let close = ancestor
                    .children_with_tokens()
                    .find_map(|element| match element {
                        rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::R_PAREN => {
                            Some(token.text_range().end())
                        }
                        _ => None,
                    });
                if close.is_some_and(|close| use_start > close)
                    && directly_declares(&ancestor, name)
                {
                    return Some(ancestor);
                }
            }
            SyntaxKind::TRY_STMT => {
                let try_body = ancestor
                    .children()
                    .find(|child| child.kind() == SyntaxKind::BLOCK_STMT);
                if try_body.as_ref() == Some(&child_on_path) {
                    if let Some(resource) = ancestor
                        .children()
                        .find(|child| child.kind() == SyntaxKind::RESOURCE_SPEC)
                        .and_then(|resources| {
                            resources
                                .children()
                                .filter(|resource| resource.kind() == SyntaxKind::LOCAL_VAR_DECL)
                                .find(|resource| directly_declares(resource, name))
                        })
                    {
                        return Some(resource);
                    }
                }
            }
            SyntaxKind::CATCH_CLAUSE if directly_declares(&ancestor, name) => {
                return Some(ancestor);
            }
            SyntaxKind::LAMBDA_EXPR => {
                if let Some(parameter) = ancestor
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::PARAM)
                    .find(|parameter| directly_declares(parameter, name))
                {
                    return Some(parameter);
                }
                if directly_declares(&ancestor, name) {
                    return Some(ancestor);
                }
            }
            _ => {}
        }

        if let Some(pattern) =
            visible_pattern_declaration(&ancestor, &child_on_path, use_start, name)
        {
            return Some(pattern);
        }

        if matches!(
            ancestor.kind(),
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
        ) {
            if parameter_list_declares(&ancestor, name) {
                return ancestor
                    .children()
                    .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
                    .and_then(|parameters| {
                        parameters
                            .children()
                            .find(|parameter| directly_declares(parameter, name))
                    });
            }
            break;
        }
        child_on_path = ancestor;
    }
    None
}

/// Build method context with parameters and local variables
pub fn build_method_context(
    node: &SyntaxNode,
    parent_ctx: &ExtractionContext,
    resolver: &ImportResolver,
) -> ExtractionContext {
    let mut ctx = parent_ctx.clone();

    // Extract parameter types from PARAM_LIST
    for child in node.children() {
        if child.kind() == SyntaxKind::PARAM_LIST {
            for param in child.children() {
                if param.kind() == SyntaxKind::PARAM {
                    if let Some((var_name, type_fqn)) = extract_var_name_and_type(
                        &param,
                        resolver,
                        ctx.enclosing_class_fqn.as_deref(),
                    ) {
                        debug!("Param type: {} -> {}", var_name, type_fqn);
                        ctx.local_bindings.insert(var_name.clone());
                        ctx.variable_types.insert(var_name, type_fqn);
                    }
                }
            }
        }
    }

    ctx
}

/// Extract variable name and resolved type FQN from a declaration node
/// Works with FIELD_DECL, LOCAL_VAR_DECL, and PARAM nodes
fn extract_var_name_and_type(
    node: &SyntaxNode,
    resolver: &ImportResolver,
    owner_fqn: Option<&str>,
) -> Option<(String, String)> {
    let var_name = direct_declared_names(node).into_iter().last()?;
    let type_name = extract_declared_type(node, resolver, owner_fqn)?;
    Some((var_name, type_name))
}

fn extract_declared_type(
    node: &SyntaxNode,
    resolver: &ImportResolver,
    owner_fqn: Option<&str>,
) -> Option<String> {
    let mut type_name: Option<String> = None;

    // Look for CLASS_TYPE or PRIMITIVE_TYPE to get the type.
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::CLASS_TYPE && type_name.is_none() {
                    type_name = Some(n.text().to_string());
                } else if n.kind() == SyntaxKind::PRIMITIVE_TYPE && type_name.is_none() {
                    for token in n.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = token {
                            if t.kind().is_keyword() || t.kind().is_identifier() {
                                type_name = Some(t.text().to_string());
                                break;
                            }
                        }
                    }
                }
            }
            rowan::NodeOrToken::Token(_) => {}
        }
    }

    // Resolve type to FQN
    if let Some(type_simple) = type_name {
        if matches!(
            type_simple.as_str(),
            "byte" | "short" | "int" | "long" | "float" | "double" | "boolean" | "char"
        ) {
            return Some(type_simple);
        }
        return resolve_parameterized_type_in_owner(&type_simple, resolver, owner_fqn);
    }

    None
}

/// Extract type usages from local variable declarations
pub fn extract_local_var_types(
    _node: &SyntaxNode,
    _file_id: FileId,
    _resolver: &ImportResolver,
    _usages: &mut Vec<Usage>,
    _ctx: &ExtractionContext,
) {
    // No additional work is required: the CLASS_TYPE visitor records the type
    // node independently of the declaration that contains it.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_local_binding_visible(name_expr: &SyntaxNode, name: &str) -> bool {
        let root = name_expr.ancestors().last().expect("syntax root");
        let ctx = ExtractionContext::for_root(&root);
        is_local_binding_visible_with_context(name_expr, name, &ctx)
    }

    fn parse(source: &str) -> SyntaxNode {
        let tokens = hyperion_parser::Lexer::new(source).collect();
        let (green, errors) = hyperion_parser::Parser::parse(tokens);
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        SyntaxNode::new_root(green)
    }

    fn name_uses(root: &SyntaxNode, name: &str) -> Vec<SyntaxNode> {
        root.descendants()
            .filter(|node| node.kind() == SyntaxKind::NAME_EXPR && node.text() == name)
            .collect()
    }

    #[test]
    fn foreach_binding_starts_at_the_contained_statement() {
        let root = parse(
            "class C { void f(Iterable<String> values) { \
             for (String value : value.values()) { use(value); } } }",
        );
        let uses = name_uses(&root, "value");
        assert_eq!(uses.len(), 2);
        assert!(!is_local_binding_visible(&uses[0], "value"));
        assert!(is_local_binding_visible(&uses[1], "value"));
    }

    #[test]
    fn resource_binding_reaches_later_resources_and_try_body_only() {
        let root = parse(
            "class C { void f() { try (Reader first = open(); \
             Reader second = wrap(first)) { use(first); } \
             catch (Exception ex) { use(first); } } }",
        );
        let uses = name_uses(&root, "first");
        assert_eq!(uses.len(), 3);
        assert!(is_local_binding_visible(&uses[0], "first"));
        assert!(is_local_binding_visible(&uses[1], "first"));
        assert!(!is_local_binding_visible(&uses[2], "first"));
    }

    #[test]
    fn pattern_binding_follows_boolean_flow_scope() {
        let root = parse(
            "class C { void f(Object value) { \
             if (value instanceof String text && text.isEmpty()) { use(text); } \
             else { use(text); } use(text); \
             if (!(value instanceof String other)) { use(other); } else { use(other); } } }",
        );
        let text_uses = name_uses(&root, "text");
        assert_eq!(text_uses.len(), 4);
        assert!(is_local_binding_visible(&text_uses[0], "text"));
        assert!(is_local_binding_visible(&text_uses[1], "text"));
        assert!(!is_local_binding_visible(&text_uses[2], "text"));
        assert!(!is_local_binding_visible(&text_uses[3], "text"));

        let other_uses = name_uses(&root, "other");
        assert_eq!(other_uses.len(), 2);
        assert!(!is_local_binding_visible(&other_uses[0], "other"));
        assert!(is_local_binding_visible(&other_uses[1], "other"));

        let switch_root = parse(
            "class C { void f(Object value) { switch (value) { \
             case String text when text.isEmpty() -> use(text); \
             default -> use(text); } } }",
        );
        let switch_uses = name_uses(&switch_root, "text");
        assert_eq!(switch_uses.len(), 3);
        assert!(is_local_binding_visible(&switch_uses[0], "text"));
        assert!(is_local_binding_visible(&switch_uses[1], "text"));
        assert!(!is_local_binding_visible(&switch_uses[2], "text"));
    }

    #[test]
    fn indexed_locals_respect_order_and_block_boundaries() {
        let root = parse(
            "class C { void f() { use(value); int value = 1; use(value); \
             { int inner = 2; use(inner); } use(inner); \
             if (true) { int branch = 3; use(branch); } use(branch); } }",
        );
        let value = name_uses(&root, "value");
        assert_eq!(value.len(), 2);
        assert!(!is_local_binding_visible(&value[0], "value"));
        assert!(is_local_binding_visible(&value[1], "value"));

        let inner = name_uses(&root, "inner");
        assert_eq!(inner.len(), 2);
        assert!(is_local_binding_visible(&inner[0], "inner"));
        assert!(!is_local_binding_visible(&inner[1], "inner"));

        let branch = name_uses(&root, "branch");
        assert_eq!(branch.len(), 2);
        assert!(is_local_binding_visible(&branch[0], "branch"));
        assert!(!is_local_binding_visible(&branch[1], "branch"));
    }

    #[test]
    fn indexed_locals_keep_lambda_and_catch_parameters_scoped() {
        let root = parse(
            "class C { void f() { \
             java.util.function.Function<Integer, Integer> fn = input -> input + 1; \
             use(input); \
             try { run(); } catch (Exception failure) { use(failure); } \
             use(failure); } }",
        );
        let input = name_uses(&root, "input");
        assert_eq!(input.len(), 2);
        assert!(is_local_binding_visible(&input[0], "input"));
        assert!(!is_local_binding_visible(&input[1], "input"));

        let failure = name_uses(&root, "failure");
        assert_eq!(failure.len(), 2);
        assert!(is_local_binding_visible(&failure[0], "failure"));
        assert!(!is_local_binding_visible(&failure[1], "failure"));
    }
}
