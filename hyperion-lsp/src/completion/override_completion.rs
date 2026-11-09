// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Override and Implement Method Completion
//!
//! Provides method completion suggestions for overriding parent class methods
//! and implementing interface methods.

use hyperion_workspace::index::WorkspaceIndex;
use std::collections::HashSet;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use tracing::debug;

use crate::inherited_members::{inherited_method_stubs, InheritedMethodStub};

/// Generate override/implement method completions for the type being edited.
///
/// The candidate set is `crate::inherited_members::inherited_method_stubs`,
/// the one canonical hierarchy walk. This function previously took
/// the directly written `parent_class` and `interfaces` and read each one's own
/// members, so a grandparent's method or a superinterface's contract was never
/// suggested; the elected effective method set is transitive through both.
pub fn generate_override_implement_completions(
    workspace_index: &WorkspaceIndex,
    class_fqn: &str,
    existing_methods: &HashSet<String>,
) -> Vec<CompletionItem> {
    debug!("Generating override/implement completions for class: {class_fqn}");
    let completions = inherited_method_stubs(workspace_index, class_fqn, existing_methods)
        .iter()
        .map(create_override_completion)
        .collect::<Vec<_>>();
    debug!(
        "Generated {} override/implement completions",
        completions.len()
    );
    completions
}

/// Create a completion item for an override/implement method
fn create_override_completion(method: &InheritedMethodStub) -> CompletionItem {
    let snippet = generate_method_snippet(method);
    let verb = if method.requires_implementation {
        "Implement"
    } else {
        "Override"
    };
    let owner = &method.declaring_owner;
    CompletionItem {
        label: format!("{verb} {} ({owner})", method.name),
        kind: Some(CompletionItemKind::METHOD),
        detail: Some(format!("{verb} {} - from {owner}", method.name)),
        insert_text: Some(snippet),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some("0_override".to_string()), // High priority
        filter_text: Some(method.name.clone()),    // Filter by method name
        ..Default::default()
    }
}

/// Generate method snippet with proper formatting and tabstops
fn generate_method_snippet(method: &InheritedMethodStub) -> String {
    let params = method
        .parameter_types
        .iter()
        .enumerate()
        .map(|(index, param_type)| {
            let param_name = infer_param_name(param_type, index);
            format!("${{{}:{param_type} {param_name}}}", index + 1)
        })
        .collect::<Vec<_>>()
        .join(", ");

    let body = if method.requires_implementation {
        // Nothing is inherited to delegate to.
        if method.return_type == "void" {
            "throw new UnsupportedOperationException(\"Not implemented\");".to_string()
        } else {
            format!(
                "throw new UnsupportedOperationException(\"Not implemented\"); // TODO: return {}",
                method.return_type
            )
        }
    } else {
        let arguments = method
            .parameter_types
            .iter()
            .enumerate()
            .map(|(index, param_type)| infer_param_name(param_type, index))
            .collect::<Vec<_>>()
            .join(", ");
        let delegation = method.super_call(&arguments);
        if method.return_type == "void" {
            format!("{delegation};")
        } else {
            format!("return {delegation};")
        }
    };

    format!(
        "@Override\npublic {} {}({params}) {{\n\t{body}\n\t$0\n}}",
        method.return_type, method.name
    )
}

/// Infer parameter name from type
fn infer_param_name(param_type: &str, index: usize) -> String {
    // Remove generics and package names
    let simple_type = param_type
        .split('<')
        .next()
        .unwrap_or(param_type)
        .split('.')
        .next_back()
        .unwrap_or(param_type);

    // Common type abbreviations
    let base_name = match simple_type {
        "String" => "str",
        "Integer" | "int" => "i",
        "Long" | "long" => "l",
        "Double" | "double" => "d",
        "Boolean" | "boolean" => "b",
        "List" => "list",
        "Set" => "set",
        "Map" => "map",
        "Object" => "obj",
        _ => {
            // Use first character lowercase
            &simple_type[0..1].to_lowercase()
        }
    };

    // Add index if needed to avoid duplicates
    if index == 0 {
        base_name.to_string()
    } else {
        format!("{base_name}{index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub(
        name: &str,
        return_type: &str,
        parameter_types: &[&str],
        declaring_owner: &str,
        requires_implementation: bool,
        declaring_owner_is_interface: bool,
    ) -> InheritedMethodStub {
        InheritedMethodStub {
            name: name.to_string(),
            declaring_owner: declaring_owner.to_string(),
            requires_implementation,
            declaring_owner_is_interface,
            return_type: return_type.to_string(),
            parameter_types: parameter_types.iter().map(|t| (*t).to_string()).collect(),
        }
    }

    #[test]
    fn test_infer_param_name() {
        assert_eq!(infer_param_name("String", 0), "str");
        assert_eq!(infer_param_name("Integer", 0), "i");
        assert_eq!(infer_param_name("User", 0), "u");
        assert_eq!(infer_param_name("java.util.List", 0), "list");
        assert_eq!(infer_param_name("List<String>", 0), "list");
    }

    #[test]
    fn test_infer_param_name_with_index() {
        assert_eq!(infer_param_name("String", 1), "str1");
        assert_eq!(infer_param_name("Integer", 2), "i2");
    }

    #[test]
    fn test_generate_method_snippet_no_params() {
        let snippet = generate_method_snippet(&stub(
            "toString",
            "String",
            &[],
            "java.lang.Object",
            false,
            false,
        ));
        assert!(snippet.contains("@Override"));
        assert!(snippet.contains("public String toString()"));
        assert!(snippet.contains("return super.toString();"));
    }

    #[test]
    fn test_generate_method_snippet_with_params() {
        let snippet = generate_method_snippet(&stub(
            "equals",
            "boolean",
            &["Object"],
            "java.lang.Object",
            false,
            false,
        ));
        assert!(snippet.contains("@Override"));
        assert!(snippet.contains("public boolean equals"));
        assert!(snippet.contains("Object obj"));
        assert!(snippet.contains("return super.equals(obj);"));
    }

    #[test]
    fn test_generate_method_snippet_interface() {
        let snippet =
            generate_method_snippet(&stub("run", "void", &[], "java.lang.Runnable", true, true));
        assert!(snippet.contains("@Override"));
        assert!(snippet.contains("public void run()"));
        assert!(snippet.contains("throw new UnsupportedOperationException"));
        assert!(!snippet.contains("super"));
    }

    /// JLS 15.12.1: plain `super` denotes the direct superclass, so it cannot
    /// reach an inherited interface `default` method.
    #[test]
    fn an_inherited_default_method_delegates_through_its_interface() {
        let snippet = generate_method_snippet(&stub(
            "greet",
            "void",
            &[],
            "com.example.Greeter",
            false,
            true,
        ));
        assert!(
            snippet.contains("Greeter.super.greet();"),
            "expected a qualified super delegation, got: {snippet}"
        );
    }
}
