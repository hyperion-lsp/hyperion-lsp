// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared Kotlin call-site analysis.
//!
//! Signature help and parameter-name inlay hints both need the same two
//! things: the shape of the call under the cursor (callee, receiver,
//! arguments, trailing lambda) and the parameter lists of whatever that callee
//! could resolve to. Both are computed here, off the Kotlin CST, so the two
//! providers cannot drift apart.
//!
//! # Why not reuse the Java machinery
//!
//! `hyperion_parser::SyntaxKind` and `hyperion_kotlin::SyntaxKind` are separate
//! enums with separate numbering, so the Java walkers in [`crate::signature_help`]
//! and [`crate::inlay_hints`] would misread a Kotlin green tree. Kotlin also has
//! call shapes Java does not: named arguments, trailing lambdas outside the
//! parentheses, and calls with no parentheses at all (`items.map { … }`).
//!
//! # Kotlin `CALL_EXPR` shape
//!
//! ```text
//! foo(1, 2)          CALL_EXPR [IDENT, L_PAREN, LITERAL_EXPR, COMMA, LITERAL_EXPR, R_PAREN]
//! a.b(1)             CALL_EXPR [MEMBER_ACCESS_EXPR [IDENT, DOT, IDENT], L_PAREN, …, R_PAREN]
//! a?.b(1)            CALL_EXPR [SAFE_ACCESS_EXPR  [IDENT, SAFE_CALL, IDENT], L_PAREN, …]
//! foo<Int>(1)        CALL_EXPR [IDENT, TYPE_ARGUMENT_LIST, L_PAREN, …, R_PAREN]
//! foo(1) { … }       CALL_EXPR [IDENT, L_PAREN, …, R_PAREN, LAMBDA_EXPR]
//! items.map { … }    CALL_EXPR [MEMBER_ACCESS_EXPR, LAMBDA_EXPR]           (no parens at all)
//! foo(name = 1)      the argument is BINARY_EXPR [IDENT, EQ, value]
//! foo(x)             the argument is a bare IDENT *token*, not a node
//! ```
//!
//! Note the last two: a named argument is an `EQ` binary expression, and a
//! plain variable reference is a bare token rather than a wrapper node, so
//! argument collection has to walk `children_with_tokens`.

use hyperion_kotlin::{SyntaxKind as K, SyntaxNode, SyntaxToken};
use hyperion_workspace::{DeclarationNamespace, FileId, SymbolTarget, WorkspaceIndex};
use rowan::{TextRange, TextSize};
use std::sync::Arc;
use tower_lsp::lsp_types::SymbolKind;

/// A node or token in the Kotlin CST.
pub(crate) type KtElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

/// Upper bound on how many overloads a single signature-help response carries.
const MAX_SIGNATURES: usize = 8;

// ---------------------------------------------------------------------------
// Call-site shape
// ---------------------------------------------------------------------------

/// One argument of a Kotlin call.
#[derive(Debug, Clone)]
pub(crate) struct KotlinArgument {
    /// Full span, including any `name =` prefix.
    pub range: TextRange,
    /// `Some(name)` for a named argument (`name = value`).
    pub name: Option<String>,
    /// The value expression: `Node` for wrapped expressions, `Token` for bare
    /// identifier references.
    pub value: Option<KtElement>,
}

impl KotlinArgument {
    /// Kind of the value expression (node kind, or token kind for bare tokens).
    pub fn value_kind(&self) -> Option<K> {
        self.value.as_ref().map(KtElement::kind)
    }

    /// Start offset of the value expression, falling back to the argument span.
    pub fn value_start(&self) -> TextSize {
        self.value
            .as_ref()
            .map_or_else(|| self.range.start(), |v| v.text_range().start())
    }

    /// Source text of the value expression.
    pub fn value_text(&self) -> String {
        match &self.value {
            Some(rowan::NodeOrToken::Node(n)) => n.text().to_string().trim().to_string(),
            Some(rowan::NodeOrToken::Token(t)) => t.text().to_string(),
            None => String::new(),
        }
    }
}

/// The decomposed form of a Kotlin `CALL_EXPR`.
#[derive(Debug, Clone)]
pub(crate) struct KotlinCall {
    /// The `CALL_EXPR` node itself.
    pub node: SyntaxNode,
    /// Simple name of the callee (`b` in `a.b(1)`).
    pub name: String,
    /// Receiver expression for a qualified call, else `None`.
    pub receiver: Option<KtElement>,
    /// Offset of `(`, absent for a parenthesis-free trailing-lambda call.
    pub l_paren: Option<TextSize>,
    /// Offset of `)`, absent while the call is still being typed.
    pub r_paren: Option<TextSize>,
    /// Offsets of the top-level argument separators.
    pub commas: Vec<TextSize>,
    /// Arguments inside the parentheses, in source order.
    pub args: Vec<KotlinArgument>,
    /// Trailing lambda written outside the parentheses, if any.
    pub trailing_lambda: Option<SyntaxNode>,
}

impl KotlinCall {
    /// Number of argument *slots*, counting a trailing lambda as one.
    ///
    /// Slots, not filled arguments: mid-typing, `send("hi", |)` has one
    /// expression but two slots, and it is the slot count that says which
    /// overload the user is aiming at.
    pub fn arg_count(&self) -> usize {
        let in_parens = if self.args.is_empty() && self.commas.is_empty() {
            0
        } else {
            self.args.len().max(self.commas.len() + 1)
        };
        in_parens + usize::from(self.trailing_lambda.is_some())
    }

    /// Is `offset` inside the region where signature help applies — i.e. between
    /// the parentheses, or inside the trailing lambda?
    pub fn covers_offset(&self, offset: TextSize) -> bool {
        if let Some(lambda) = &self.trailing_lambda {
            let range = lambda.text_range();
            if offset > range.start() && offset <= range.end() {
                return true;
            }
        }
        match self.l_paren {
            Some(l_paren) => {
                offset > l_paren && self.r_paren.is_none_or(|r_paren| offset <= r_paren)
            }
            None => false,
        }
    }
}

/// Decompose a `CALL_EXPR` node. Returns `None` for any other node kind, or when
/// the callee name cannot be read.
pub(crate) fn analyze_call(node: &SyntaxNode) -> Option<KotlinCall> {
    if node.kind() != K::CALL_EXPR {
        return None;
    }

    // Everything before `(` (or before the lambda, for `items.map { … }`) is
    // the callee. Type arguments sit between the two and belong to neither.
    let mut callee: Option<KtElement> = None;
    let mut l_paren = None;
    let mut r_paren = None;
    let mut commas = Vec::new();
    let mut trailing_lambda = None;
    let mut groups: Vec<Vec<KtElement>> = Vec::new();
    let mut current: Vec<KtElement> = Vec::new();
    let mut in_parens = false;
    let mut closed = false;

    for child in node.children_with_tokens() {
        if child.kind().is_trivia() {
            continue;
        }
        match child.kind() {
            K::L_PAREN if !in_parens && !closed => {
                in_parens = true;
                l_paren = Some(child.text_range().start());
            }
            K::R_PAREN if in_parens => {
                in_parens = false;
                closed = true;
                r_paren = Some(child.text_range().start());
                if !current.is_empty() {
                    groups.push(std::mem::take(&mut current));
                }
            }
            K::COMMA if in_parens => {
                commas.push(child.text_range().start());
                groups.push(std::mem::take(&mut current));
            }
            K::TYPE_ARGUMENT_LIST if !in_parens && !closed => {}
            K::LAMBDA_EXPR if !in_parens => {
                // Trailing lambda: outside the parens, or the whole argument
                // list when the call has no parens.
                trailing_lambda = child.as_node().cloned();
            }
            _ => {
                if in_parens {
                    current.push(child);
                } else if !closed && callee.is_none() {
                    callee = Some(child);
                }
            }
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }

    let callee = callee?;
    let (name, receiver) = split_callee(&callee)?;

    let args = groups.into_iter().filter_map(argument_from_group).collect();

    Some(KotlinCall {
        node: node.clone(),
        name,
        receiver,
        l_paren,
        r_paren,
        commas,
        args,
        trailing_lambda,
    })
}

/// Split a callee expression into `(simple name, receiver)`.
///
/// `foo` → `("foo", None)`; `a.b` / `a?.b` → `("b", Some(a))`.
fn split_callee(callee: &KtElement) -> Option<(String, Option<KtElement>)> {
    match callee {
        rowan::NodeOrToken::Token(t) if is_name_token(t.kind()) => {
            Some((t.text().to_string(), None))
        }
        rowan::NodeOrToken::Node(n)
            if matches!(n.kind(), K::MEMBER_ACCESS_EXPR | K::SAFE_ACCESS_EXPR) =>
        {
            // [receiver, DOT | SAFE_CALL, IDENT]: the name is the last
            // identifier token, the receiver is everything before the operator.
            let mut receiver = None;
            let mut name = None;
            for child in n.children_with_tokens() {
                if child.kind().is_trivia() {
                    continue;
                }
                match &child {
                    rowan::NodeOrToken::Token(t)
                        if matches!(t.kind(), K::DOT | K::SAFE_CALL | K::COLONCOLON) => {}
                    rowan::NodeOrToken::Token(t) if is_name_token(t.kind()) => {
                        if receiver.is_none() {
                            receiver = Some(child.clone());
                        } else {
                            name = Some(t.text().to_string());
                        }
                    }
                    _ => {
                        if receiver.is_none() {
                            receiver = Some(child.clone());
                        }
                    }
                }
            }
            // `a.b`: the first identifier was banked as the receiver and the
            // second as the name. A lone identifier means the tree is malformed.
            Some((name?, receiver))
        }
        _ => None,
    }
}

/// Is this token kind usable as a Kotlin identifier here?
pub(crate) fn is_name_token(kind: K) -> bool {
    kind == K::BACKTICK_IDENT || crate::hover::kotlin::is_kotlin_identifier_like(kind)
}

/// Fully qualified name of an inferred receiver type, when it names a class.
///
/// Kotlin's own type surface is reached via
/// [`hyperion_semantics::semantics_type_to_kotlin`]; here we want the *index*
/// key instead, which is the package-qualified class name.
pub(crate) fn type_to_fqn(ty: &hyperion_semantics::Type) -> Option<String> {
    match ty {
        hyperion_semantics::Type::Class(class_type) => {
            Some(class_type.package.as_ref().map_or_else(
                || class_type.name.clone(),
                |pkg| format!("{pkg}.{}", class_type.name),
            ))
        }
        _ => None,
    }
}

/// Turn one comma-delimited run of children into an argument.
fn argument_from_group(group: Vec<KtElement>) -> Option<KotlinArgument> {
    let first = group.first()?;
    let last = group.last()?;
    let range = TextRange::new(first.text_range().start(), last.text_range().end());

    // `name = value` parses as BINARY_EXPR [IDENT, EQ, value]. `a == b` is also
    // a BINARY_EXPR, hence the check for `EQ` specifically.
    if group.len() == 1 {
        if let rowan::NodeOrToken::Node(n) = first {
            if n.kind() == K::BINARY_EXPR {
                if let Some(named) = split_named_argument(n) {
                    return Some(KotlinArgument {
                        range,
                        name: Some(named.0),
                        value: named.1,
                    });
                }
            }
        }
    }

    Some(KotlinArgument {
        range,
        name: None,
        value: Some(first.clone()),
    })
}

/// `name = value` → `(name, value)`, or `None` if this binary expression is not
/// a named argument.
fn split_named_argument(node: &SyntaxNode) -> Option<(String, Option<KtElement>)> {
    let mut name = None;
    let mut seen_eq = false;
    let mut value = None;

    for child in node.children_with_tokens() {
        if child.kind().is_trivia() {
            continue;
        }
        if child.kind() == K::EQ {
            // An `EQ` before any identifier is not a named argument.
            name.as_ref()?;
            seen_eq = true;
            continue;
        }
        if seen_eq {
            if value.is_none() {
                value = Some(child);
            }
            continue;
        }
        match &child {
            rowan::NodeOrToken::Token(t) if is_name_token(t.kind()) && name.is_none() => {
                name = Some(t.text().to_string());
            }
            // Anything other than a leading bare identifier disqualifies it.
            _ => return None,
        }
    }

    if seen_eq {
        Some((name?, value))
    } else {
        None
    }
}

/// The innermost call whose argument region contains `offset`.
///
/// Both biases of `token_at_offset` are consulted so that a cursor sitting
/// exactly on `(`, `,` or `)` still resolves to the call it belongs to.
pub(crate) fn call_at_offset(root: &SyntaxNode, offset: TextSize) -> Option<KotlinCall> {
    let clamped = offset.min(root.text_range().end());
    let tokens = root.token_at_offset(clamped);

    let mut best: Option<KotlinCall> = None;
    for token in tokens {
        let mut node = token.parent();
        let mut depth = 0;
        while let Some(current) = node {
            if depth > 32 {
                break;
            }
            if current.kind() == K::CALL_EXPR {
                if let Some(call) = analyze_call(&current) {
                    if call.covers_offset(offset) {
                        let better = best.as_ref().is_none_or(|existing| {
                            current.text_range().len() < existing.node.text_range().len()
                        });
                        if better {
                            best = Some(call);
                        }
                    }
                }
            }
            node = current.parent();
            depth += 1;
        }
    }
    best
}

/// Index of the parameter the cursor is currently editing.
///
/// Positional arguments are counted by their separators; a named argument
/// re-points the index at the parameter it names; a cursor inside a trailing
/// lambda selects the final parameter.
pub(crate) fn active_parameter(
    call: &KotlinCall,
    offset: TextSize,
    parameters: &[KotlinParameter],
) -> u32 {
    if let Some(lambda) = &call.trailing_lambda {
        if offset > lambda.text_range().start() {
            // The trailing lambda is the last argument; when the signature has
            // no parameters at all, clamp to 0 rather than underflow.
            let index = parameters.len().saturating_sub(1).max(call.args.len());
            return index as u32;
        }
    }

    let positional = call.commas.iter().filter(|comma| **comma < offset).count();

    if let Some(name) = call.args.get(positional).and_then(|arg| arg.name.as_ref()) {
        if let Some(index) = parameters.iter().position(|param| &param.name == name) {
            return index as u32;
        }
    }

    positional as u32
}

// ---------------------------------------------------------------------------
// Signature model
// ---------------------------------------------------------------------------

/// One parameter of a resolved Kotlin callable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KotlinParameter {
    pub name: String,
    /// Declared type as written, when known.
    pub type_text: Option<String>,
    pub is_vararg: bool,
}

impl KotlinParameter {
    /// Kotlin surface form: `name: Type`, or `vararg name: Type`.
    pub fn label(&self) -> String {
        let prefix = if self.is_vararg { "vararg " } else { "" };
        match &self.type_text {
            Some(ty) => format!("{prefix}{}: {ty}", self.name),
            None => format!("{prefix}{}", self.name),
        }
    }
}

/// A resolved callable, rendered for signature help.
#[derive(Debug, Clone)]
pub(crate) struct KotlinSignature {
    /// `greet(name: String, times: Int): String`.
    pub label: String,
    pub parameters: Vec<KotlinParameter>,
    /// `[start, end)` byte offsets of each parameter label inside `label`.
    pub parameter_spans: Vec<(u32, u32)>,
    pub documentation: Option<String>,
}

impl KotlinSignature {
    /// Render a callable. `return_type` is omitted for constructors and for
    /// functions declared without one.
    pub fn new(
        name: &str,
        parameters: Vec<KotlinParameter>,
        return_type: Option<&str>,
        documentation: Option<String>,
    ) -> Self {
        let mut label = format!("{name}(");
        let mut parameter_spans = Vec::with_capacity(parameters.len());
        for (index, parameter) in parameters.iter().enumerate() {
            if index > 0 {
                label.push_str(", ");
            }
            let start = label.len() as u32;
            label.push_str(&parameter.label());
            parameter_spans.push((start, label.len() as u32));
        }
        label.push(')');
        if let Some(return_type) = return_type {
            let return_type = return_type.trim();
            if !return_type.is_empty() {
                label.push_str(": ");
                label.push_str(return_type);
            }
        }

        Self {
            label,
            parameters,
            parameter_spans,
            documentation,
        }
    }
}

/// Pick the signature that best fits what has been typed so far.
///
/// Named arguments dominate: a signature that can accept every name used wins
/// over one that cannot, however well its arity matches. Within that, an exact
/// arity match beats a signature with room to spare, which beats one that is
/// already overfull.
pub(crate) fn best_signature(signatures: &[KotlinSignature], call: &KotlinCall) -> usize {
    let arg_count = call.arg_count();
    let used_names: Vec<&String> = call.args.iter().filter_map(|a| a.name.as_ref()).collect();

    let mut best_index = 0;
    let mut best_score = u32::MAX;

    for (index, signature) in signatures.iter().enumerate() {
        let unmatched_names = used_names
            .iter()
            .filter(|name| !signature.parameters.iter().any(|p| &&p.name == *name))
            .count() as u32;

        let arity = signature.parameters.len();
        let arity_score = match arity.cmp(&arg_count) {
            std::cmp::Ordering::Equal => 0,
            // Room to spare: the remaining parameters may still be typed.
            std::cmp::Ordering::Greater => (arity - arg_count) as u32,
            // Already overfull: this overload cannot be the one.
            std::cmp::Ordering::Less => 100 + (arg_count - arity) as u32,
        };

        let score = unmatched_names * 1000 + arity_score;
        if score < best_score {
            best_score = score;
            best_index = index;
        }
    }

    best_index
}

// ---------------------------------------------------------------------------
// Callee resolution
// ---------------------------------------------------------------------------

/// Resolves a call's callee to zero or more candidate signatures.
///
/// Declarations in the file being edited win over the workspace index: they are
/// always current, and — unlike the index, whose Kotlin FQNs carry no parameter
/// descriptor and therefore collapse overloads onto one entry — they preserve
/// every overload.
pub(crate) struct KotlinCalleeResolver<'a> {
    pub workspace_index: &'a Arc<WorkspaceIndex>,
    pub file_id: FileId,
    pub root: &'a SyntaxNode,
}

impl KotlinCalleeResolver<'_> {
    /// Candidate signatures for `call`, best-first.
    ///
    /// `receiver_fqn` is the inferred type of the receiver for a qualified
    /// call; pass `None` when the call is unqualified or the type is unknown.
    pub fn resolve(&self, call: &KotlinCall, receiver_fqn: Option<&str>) -> Vec<KotlinSignature> {
        let mut signatures = Vec::new();

        if call.receiver.is_none() {
            self.collect_local_functions(call, &mut signatures);
            self.collect_local_constructors(&call.name, &mut signatures);
        } else if let Some(fqn) = receiver_fqn {
            self.collect_members_of(fqn, &call.name, &mut signatures);
        }

        // Same-file declarations answered the call; don't dilute them with
        // unrelated same-named symbols from elsewhere in the workspace.
        if signatures.is_empty() && call.receiver.is_none() {
            self.collect_contextual_unqualified_candidates(call, &mut signatures);
        }

        dedupe(&mut signatures);
        signatures.truncate(MAX_SIGNATURES);
        signatures
    }

    /// `fun` declarations in this file that are visible from the call site.
    fn collect_local_functions(&self, call: &KotlinCall, out: &mut Vec<KotlinSignature>) {
        let offset = call.node.text_range().start();
        for node in self.root.descendants() {
            if node.kind() != K::FUNCTION_DECL {
                continue;
            }
            if function_name(&node).as_deref() != Some(call.name.as_str()) {
                continue;
            }
            if !is_visible_from(&node, offset) {
                continue;
            }
            out.push(signature_from_function(&node, &call.name));
        }
    }

    /// Constructors of a class/object declared in this file.
    fn collect_local_constructors(&self, name: &str, out: &mut Vec<KotlinSignature>) {
        for node in self.root.descendants() {
            if !matches!(node.kind(), K::CLASS_DECL | K::OBJECT_DECL) {
                continue;
            }
            if declaration_name(&node).as_deref() != Some(name) {
                continue;
            }

            // Primary constructor: the parameter list on the declaration itself.
            if let Some(list) = node
                .children()
                .find(|child| child.kind() == K::PARAMETER_LIST)
            {
                out.push(KotlinSignature::new(
                    name,
                    parameters_from_list(&list),
                    None,
                    None,
                ));
            }

            // Secondary constructors live in the class body. The workspace
            // index never sees these — its Kotlin extractor skips
            // CONSTRUCTOR_DECL entirely — so the CST is the only source.
            for body in node.children().filter(|c| c.kind() == K::CLASS_BODY) {
                for ctor in body.children().filter(|c| c.kind() == K::CONSTRUCTOR_DECL) {
                    let parameters = ctor
                        .children()
                        .find(|c| c.kind() == K::PARAMETER_LIST)
                        .map(|list| parameters_from_list(&list))
                        .unwrap_or_default();
                    out.push(KotlinSignature::new(name, parameters, None, None));
                }
            }
        }
    }

    /// Members of `class_fqn` named `name`, from the file first and the index second.
    fn collect_members_of(&self, class_fqn: &str, name: &str, out: &mut Vec<KotlinSignature>) {
        let simple = simple_name_of(class_fqn);

        // The declaring class may be in the buffer being edited, where it is
        // newer than the index and keeps its overloads distinct.
        for node in self.root.descendants() {
            if !matches!(
                node.kind(),
                K::CLASS_DECL | K::OBJECT_DECL | K::INTERFACE_DECL
            ) {
                continue;
            }
            if declaration_name(&node).as_deref() != Some(simple) {
                continue;
            }
            for body in node.children().filter(|c| c.kind() == K::CLASS_BODY) {
                for member in body.children().filter(|c| c.kind() == K::FUNCTION_DECL) {
                    if function_name(&member).as_deref() == Some(name) {
                        out.push(signature_from_function(&member, name));
                    }
                }
            }
        }
        if !out.is_empty() {
            return;
        }

        // Kotlin member FQNs are `pkg.Class.member` with no parameter
        // descriptor, so `find_methods_by_name` (which prefix-matches
        // `Class.member(`) never hits them. Scan the members instead.
        for symbol in self
            .workspace_index
            .get_class_members_with_inheritance(class_fqn)
        {
            if symbol.simple_name.as_ref() == name && is_callable(symbol.kind) {
                out.push(signature_from_symbol(&symbol, name));
            }
        }
    }

    /// Resolve an unqualified call from the exact file import/package context.
    /// Explicit bindings win over the current package, which wins over
    /// on-demand imports. Multiple wildcard owners are an ambiguity, not an
    /// overload set. Qualified calls with an unknown receiver never reach this
    /// path and deliberately produce no candidates.
    fn collect_contextual_unqualified_candidates(
        &self,
        call: &KotlinCall,
        out: &mut Vec<KotlinSignature>,
    ) {
        let Some(imports) = self.workspace_index.get_imports(self.file_id) else {
            return;
        };

        let callable = if let Some(explicit) = imports.explicit.get(&call.name) {
            self.callable_definition(explicit)
        } else {
            let package_fqn = imports.current_package.as_deref().map_or_else(
                || call.name.clone(),
                |package| format!("{package}.{}", call.name),
            );
            let package_candidate = self.callable_definition(&package_fqn);
            package_candidate.or_else(|| {
                let mut wildcard_candidates = imports
                    .wildcards
                    .iter()
                    .filter_map(|package| {
                        self.callable_definition(&format!("{package}.{}", call.name))
                    })
                    .collect::<Vec<_>>();
                wildcard_candidates
                    .sort_unstable_by(|left, right| left.target.fqn.cmp(&right.target.fqn));
                wildcard_candidates.dedup_by(|left, right| left.target == right.target);
                let [candidate] = wildcard_candidates.as_slice() else {
                    return None;
                };
                Some(candidate.clone())
            })
        };
        if let Some(definition) = callable {
            out.push(signature_from_symbol(&definition.symbol, &call.name));
        }

        let hyperion_workspace::TypeNameResolution::Exact(type_fqn) = self
            .workspace_index
            .resolve_type_name(self.file_id, &[], &call.name)
        else {
            return;
        };
        let Some(type_definition) =
            self.workspace_index
                .find_symbol_definition(&SymbolTarget::new(
                    type_fqn.as_str(),
                    DeclarationNamespace::Type,
                ))
        else {
            return;
        };
        if !is_type(type_definition.symbol.kind) {
            return;
        }
        let constructors = self.workspace_index.get_constructors_for_class(&type_fqn);
        if constructors.is_empty() {
            out.push(signature_from_symbol(&type_definition.symbol, &call.name));
        } else {
            for constructor in constructors {
                out.push(signature_from_symbol(&constructor, &call.name));
            }
        }
    }

    fn callable_definition(&self, fqn: &str) -> Option<hyperion_workspace::ResolvedDefinition> {
        let definition = self
            .workspace_index
            .find_symbol_definition(&SymbolTarget::new(fqn, DeclarationNamespace::Callable))?;
        is_callable(definition.symbol.kind).then_some(definition)
    }
}

/// Kinds that can be called directly.
const fn is_callable(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::FUNCTION | SymbolKind::METHOD | SymbolKind::CONSTRUCTOR
    )
}

/// Kinds whose name doubles as a constructor call.
const fn is_type(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::CLASS | SymbolKind::OBJECT)
}

fn simple_name_of(fqn: &str) -> &str {
    fqn.rsplit_once('.').map_or(fqn, |(_, simple)| simple)
}

fn dedupe(signatures: &mut Vec<KotlinSignature>) {
    let mut seen = std::collections::HashSet::new();
    signatures.retain(|signature| seen.insert(signature.label.clone()));
}

// ---------------------------------------------------------------------------
// CST readers
// ---------------------------------------------------------------------------

/// Name of a `FUNCTION_DECL`: the last identifier before the parameter list.
///
/// Handles `fun <T> name(…)` and the extension form `fun Receiver.name(…)`,
/// where several identifier-ish tokens precede the parameter list.
pub(crate) fn function_name(node: &SyntaxNode) -> Option<String> {
    let mut name = None;
    for child in node.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Node(n) if n.kind() == K::PARAMETER_LIST => break,
            rowan::NodeOrToken::Token(t) if is_name_token(t.kind()) => {
                name = Some(t.text().to_string());
            }
            _ => {}
        }
    }
    name
}

/// Name of a `CLASS_DECL` / `OBJECT_DECL` / `INTERFACE_DECL`.
fn declaration_name(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens().find_map(|child| match child {
        rowan::NodeOrToken::Token(t) if is_name_token(t.kind()) => Some(t.text().to_string()),
        _ => None,
    })
}

/// Is a declaration reachable from an unqualified call at `offset`?
///
/// Top-level declarations always are. A local `fun` is reachable only inside
/// its own block. A member is reachable inside its own type body — and,
/// because the walk continues upwards, from the enclosing class of a companion
/// object too.
fn is_visible_from(decl: &SyntaxNode, offset: TextSize) -> bool {
    let mut saw_type_body = false;
    let mut node = decl.parent();
    while let Some(current) = node {
        match current.kind() {
            K::SOURCE_FILE => return !saw_type_body,
            K::BLOCK => return current.text_range().contains(offset),
            K::CLASS_BODY => {
                if current.text_range().contains(offset) {
                    return true;
                }
                saw_type_body = true;
            }
            _ => {}
        }
        node = current.parent();
    }
    false
}

/// Parameters of a `PARAMETER_LIST`.
pub(crate) fn parameters_from_list(list: &SyntaxNode) -> Vec<KotlinParameter> {
    list.children()
        .filter(|child| child.kind() == K::PARAMETER)
        .filter_map(|param| {
            let mut name = None;
            let mut type_text = None;
            let mut is_vararg = false;
            let mut seen_colon = false;

            for child in param.children_with_tokens() {
                if child.kind().is_trivia() {
                    continue;
                }
                match &child {
                    rowan::NodeOrToken::Token(t) if t.kind() == K::VARARG_KW => is_vararg = true,
                    rowan::NodeOrToken::Token(t) if t.kind() == K::COLON => seen_colon = true,
                    // Everything after `=` is the default value, not the type.
                    rowan::NodeOrToken::Token(t) if t.kind() == K::EQ => break,
                    rowan::NodeOrToken::Token(t) if is_name_token(t.kind()) && name.is_none() => {
                        name = Some(t.text().to_string());
                    }
                    rowan::NodeOrToken::Node(n) if seen_colon && type_text.is_none() => {
                        type_text = Some(n.text().to_string().trim().to_string());
                    }
                    _ => {}
                }
            }

            Some(KotlinParameter {
                name: name?,
                type_text,
                is_vararg,
            })
        })
        .collect()
}

/// Build a signature from a `FUNCTION_DECL`.
fn signature_from_function(node: &SyntaxNode, name: &str) -> KotlinSignature {
    let parameters = node
        .children()
        .find(|child| child.kind() == K::PARAMETER_LIST)
        .map(|list| parameters_from_list(&list))
        .unwrap_or_default();

    // The return type is the type node that follows the parameter list; any
    // type node *before* it is an extension receiver.
    let mut seen_params = false;
    let return_type = node.children().find_map(|child| {
        if child.kind() == K::PARAMETER_LIST {
            seen_params = true;
            return None;
        }
        if seen_params
            && matches!(
                child.kind(),
                K::TYPE_REF | K::NULLABLE_TYPE | K::FUNCTION_TYPE
            )
        {
            return Some(child.text().to_string().trim().to_string());
        }
        None
    });

    KotlinSignature::new(name, parameters, return_type.as_deref(), None)
}

/// Build a signature from a workspace-index symbol.
///
/// Index parameters are `"Type name"` strings (`"vararg Type name"` for
/// varargs), matching the Java extractor's descriptor shape.
fn signature_from_symbol(symbol: &hyperion_workspace::Symbol, name: &str) -> KotlinSignature {
    let parameters = symbol
        .parameters
        .iter()
        .map(|raw| parse_indexed_parameter(raw))
        .collect();
    KotlinSignature::new(
        name,
        parameters,
        symbol.return_type.as_deref(),
        symbol.documentation.as_ref().map(|d| d.to_string()),
    )
}

/// Parse one `"Type name"` index entry back into a parameter.
fn parse_indexed_parameter(raw: &str) -> KotlinParameter {
    let raw = raw.trim();
    let (raw, is_vararg) = raw
        .strip_prefix("vararg ")
        .map_or((raw, false), |rest| (rest.trim(), true));

    // Split on the *last* space: the type may itself contain spaces
    // (`Map<String, Int> m`), the name never does.
    match raw.rsplit_once(char::is_whitespace) {
        Some((type_text, name)) if !name.is_empty() => KotlinParameter {
            name: name.to_string(),
            type_text: Some(type_text.trim().to_string()),
            is_vararg,
        },
        // A bare entry is a name with no recorded type.
        _ => KotlinParameter {
            name: raw.to_string(),
            type_text: None,
            is_vararg,
        },
    }
}
