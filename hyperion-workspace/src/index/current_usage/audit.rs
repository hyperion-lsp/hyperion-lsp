// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Fail-closed lexical and dynamic-name audits for destructive refactorings.

use super::super::query::find_symbol_definition_coherent_in;
use super::*;
use std::collections::VecDeque;

fn contains_identifier_spelling(haystack: &str, needle: &str) -> bool {
    !identifier_spelling_offsets(haystack, needle).is_empty()
}

fn identifier_spelling_offsets(haystack: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    let is_identifier_part =
        |character: char| character == '_' || character == '$' || character.is_alphanumeric();
    haystack
        .match_indices(needle)
        .filter_map(|(start, spelling)| {
            let before = haystack[..start].chars().next_back();
            let after = haystack[start + spelling.len()..].chars().next();
            (!before.is_some_and(is_identifier_part) && !after.is_some_and(is_identifier_part))
                .then_some(start)
        })
        .collect()
}

const DYNAMIC_SYMBOL_LOOKUPS: &[&str] = &[
    "bind",
    "findClass",
    "findConstructor",
    "findGetter",
    "findSetter",
    "findSpecial",
    "findStatic",
    "findStaticGetter",
    "findStaticSetter",
    "findStaticVarHandle",
    "findVarHandle",
    "findVirtual",
    "forName",
    "fromMethodDescriptorString",
    "getConstructor",
    "getDeclaredConstructor",
    "getDeclaredField",
    "getDeclaredMethod",
    "getField",
    "getMethod",
    "loadClass",
    "unreflect",
    "unreflectConstructor",
    "unreflectGetter",
    "unreflectSetter",
    "unreflectSpecial",
    "unreflectVarHandle",
];

fn is_nominal_subtype_of(index: &WorkspaceIndex, candidate: &str, expected: &str) -> bool {
    let mut pending = VecDeque::from([candidate.to_string()]);
    let mut visited = HashSet::new();
    while let Some(candidate) = pending.pop_front() {
        if !visited.insert(candidate.clone()) {
            continue;
        }
        if candidate == expected {
            return true;
        }
        pending.extend(index.direct_supertypes(&candidate));
    }
    false
}

fn dynamic_lookup_owner_accepts(index: &WorkspaceIndex, owner: &str, method: &str) -> bool {
    match method {
        "forName"
        | "getConstructor"
        | "getDeclaredConstructor"
        | "getDeclaredField"
        | "getDeclaredMethod"
        | "getField"
        | "getMethod" => owner == "java.lang.Class",
        "loadClass" => is_nominal_subtype_of(index, owner, "java.lang.ClassLoader"),
        "fromMethodDescriptorString" => owner == "java.lang.invoke.MethodType",
        "bind"
        | "findClass"
        | "findConstructor"
        | "findGetter"
        | "findSetter"
        | "findSpecial"
        | "findStatic"
        | "findStaticGetter"
        | "findStaticSetter"
        | "findStaticVarHandle"
        | "findVarHandle"
        | "findVirtual"
        | "unreflect"
        | "unreflectConstructor"
        | "unreflectGetter"
        | "unreflectSetter"
        | "unreflectSpecial"
        | "unreflectVarHandle" => matches!(
            owner,
            "java.lang.invoke.MethodHandles.Lookup" | "java.lang.invoke.MethodHandles$Lookup"
        ),
        _ => false,
    }
}

/// Is a callee spelled like one of [`DYNAMIC_SYMBOL_LOOKUPS`] actually a
/// reflective lookup, for the purposes of blocking a destructive refactoring?
///
/// The burden of proof runs the other way round from the obvious reading. This
/// module is fail-closed (see the module docs): a safe delete may only proceed
/// once every name-bearing construct in the workspace has been *proven* not to
/// reach the declaration. `dynamic_lookup_owner_accepts` exists to discharge
/// exactly one such proof -- "this `getMethod` is the user's own method, not
/// `java.lang.Class`'s" -- and discharging it requires a resolved binding whose
/// owner is known. When no usage was recorded at the callee's range at all, the
/// owner is *unknown*, and an unknown owner proves nothing.
///
/// Requiring positive proof that the owner *is* a reflection API inverted that:
/// every gap in usage extraction turned silently into a fail-*open* delete.
/// Two such gaps are why `MethodHandles.lookup().bind(..)` and `Class::forName`
/// went undetected -- a receiver chain cannot step through an externally
/// (bytecode) indexed method's return type, and a descriptor-less method
/// reference to an overloaded target resolves to no single candidate -- and any
/// future gap would have re-opened the hole in the same way.
///
/// So: block unless the callee resolves to something whose owner is known and
/// is not a reflection entry point. A same-named user method still resolves and
/// is still cleared; an unresolvable one now costs an unavailable safe delete
/// rather than a silently unsound edit.
fn is_dynamic_symbol_lookup(
    index: &WorkspaceIndex,
    usages: &[crate::usage_extractor::Usage],
    range: crate::TextRange,
    method: &str,
) -> bool {
    let Some(usage) = usages.iter().find(|usage| {
        usage.location.range == range
            && usage.location.kind == SymbolKind::METHOD
            && usage.location.name.as_ref() == method
    }) else {
        return true;
    };
    let descriptorless = usage
        .fqn
        .split_once('(')
        .map_or(usage.fqn.as_str(), |pair| pair.0);
    descriptorless
        .rsplit_once('.')
        .is_none_or(|(owner, resolved_method)| {
            resolved_method != method || dynamic_lookup_owner_accepts(index, owner, method)
        })
}

fn method_call_name_token(
    call: &hyperion_parser::SyntaxNode,
) -> Option<hyperion_parser::SyntaxToken> {
    use hyperion_parser::SyntaxKind;

    // Qualified calls retain the callee identifier as a direct token. For an
    // unqualified call it is wrapped in the direct NAME_EXPR child. Looking
    // only in these two positions is important: a reflection API name passed
    // as an argument or used in a receiver expression is not the callee.
    call.children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_identifier())
        .last()
        .or_else(|| {
            call.children()
                .find(|child| child.kind() == SyntaxKind::NAME_EXPR)
                .and_then(|name| {
                    name.descendants_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .filter(|token| token.kind().is_identifier())
                        .last()
                })
        })
}

fn java_directly_declares(node: &hyperion_parser::SyntaxNode, name: &str) -> bool {
    use hyperion_parser::SyntaxKind;

    let boundary_kind = match node.kind() {
        SyntaxKind::FOREACH_STMT => Some(SyntaxKind::COLON),
        SyntaxKind::LAMBDA_EXPR => Some(SyntaxKind::ARROW),
        SyntaxKind::CATCH_CLAUSE => Some(SyntaxKind::R_PAREN),
        _ => None,
    };
    let boundary = boundary_kind.and_then(|boundary_kind| {
        node.children_with_tokens()
            .find_map(|element| {
                element
                    .into_token()
                    .filter(|token| token.kind() == boundary_kind)
            })
            .map(|token| token.text_range().start())
    });
    node.children_with_tokens().any(|element| match element {
        rowan::NodeOrToken::Token(token) => {
            token.kind().is_identifier()
                && token.text() == name
                && boundary.is_none_or(|end| token.text_range().start() < end)
        }
        rowan::NodeOrToken::Node(parameter) if parameter.kind() == SyntaxKind::PARAM => parameter
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier())
            .last()
            .is_some_and(|token| token.text() == name),
        _ => false,
    })
}

fn java_token_has_visible_local_declaration(token: &hyperion_parser::SyntaxToken) -> bool {
    use hyperion_parser::SyntaxKind;

    let mut previous = token.prev_token();
    while previous
        .as_ref()
        .is_some_and(|token| token.kind().is_trivia())
    {
        previous = previous.and_then(|token| token.prev_token());
    }
    if previous.is_some_and(|token| token.kind() == SyntaxKind::DOT) {
        return false;
    }
    let name = token.text();
    let use_start = token.text_range().start();
    let Some(mut child_on_path) = token.parent() else {
        return false;
    };
    if matches!(
        child_on_path.kind(),
        SyntaxKind::PARAM
            | SyntaxKind::TYPE_PARAM
            | SyntaxKind::LOCAL_VAR_DECL
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::TYPE_PATTERN
            | SyntaxKind::RECORD_PATTERN
    ) && java_directly_declares(&child_on_path, name)
    {
        return true;
    }
    for ancestor in child_on_path.ancestors().skip(1) {
        if ancestor.kind() == SyntaxKind::LOCAL_VAR_DECL
            && java_directly_declares(&ancestor, name)
            && ancestor
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|candidate| candidate.kind().is_identifier() && candidate.text() == name)
                .any(|candidate| candidate.text_range().start() <= use_start)
        {
            return true;
        }
        for sibling in ancestor.children() {
            if sibling == child_on_path {
                break;
            }
            if sibling.kind() == SyntaxKind::LOCAL_VAR_DECL
                && java_directly_declares(&sibling, name)
            {
                return true;
            }
        }
        if ancestor
            .children()
            .filter(|child| child.kind() == SyntaxKind::TYPE_PARAM_LIST)
            .flat_map(|parameters| parameters.children())
            .any(|parameter| {
                parameter.kind() == SyntaxKind::TYPE_PARAM
                    && java_directly_declares(&parameter, name)
            })
        {
            return true;
        }
        match ancestor.kind() {
            SyntaxKind::FOREACH_STMT | SyntaxKind::CATCH_CLAUSE | SyntaxKind::LAMBDA_EXPR
                if java_directly_declares(&ancestor, name) =>
            {
                return true;
            }
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => {
                return ancestor
                    .children()
                    .filter(|child| child.kind() == SyntaxKind::PARAM_LIST)
                    .flat_map(|parameters| parameters.children())
                    .any(|parameter| {
                        parameter.kind() == SyntaxKind::PARAM
                            && java_directly_declares(&parameter, name)
                    });
            }
            _ => {}
        }
        child_on_path = ancestor;
    }
    false
}

fn dynamic_symbol_lookup_in(
    index: &WorkspaceIndex,
    root: &hyperion_parser::SyntaxNode,
    is_ignored: &impl Fn(crate::TextRange) -> bool,
    target_names: &HashSet<&str>,
    usages: &[crate::usage_extractor::Usage],
    is_cancelled: &impl Fn() -> bool,
) -> std::result::Result<Option<hyperion_parser::SyntaxToken>, CurrentUsageQueryError> {
    for (scan_index, call) in root
        .descendants()
        .filter(|node| node.kind() == hyperion_parser::SyntaxKind::METHOD_CALL_EXPR)
        .enumerate()
    {
        if scan_index % 128 == 0 && is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if let Some(token) = method_call_name_token(&call) {
            let range = crate::TextRange::new(
                u32::from(token.text_range().start()),
                u32::from(token.text_range().end()),
            );
            if !is_ignored(range)
                && DYNAMIC_SYMBOL_LOOKUPS.contains(&token.text())
                && is_dynamic_symbol_lookup(index, usages, range, token.text())
                && !java_call_has_proven_unrelated_literal_name(&call, target_names)
            {
                return Ok(Some(token));
            }
        }
    }
    for (scan_index, reference) in root
        .descendants()
        .filter(|node| node.kind() == hyperion_parser::SyntaxKind::METHOD_REF_EXPR)
        .enumerate()
    {
        if scan_index % 128 == 0 && is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if let Some(token) = reference
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier())
            .last()
        {
            let range = crate::TextRange::new(
                u32::from(token.text_range().start()),
                u32::from(token.text_range().end()),
            );
            if !is_ignored(range)
                && DYNAMIC_SYMBOL_LOOKUPS.contains(&token.text())
                && is_dynamic_symbol_lookup(index, usages, range, token.text())
            {
                return Ok(Some(token));
            }
        }
    }
    Ok(None)
}

fn java_call_has_proven_unrelated_literal_name(
    call: &hyperion_parser::SyntaxNode,
    target_names: &HashSet<&str>,
) -> bool {
    let mut literals = call
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_string_literal());
    let Some(literal) = literals.next() else {
        return false;
    };
    literals.next().is_none()
        && literal_is_direct_java_call_argument(&literal, call)
        && !literal.text().contains('\\')
        && !call
            .descendants_with_tokens()
            .any(|element| element.kind() == hyperion_parser::SyntaxKind::PLUS)
        && target_names
            .iter()
            .all(|name| !contains_identifier_spelling(literal.text(), name))
}

fn literal_is_direct_java_call_argument(
    literal: &hyperion_parser::SyntaxToken,
    call: &hyperion_parser::SyntaxNode,
) -> bool {
    let Some(mut node) = literal.parent() else {
        return false;
    };
    while node != *call {
        if !matches!(
            node.kind(),
            hyperion_parser::SyntaxKind::LITERAL_EXPR
                | hyperion_parser::SyntaxKind::PAREN_EXPR
                | hyperion_parser::SyntaxKind::ARG_LIST
        ) {
            return false;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
    true
}

fn kotlin_call_name_token(
    call: &hyperion_kotlin::SyntaxNode,
) -> Option<hyperion_kotlin::SyntaxToken> {
    use hyperion_kotlin::SyntaxKind;

    let mut candidate = None;
    for element in call.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::L_PAREN => break,
            rowan::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IDENT {
                    candidate = Some(token);
                }
            }
            rowan::NodeOrToken::Node(node) => {
                if node.kind() == SyntaxKind::TYPE_ARGUMENT_LIST {
                    continue;
                }
                if let Some(identifier) = node
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| token.kind() == SyntaxKind::IDENT)
                    .last()
                {
                    candidate = Some(identifier);
                }
            }
        }
    }
    candidate
}

fn kotlin_token_is_declaration_name(token: &hyperion_kotlin::SyntaxToken) -> bool {
    use hyperion_kotlin::SyntaxKind;

    token.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            SyntaxKind::PROPERTY_DECL
                | SyntaxKind::PARAMETER
                | SyntaxKind::FOR_LOOP_VAR
                | SyntaxKind::LAMBDA_PARAMETERS
                | SyntaxKind::TYPE_PARAMETER
                | SyntaxKind::DESTRUCTURING_DECL
        )
    })
}

fn kotlin_token_is_qualified(token: &hyperion_kotlin::SyntaxToken) -> bool {
    let mut previous = token.prev_token();
    while previous
        .as_ref()
        .is_some_and(|token| token.kind().is_trivia())
    {
        previous = previous.and_then(|token| token.prev_token());
    }
    previous.is_some_and(|token| token.kind() == hyperion_kotlin::SyntaxKind::DOT)
}

fn kotlin_dynamic_symbol_lookup_in(
    index: &WorkspaceIndex,
    root: &hyperion_kotlin::SyntaxNode,
    is_ignored: &impl Fn(crate::TextRange) -> bool,
    target_names: &HashSet<&str>,
    usages: &[crate::usage_extractor::Usage],
    is_cancelled: &impl Fn() -> bool,
) -> std::result::Result<Option<hyperion_kotlin::SyntaxToken>, CurrentUsageQueryError> {
    for (scan_index, call) in root
        .descendants()
        .filter(|node| node.kind() == hyperion_kotlin::SyntaxKind::CALL_EXPR)
        .enumerate()
    {
        if scan_index % 128 == 0 && is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if let Some(token) = kotlin_call_name_token(&call) {
            let range = crate::TextRange::new(
                u32::from(token.text_range().start()),
                u32::from(token.text_range().end()),
            );
            if !is_ignored(range)
                && DYNAMIC_SYMBOL_LOOKUPS.contains(&token.text())
                && is_dynamic_symbol_lookup(index, usages, range, token.text())
                && !kotlin_call_has_proven_unrelated_literal_name(&call, target_names)
            {
                return Ok(Some(token));
            }
        }
    }
    // A resolved dynamic API usage outside a call expression is a callable
    // reference (for example `Class::forName`); its future argument is not
    // available to this proof.
    for (scan_index, usage) in usages.iter().enumerate() {
        if scan_index % 128 == 0 && is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        let range = usage.location.range;
        // This loop is driven by the recorded usages themselves, so the only
        // question left for `is_dynamic_symbol_lookup` is whether the owner
        // clears the callee. Restrict it to method usages so a Kotlin property
        // that merely shares a reflection API's name is not read as one.
        if usage.location.kind != SymbolKind::METHOD
            || !DYNAMIC_SYMBOL_LOOKUPS.contains(&usage.location.name.as_ref())
            || !is_dynamic_symbol_lookup(index, usages, range, usage.location.name.as_ref())
        {
            continue;
        }
        let mut matching_token = None;
        for (token_index, token) in root
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .enumerate()
        {
            if token_index % 256 == 0 && is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            if crate::TextRange::new(
                u32::from(token.text_range().start()),
                u32::from(token.text_range().end()),
            ) == range
            {
                matching_token = Some(token);
                break;
            }
        }
        if let Some(token) = matching_token {
            let is_call = token.parent().is_some_and(|parent| {
                parent
                    .ancestors()
                    .any(|node| node.kind() == hyperion_kotlin::SyntaxKind::CALL_EXPR)
            });
            if !is_call && !is_ignored(range) {
                return Ok(Some(token));
            }
        }
    }
    Ok(None)
}

fn kotlin_call_has_proven_unrelated_literal_name(
    call: &hyperion_kotlin::SyntaxNode,
    target_names: &HashSet<&str>,
) -> bool {
    let mut literals = call
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|token| token.kind().is_string_literal());
    let Some(literal) = literals.next() else {
        return false;
    };
    literals.next().is_none()
        && literal_is_direct_kotlin_call_argument(&literal, call)
        && !literal.text().contains('\\')
        && !literal.text().contains('$')
        && !call
            .descendants_with_tokens()
            .any(|element| element.kind() == hyperion_kotlin::SyntaxKind::PLUS)
        && target_names
            .iter()
            .all(|name| !contains_identifier_spelling(literal.text(), name))
}

fn literal_is_direct_kotlin_call_argument(
    literal: &hyperion_kotlin::SyntaxToken,
    call: &hyperion_kotlin::SyntaxNode,
) -> bool {
    let Some(mut node) = literal.parent() else {
        return false;
    };
    while node != *call {
        if !matches!(
            node.kind(),
            hyperion_kotlin::SyntaxKind::LITERAL_EXPR | hyperion_kotlin::SyntaxKind::PAREN_EXPR
        ) {
            return false;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn collect_tree_sitter_audit_ranges<'a>(
    index: &WorkspaceIndex,
    node: tree_sitter::Node<'_>,
    source: &str,
    target_names: &HashSet<&'a str>,
    is_ignored: &impl Fn(crate::TextRange) -> bool,
    identifier_ranges: &mut Vec<crate::TextRange>,
    declaration_ranges: &mut Vec<crate::TextRange>,
    name_bearing_evidence: &mut Option<&'a str>,
    dynamic_lookup: &mut Option<String>,
    usages: &[crate::usage_extractor::Usage],
    is_cancelled: &impl Fn() -> bool,
) -> std::result::Result<(), CurrentUsageQueryError> {
    if is_cancelled() {
        return Err(CurrentUsageQueryError::Cancelled);
    }
    let Ok(start) = u32::try_from(node.start_byte()) else {
        return Ok(());
    };
    let Ok(end) = u32::try_from(node.end_byte()) else {
        return Ok(());
    };
    let range = crate::TextRange::new(start, end);
    if is_ignored(range) {
        return Ok(());
    }
    let kind = node.kind();
    let text = source.get(node.byte_range()).unwrap_or_default();
    let is_comment = kind.contains("comment");
    let is_string = kind.contains("string") || kind.contains("gstring");
    if is_string || (is_comment && (text.contains("{@link") || text.contains("@see"))) {
        if name_bearing_evidence.is_none() {
            *name_bearing_evidence = target_names
                .iter()
                .find(|name| contains_identifier_spelling(text, name))
                .copied();
        }
        return Ok(());
    }
    if is_comment {
        return Ok(());
    }

    if dynamic_lookup.is_none()
        && (kind.contains("invocation")
            || kind.contains("call_expression")
            || kind.contains("function_call"))
    {
        let callee = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("function"))
            .or_else(|| node.child_by_field_name("method"));
        if let Some(callee) = callee {
            let callee_text = source.get(callee.byte_range()).unwrap_or_default();
            if let Some(name) = DYNAMIC_SYMBOL_LOOKUPS
                .iter()
                .find(|name| callee_text.rsplit('.').next() == Some(**name))
            {
                let mut cursor = callee.walk();
                let callee_range = callee
                    .children(&mut cursor)
                    .filter(|child| child.child_count() == 0)
                    .find(|child| source.get(child.byte_range()) == Some(*name))
                    .and_then(|child| {
                        Some(crate::TextRange::new(
                            u32::try_from(child.start_byte()).ok()?,
                            u32::try_from(child.end_byte()).ok()?,
                        ))
                    })
                    .unwrap_or_else(|| {
                        crate::TextRange::new(
                            u32::try_from(callee.start_byte()).unwrap_or(u32::MAX),
                            u32::try_from(callee.end_byte()).unwrap_or(u32::MAX),
                        )
                    });
                if is_dynamic_symbol_lookup(index, usages, callee_range, name) {
                    *dynamic_lookup = Some((*name).to_string());
                }
            }
        }
    }

    if node.child_count() == 0 {
        if target_names.contains(text) {
            identifier_ranges.push(range);
            if tree_sitter_node_is_declaration_name(node) {
                declaration_ranges.push(range);
            }
        }
        return Ok(());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_sitter_audit_ranges(
            index,
            child,
            source,
            target_names,
            is_ignored,
            identifier_ranges,
            declaration_ranges,
            name_bearing_evidence,
            dynamic_lookup,
            usages,
            is_cancelled,
        )?;
    }
    Ok(())
}

fn tree_sitter_node_is_declaration_name(node: tree_sitter::Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let parent_kind = parent.kind();
    let declaration_like = parent_kind.contains("declaration")
        || parent_kind.contains("definition")
        || parent_kind.contains("parameter")
        || parent_kind.contains("declarator")
        || parent_kind.contains("binding");
    declaration_like
        && parent
            .child_by_field_name("name")
            .is_some_and(|name| name.id() == node.id())
}
impl WorkspaceIndex {
    pub(super) fn audit_current_identifier_bindings(
        &self,
        snapshot: &CurrentUsageScanSnapshot,
        input: &CurrentUsageScanInput,
        indexed: &crate::IndexedContent,
        root: &hyperion_parser::SyntaxNode,
        usages: &[crate::usage_extractor::Usage],
        is_cancelled: &impl Fn() -> bool,
    ) -> std::result::Result<(), CurrentUsageQueryError> {
        let target_names = snapshot
            .targets
            .iter()
            .filter_map(CurrentUsageTarget::lexical_identifier)
            .collect::<HashSet<_>>();
        let is_ignored = |range: crate::TextRange| {
            snapshot
                .ignored_identifier_ranges
                .iter()
                .any(|(revision, ignored)| {
                    *revision == input.revision
                        && ignored.start <= range.start
                        && range.end <= ignored.end
                })
        };

        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if let Some(lookup) =
            dynamic_symbol_lookup_in(self, root, &is_ignored, &target_names, usages, is_cancelled)?
        {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} contains dynamic symbol lookup through '{}', whose argument cannot be proven constant",
                input.file_path.display(),
                lookup.text()
            )));
        }

        // Ranges of identifiers already proven to be exact usages of a target
        // (including type spellings in imports and Javadoc references).
        let exact_usage_ranges = usages
            .iter()
            .filter(|usage| {
                snapshot.targets.iter().any(|target| {
                    target.classify(&usage.fqn, usage.location.kind) == CurrentUsageMatch::Exact
                })
            })
            .map(|usage| usage.location.range)
            .collect::<HashSet<_>>();
        let mut name_bearing_evidence = None;
        for (index, token) in root
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| {
                token.kind().is_string_literal()
                    || token.kind() == hyperion_parser::SyntaxKind::DOC_COMMENT
            })
            .enumerate()
        {
            if index % 128 == 0 && is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            let range = crate::TextRange::new(
                u32::from(token.text_range().start()),
                u32::from(token.text_range().end()),
            );
            if is_ignored(range) {
                continue;
            }
            if token.kind() == hyperion_parser::SyntaxKind::DOC_COMMENT {
                // Prose and `{@code}` mentions are inert. Only the reference
                // expression of a link/see/throws tag names a symbol; each such
                // spelling must be an accounted usage or the rename is unsafe.
                let base = u32::from(token.text_range().start());
                for (start, end) in
                    crate::usage_extractor::import_ref::javadoc_reference_spans(token.text())
                {
                    let reference = &token.text()[start..end];
                    for name in &target_names {
                        for offset in identifier_spelling_offsets(reference, name) {
                            let begin = base + (start + offset) as u32;
                            let spelled = crate::TextRange::new(begin, begin + name.len() as u32);
                            if !exact_usage_ranges.contains(&spelled) {
                                name_bearing_evidence = Some(*name);
                            }
                        }
                    }
                }
            } else {
                name_bearing_evidence = target_names
                    .iter()
                    .find(|name| contains_identifier_spelling(token.text(), name))
                    .copied();
            }
            if name_bearing_evidence.is_some() {
                break;
            }
        }
        if let Some(name) = name_bearing_evidence {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} contains a possible name-based refactoring use of '{}' in a string or documentation link",
                input.file_path.display(),
                name
            )));
        }

        let mut identifier_ranges = Vec::new();
        let mut declaration_ranges = Vec::new();
        for (index, token) in root
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .enumerate()
        {
            if index % 256 == 0 && is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            if token.kind().is_identifier() && target_names.iter().any(|name| *name == token.text())
            {
                let range = crate::TextRange::new(
                    u32::from(token.text_range().start()),
                    u32::from(token.text_range().end()),
                );
                identifier_ranges.push(range);
                if java_token_has_visible_local_declaration(&token) {
                    declaration_ranges.push(range);
                }
            }
        }
        self.audit_identifier_ranges(
            IdentifierRangeAudit {
                snapshot,
                input,
                indexed,
                usages,
                identifier_ranges: &identifier_ranges,
                declaration_ranges: &declaration_ranges,
            },
            is_cancelled,
        )
    }

    fn audit_identifier_ranges(
        &self,
        audit: IdentifierRangeAudit<'_>,
        is_cancelled: &impl Fn() -> bool,
    ) -> std::result::Result<(), CurrentUsageQueryError> {
        let IdentifierRangeAudit {
            snapshot,
            input,
            indexed,
            usages,
            identifier_ranges,
            declaration_ranges,
        } = audit;
        let target_names = snapshot
            .targets
            .iter()
            .filter_map(CurrentUsageTarget::lexical_identifier)
            .collect::<HashSet<_>>();
        let is_ignored = |range: crate::TextRange| {
            snapshot
                .ignored_identifier_ranges
                .iter()
                .any(|(revision, ignored)| {
                    *revision == input.revision
                        && ignored.start <= range.start
                        && range.end <= ignored.end
                })
        };

        let inner = self.inner.read();
        if inner.workspace_revision != snapshot.workspace_revision
            || inner.revision_token(input.revision.file_id()) != Some(input.revision)
        {
            return Err(CurrentUsageQueryError::ConcurrentModification);
        }
        let mut accounted_usage_identifiers = HashSet::new();
        for (index, usage) in usages.iter().enumerate() {
            if index % 256 == 0 && is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            let name = usage.location.name.as_ref();
            if !target_names.contains(name)
                || usage.location.file_id != input.revision.file_id()
                || usage.location.range.start > usage.location.range.end
                || usage.location.range.end as usize > indexed.content.len()
                || indexed
                    .content
                    .get(usage.location.range.start as usize..usage.location.range.end as usize)
                    != Some(name)
            {
                continue;
            }
            let classification = snapshot
                .targets
                .iter()
                .map(|target| target.classify(&usage.fqn, usage.location.kind))
                .max_by_key(|classification| match classification {
                    CurrentUsageMatch::No => 0,
                    CurrentUsageMatch::AmbiguousMethod => 1,
                    CurrentUsageMatch::Exact => 2,
                })
                .unwrap_or(CurrentUsageMatch::No);
            let accounted = match classification {
                CurrentUsageMatch::Exact => true,
                CurrentUsageMatch::AmbiguousMethod => {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "{} contains an overload-ambiguous usage of {}",
                        input.file_path.display(),
                        usage.fqn
                    )));
                }
                // A non-target extraction is proof only when the exact
                // namespace-qualified declaration exists in this same index
                // revision. Import/type resolvers retain ambiguity instead of
                // choosing by source order, so unresolved tokens fall through
                // to the lexical fail-closed pass below.
                CurrentUsageMatch::No => {
                    let namespace = crate::DeclarationNamespace::for_kind(usage.location.kind);
                    namespace != crate::DeclarationNamespace::Other
                        && find_symbol_definition_coherent_in(
                            &inner,
                            &crate::SymbolTarget::new(usage.fqn.as_str(), namespace),
                        )
                        .is_some()
                }
            };
            if accounted {
                accounted_usage_identifiers.insert(usage.location.range);
            }
        }
        drop(inner);

        for (index, &range) in identifier_ranges.iter().enumerate() {
            if index % 256 == 0 && is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            let Some(name) = indexed
                .content
                .get(range.start as usize..range.end as usize)
            else {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "{} contains an invalid lexical audit range {}..{}",
                    input.file_path.display(),
                    range.start,
                    range.end
                )));
            };
            let is_ignored = is_ignored(range);
            let is_declaration = declaration_ranges.contains(&range)
                || input
                    .known_declaration_identifiers
                    .iter()
                    .any(|(known_name, known_range)| {
                        known_name.as_ref() == name && *known_range == range
                    });
            let is_accounted_usage = accounted_usage_identifiers.contains(&range);
            if !is_ignored && !is_declaration && !is_accounted_usage {
                return Err(CurrentUsageQueryError::Unavailable(format!(
                    "{} contains an unresolved refactoring candidate '{}' at {}..{}",
                    input.file_path.display(),
                    name,
                    range.start,
                    range.end
                )));
            }
        }
        Ok(())
    }

    pub(super) fn audit_non_java_identifier_bindings(
        &self,
        snapshot: &CurrentUsageScanSnapshot,
        input: &CurrentUsageScanInput,
        indexed: &crate::IndexedContent,
        usages: &[crate::usage_extractor::Usage],
        is_cancelled: &impl Fn() -> bool,
    ) -> std::result::Result<(), CurrentUsageQueryError> {
        let target_names = snapshot
            .targets
            .iter()
            .filter_map(CurrentUsageTarget::lexical_identifier)
            .collect::<HashSet<_>>();
        let is_ignored = |range: crate::TextRange| {
            snapshot
                .ignored_identifier_ranges
                .iter()
                .any(|(revision, ignored)| {
                    *revision == input.revision
                        && ignored.start <= range.start
                        && range.end <= ignored.end
                })
        };

        let mut identifier_ranges = Vec::new();
        let mut declaration_ranges = Vec::new();
        let mut name_bearing_evidence = None;
        let mut dynamic_lookup = None;
        match input.language {
            Language::Java => unreachable!("Java has a dedicated strict audit"),
            Language::Kotlin => {
                let mut tokens = Vec::new();
                for (index, token) in hyperion_kotlin::Lexer::new(&indexed.content).enumerate() {
                    if index % 256 == 0 && is_cancelled() {
                        return Err(CurrentUsageQueryError::Cancelled);
                    }
                    tokens.push(token);
                }
                let (green, errors) = hyperion_kotlin::Parser::parse(tokens);
                if is_cancelled() {
                    return Err(CurrentUsageQueryError::Cancelled);
                }
                if !errors.is_empty() {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "{} has {} parse error(s)",
                        input.file_path.display(),
                        errors.len()
                    )));
                }
                let root = hyperion_kotlin::SyntaxNode::new_root(green);
                let scope_graph = hyperion_kotlin::build_scope_graph(&root);
                for (index, token) in root
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .enumerate()
                {
                    if index % 256 == 0 && is_cancelled() {
                        return Err(CurrentUsageQueryError::Cancelled);
                    }
                    let range = crate::TextRange::new(
                        u32::from(token.text_range().start()),
                        u32::from(token.text_range().end()),
                    );
                    if is_ignored(range) {
                        continue;
                    }
                    if token.kind() == hyperion_kotlin::SyntaxKind::IDENT
                        && target_names.iter().any(|name| *name == token.text())
                    {
                        identifier_ranges.push(range);
                        let resolves_to_unindexed_local = !kotlin_token_is_qualified(&token)
                            && scope_graph
                                .resolve_symbol(
                                    token.text(),
                                    usize::from(token.text_range().start()),
                                )
                                .is_some_and(|binding| {
                                    let definition_range = crate::TextRange::new(
                                        u32::from(binding.definition_range.start()),
                                        u32::from(binding.definition_range.end()),
                                    );
                                    !input
                                        .known_declaration_identifiers
                                        .iter()
                                        .any(|(_, known_range)| *known_range == definition_range)
                                });
                        if kotlin_token_is_declaration_name(&token) || resolves_to_unindexed_local {
                            declaration_ranges.push(range);
                        }
                    } else if token.kind().is_string_literal() {
                        let token_start = u32::from(token.text_range().start());
                        for &name in &target_names {
                            for offset in identifier_spelling_offsets(token.text(), name) {
                                let Ok(offset) = u32::try_from(offset) else {
                                    name_bearing_evidence = Some(name);
                                    continue;
                                };
                                let occurrence = crate::TextRange::new(
                                    token_start + offset,
                                    token_start + offset + name.len() as u32,
                                );
                                let extracted = usages.iter().any(|usage| {
                                    usage.location.file_id == input.revision.file_id()
                                        && usage.location.range == occurrence
                                        && usage.location.name.as_ref() == name
                                });
                                let resolves_to_unindexed_local = scope_graph
                                    .resolve_symbol(name, occurrence.start as usize)
                                    .is_some_and(|binding| {
                                        let definition_range = crate::TextRange::new(
                                            u32::from(binding.definition_range.start()),
                                            u32::from(binding.definition_range.end()),
                                        );
                                        !input.known_declaration_identifiers.iter().any(
                                            |(_, known_range)| *known_range == definition_range,
                                        )
                                    });
                                if extracted || resolves_to_unindexed_local {
                                    identifier_ranges.push(occurrence);
                                    if resolves_to_unindexed_local {
                                        declaration_ranges.push(occurrence);
                                    }
                                } else {
                                    name_bearing_evidence = Some(name);
                                }
                            }
                        }
                    } else if token.kind().is_comment()
                        && (token.text().contains("{@link") || token.text().contains("@see"))
                    {
                        name_bearing_evidence = target_names
                            .iter()
                            .find(|name| contains_identifier_spelling(token.text(), name))
                            .copied();
                    }
                }
                dynamic_lookup = kotlin_dynamic_symbol_lookup_in(
                    self,
                    &root,
                    &is_ignored,
                    &target_names,
                    usages,
                    is_cancelled,
                )?
                .map(|token| token.text().to_string());
            }
            Language::Groovy => {
                let mut parser = hyperion_groovy::tree_sitter_wrapper::GroovyParser::new()
                    .map_err(|error| {
                        CurrentUsageQueryError::Unavailable(format!(
                            "failed to parse {}: {error}",
                            input.file_path.display()
                        ))
                    })?;
                if is_cancelled() {
                    return Err(CurrentUsageQueryError::Cancelled);
                }
                let tree = parser.parse_tree(&indexed.content).map_err(|error| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "failed to parse {}: {error}",
                        input.file_path.display()
                    ))
                })?;
                if tree.root_node().has_error() {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "{} has a Groovy parse error",
                        input.file_path.display()
                    )));
                }
                collect_tree_sitter_audit_ranges(
                    self,
                    tree.root_node(),
                    &indexed.content,
                    &target_names,
                    &is_ignored,
                    &mut identifier_ranges,
                    &mut declaration_ranges,
                    &mut name_bearing_evidence,
                    &mut dynamic_lookup,
                    usages,
                    is_cancelled,
                )?;
            }
            Language::Scala => {
                let parsed = hyperion_scala::parser::parse(&indexed.content).map_err(|error| {
                    CurrentUsageQueryError::Unavailable(format!(
                        "failed to parse {}: {error:#}",
                        input.file_path.display()
                    ))
                })?;
                if is_cancelled() {
                    return Err(CurrentUsageQueryError::Cancelled);
                }
                if !parsed.errors.is_empty() {
                    return Err(CurrentUsageQueryError::Unavailable(format!(
                        "{} has {} Scala parse error(s)",
                        input.file_path.display(),
                        parsed.errors.len()
                    )));
                }
                collect_tree_sitter_audit_ranges(
                    self,
                    parsed.tree.root_node(),
                    &indexed.content,
                    &target_names,
                    &is_ignored,
                    &mut identifier_ranges,
                    &mut declaration_ranges,
                    &mut name_bearing_evidence,
                    &mut dynamic_lookup,
                    usages,
                    is_cancelled,
                )?;
            }
        }

        if let Some(lookup) = dynamic_lookup {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} contains dynamic symbol lookup through '{lookup}', whose argument cannot be proven constant",
                input.file_path.display()
            )));
        }
        if let Some(name) = name_bearing_evidence {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} contains a possible name-based refactoring use of '{}' in a string or documentation link",
                input.file_path.display(),
                name
            )));
        }
        self.audit_identifier_ranges(
            IdentifierRangeAudit {
                snapshot,
                input,
                indexed,
                usages,
                identifier_ranges: &identifier_ranges,
                declaration_ranges: &declaration_ranges,
            },
            is_cancelled,
        )
    }

    pub(super) fn parse_strict_java_refactoring_input(
        &self,
        input: &CurrentUsageScanInput,
        indexed: &crate::IndexedContent,
        is_cancelled: &impl Fn() -> bool,
    ) -> std::result::Result<hyperion_parser::SyntaxNode, CurrentUsageQueryError> {
        debug_assert_eq!(input.language, Language::Java);
        if indexed.content.contains("\\u") {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} contains a Java Unicode escape that the refactoring audit cannot bind",
                input.file_path.display()
            )));
        }
        let mut tokens = Vec::new();
        for (index, token) in hyperion_parser::Lexer::new(&indexed.content).enumerate() {
            if index % 256 == 0 && is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            tokens.push(token);
        }
        let (green, parse_errors) = hyperion_parser::Parser::parse(tokens);
        if is_cancelled() {
            return Err(CurrentUsageQueryError::Cancelled);
        }
        if !parse_errors.is_empty() {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "{} has {} parse error(s)",
                input.file_path.display(),
                parse_errors.len()
            )));
        }
        Ok(hyperion_parser::SyntaxNode::new_root(green))
    }
}
