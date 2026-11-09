// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Collection taint tracking for mutation methods
//!
//! This module handles tracking taint propagation through collection
//! mutation methods like append(), add(), set(), and put().

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

use crate::finding::Location;
use crate::taint::TaintTracker;

/// Tracks taint propagation through StringBuilder/StringBuffer append()
///
/// When append() is called with tainted data, the receiver becomes tainted.
/// This does NOT create a finding - append() is not a sink.
pub(super) fn track_append_taint(
    receiver_name: &str,
    call_node: &SyntaxNode,
    parent_node: &SyntaxNode,
    taint_tracker: &mut TaintTracker<'_>,
    is_expr_kind_fn: &impl Fn(SyntaxKind) -> bool,
    node_location_fn: &impl Fn(&SyntaxNode) -> Location,
    get_code_snippet_fn: &impl Fn(&SyntaxNode) -> String,
) -> bool {
    let Some(arg_list) = call_node
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
    else {
        return false;
    };

    // Check if any argument is tainted
    for arg in arg_list.children() {
        if !is_expr_kind_fn(arg.kind()) {
            continue;
        }

        let Some(taint) = taint_tracker.expr_taint(&arg, node_location_fn, get_code_snippet_fn)
        else {
            continue;
        };

        // Mark the StringBuilder as tainted
        let location = node_location_fn(parent_node);
        let code = get_code_snippet_fn(parent_node);
        let propagated = taint.propagate(
            location,
            "StringBuilder.append() called with tainted argument".to_string(),
            code,
        );
        taint_tracker.mark_tainted(receiver_name.to_string(), propagated);
        return true; // Once tainted, no need to check other args
    }

    false
}

/// Tracks taint propagation through Collection.add() (List, Set)
///
/// Handles both signatures:
/// - add(element) - 1 argument
/// - add(index, element) - 2 arguments
pub(super) fn track_add_taint(
    receiver_name: &str,
    call_node: &SyntaxNode,
    parent_node: &SyntaxNode,
    taint_tracker: &mut TaintTracker<'_>,
    is_expr_kind_fn: &impl Fn(SyntaxKind) -> bool,
    node_location_fn: &impl Fn(&SyntaxNode) -> Location,
    get_code_snippet_fn: &impl Fn(&SyntaxNode) -> String,
) -> bool {
    let Some(arg_list) = call_node
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
    else {
        return false;
    };

    let args: Vec<_> = arg_list
        .children()
        .filter(|c| is_expr_kind_fn(c.kind()))
        .collect();

    // For add(element), check arg 0; for add(index, element), check arg 1
    let element_arg = if args.len() == 2 {
        args.get(1)
    } else {
        args.first()
    };

    let Some(arg) = element_arg else {
        return false;
    };

    let Some(taint) = taint_tracker.expr_taint(arg, node_location_fn, get_code_snippet_fn) else {
        return false;
    };

    let location = node_location_fn(parent_node);
    let code = get_code_snippet_fn(parent_node);
    let propagated = taint.propagate(
        location,
        "Collection.add() called with tainted element".to_string(),
        code,
    );

    taint_tracker.mark_collection_tainted(receiver_name, propagated.clone());
    taint_tracker.mark_tainted(receiver_name.to_string(), propagated);

    #[cfg(test)]
    eprintln!(
        "  -> Marked collection '{}' as tainted via add()",
        receiver_name
    );

    true
}

/// Tracks taint propagation through List.set(index, element)
pub(super) fn track_set_taint(
    receiver_name: &str,
    call_node: &SyntaxNode,
    parent_node: &SyntaxNode,
    taint_tracker: &mut TaintTracker<'_>,
    is_expr_kind_fn: &impl Fn(SyntaxKind) -> bool,
    node_location_fn: &impl Fn(&SyntaxNode) -> Location,
    get_code_snippet_fn: &impl Fn(&SyntaxNode) -> String,
) -> bool {
    let Some(arg_list) = call_node
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
    else {
        return false;
    };

    let args: Vec<_> = arg_list
        .children()
        .filter(|c| is_expr_kind_fn(c.kind()))
        .collect();

    // set(index, element) - check element at arg index 1
    let Some(arg) = args.get(1) else {
        return false;
    };

    let Some(taint) = taint_tracker.expr_taint(arg, node_location_fn, get_code_snippet_fn) else {
        return false;
    };

    let location = node_location_fn(parent_node);
    let code = get_code_snippet_fn(parent_node);
    let propagated = taint.propagate(
        location,
        "List.set() called with tainted element".to_string(),
        code,
    );

    taint_tracker.mark_collection_tainted(receiver_name, propagated.clone());
    taint_tracker.mark_tainted(receiver_name.to_string(), propagated);

    #[cfg(test)]
    eprintln!("  -> Marked list '{}' as tainted via set()", receiver_name);

    true
}

/// Tracks taint propagation through Map.put(key, value)
pub(super) fn track_put_taint(
    receiver_name: &str,
    call_node: &SyntaxNode,
    parent_node: &SyntaxNode,
    taint_tracker: &mut TaintTracker<'_>,
    is_expr_kind_fn: &impl Fn(SyntaxKind) -> bool,
    node_location_fn: &impl Fn(&SyntaxNode) -> Location,
    get_code_snippet_fn: &impl Fn(&SyntaxNode) -> String,
) -> bool {
    let Some(arg_list) = call_node
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
    else {
        return false;
    };

    let args: Vec<_> = arg_list
        .children()
        .filter(|c| is_expr_kind_fn(c.kind()))
        .collect();

    // put(key, value) - check value at arg index 1
    let Some(value_arg) = args.get(1) else {
        return false;
    };

    let Some(taint) = taint_tracker.expr_taint(value_arg, node_location_fn, get_code_snippet_fn)
    else {
        return false;
    };

    let location = node_location_fn(parent_node);
    let code = get_code_snippet_fn(parent_node);
    let propagated = taint.propagate(
        location,
        "Map.put() called with tainted value".to_string(),
        code,
    );

    taint_tracker.mark_collection_tainted(receiver_name, propagated.clone());
    taint_tracker.mark_tainted(receiver_name.to_string(), propagated);

    #[cfg(test)]
    eprintln!("  -> Marked map '{}' as tainted via put()", receiver_name);

    true
}
