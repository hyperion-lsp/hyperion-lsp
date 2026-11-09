// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method reference resolution
//!
//! Handles resolution of Java method references (e.g., `User::getName`) to determine
//! their type signatures. This is crucial for Stream API type inference.

use crate::symbol::Type;
use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::SyntaxNode;
use rowan::NodeOrToken;
use tracing::debug;

/// Information extracted from a method reference
#[derive(Debug, Clone)]
pub struct MethodReferenceInfo {
    /// The receiver type (e.g., User for User::getName)
    pub receiver_type: String,
    /// The method name (e.g., getName)
    pub method_name: String,
}

/// Extract method reference information from a METHOD_REF_EXPR node
///
/// Handles patterns like:
/// - `ClassName::methodName` - Static or instance method reference
/// - `object::methodName` - Instance method reference on expression
///
/// Returns (receiver_text, method_name)
pub fn extract_method_reference(node: &SyntaxNode) -> Option<MethodReferenceInfo> {
    if node.kind() != SyntaxKind::METHOD_REF_EXPR {
        return None;
    }

    let mut receiver_text = None;
    let mut method_name = None;

    // Walk children to find receiver and method name
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => {
                // The receiver could be a NAME_EXPR, qualified name, etc.
                if receiver_text.is_none() {
                    receiver_text = Some(n.text().to_string().trim().to_string());
                }
            }
            NodeOrToken::Token(t) => {
                match t.kind() {
                    SyntaxKind::IDENT => {
                        // The IDENT after :: is the method name
                        method_name = Some(t.text().to_string());
                    }
                    SyntaxKind::COLON_COLON => {
                        // Double colon separator
                        continue;
                    }
                    _ => {}
                }
            }
        }
    }

    if let (Some(receiver), Some(method)) = (receiver_text, method_name) {
        debug!("Extracted method reference: {}::{}", receiver, method);
        Some(MethodReferenceInfo {
            receiver_type: receiver,
            method_name: method,
        })
    } else {
        debug!("Failed to extract method reference from node");
        None
    }
}

/// Resolve a method reference to its return type
///
/// This function takes a method reference (like `User::getName`) and resolves it to
/// the actual return type of the method.
///
/// # Arguments
/// * `method_ref` - The extracted method reference info
/// * `resolve_type_fn` - Function to resolve the receiver type string to a Type
/// * `lookup_method_fn` - Function to lookup a method and get its return type
///
/// # Returns
/// The return type of the referenced method, or None if resolution fails
pub fn resolve_method_reference_return_type<F, G>(
    method_ref: &MethodReferenceInfo,
    resolve_type_fn: F,
    lookup_method_fn: G,
) -> Option<Type>
where
    F: Fn(&str) -> Option<Type>,
    G: Fn(&Type, &str) -> Option<Type>,
{
    // 1. Resolve receiver type
    let receiver_type = resolve_type_fn(&method_ref.receiver_type)?;
    debug!("Resolved receiver type: {:?}", receiver_type);

    // 2. Look up method on that type
    let return_type = lookup_method_fn(&receiver_type, &method_ref.method_name)?;
    debug!(
        "Method {}::{} returns {:?}",
        method_ref.receiver_type, method_ref.method_name, return_type
    );

    Some(return_type)
}

#[cfg(test)]
mod tests {

    // Tests would go here - for now, this is just the structure
}
