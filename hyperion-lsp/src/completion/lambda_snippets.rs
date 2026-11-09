// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Lambda Expression Snippet Generation
//!
//! Generates lambda expression completions based on functional interface signatures.

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

/// Generate lambda expression completions for a functional interface
///
/// Creates multiple completion variants:
/// - Single parameter: `item -> `
/// - Single parameter with body: `item -> {}`
/// - Multiple parameters: `(a, b) -> `
/// - Multiple parameters with body: `(a, b) -> {}`
pub fn generate_lambda_completions(
    interface_name: &str,
    param_types: &[String],
    return_type: &str,
) -> Vec<CompletionItem> {
    let mut completions = Vec::new();
    let param_count = param_types.len();

    // Generate parameter names from types
    let param_names = generate_param_names(param_types);

    if param_count == 0 {
        // Supplier-style: () -> value
        completions.push(create_lambda_completion(
            "() -> ",
            "() -> $0",
            "Lambda expression (no parameters)",
            interface_name,
        ));

        completions.push(create_lambda_completion(
            "() -> {}",
            "() -> {\n\t$0\n}",
            "Lambda block (no parameters)",
            interface_name,
        ));
    } else if param_count == 1 {
        let param_name = &param_names[0];

        // Single parameter variants
        completions.push(create_lambda_completion(
            &format!("{param_name} -> "),
            &format!("{param_name} -> $0"),
            "Lambda expression (single parameter)",
            interface_name,
        ));

        completions.push(create_lambda_completion(
            &format!("{param_name} -> {{}}"),
            &format!("{param_name} -> {{\n\t$0\n}}"),
            "Lambda block (single parameter)",
            interface_name,
        ));

        // With parentheses (explicit parameter)
        completions.push(create_lambda_completion(
            &format!("({param_name}) -> "),
            &format!("({param_name}) -> $0"),
            "Lambda expression (explicit parameter)",
            interface_name,
        ));
    } else {
        // Multiple parameters
        let params_str = param_names.join(", ");

        completions.push(create_lambda_completion(
            &format!("({params_str}) -> "),
            &format!("({params_str}) -> $0"),
            "Lambda expression (multiple parameters)",
            interface_name,
        ));

        completions.push(create_lambda_completion(
            &format!("({params_str}) -> {{}}"),
            &format!("({params_str}) -> {{\n\t$0\n}}"),
            "Lambda block (multiple parameters)",
            interface_name,
        ));
    }

    // Add specific variants for common cases
    if return_type == "void" && param_count == 1 {
        // Consumer pattern - suggest common method calls
        let param_name = &param_names[0];
        completions.push(create_lambda_completion(
            &format!("{param_name}::method"),
            &format!("${{1:{param_name}}}::${{2:method}}"),
            "Method reference",
            interface_name,
        ));
    }

    completions
}

/// Create a lambda completion item with snippet support
fn create_lambda_completion(
    label: &str,
    insert_text: &str,
    detail: &str,
    interface_name: &str,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(format!("{detail} - {interface_name}")),
        insert_text: Some(insert_text.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some("0_lambda".to_string()), // High priority
        ..Default::default()
    }
}

/// Generate parameter names from parameter types
///
/// Examples:
/// - "String" -> "s"
/// - "User" -> "user"
/// - "List" -> "list"
/// - Multiple of same type: "s1", "s2", etc.
fn generate_param_names(param_types: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    let mut name_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for param_type in param_types {
        let base_name = infer_param_name(param_type);

        // Handle duplicate names
        let count = name_counts.entry(base_name.clone()).or_insert(0);
        *count += 1;

        let final_name = if *count == 1 && !names.contains(&base_name) {
            base_name
        } else {
            format!("{base_name}{count}")
        };

        names.push(final_name);
    }

    names
}

/// Infer a parameter name from a type
///
/// Uses common conventions:
/// - Primitives: type name (int, long, etc.)
/// - Single letter generics: lowercase (T -> t)
/// - Simple types: camelCase (String -> s, User -> user)
/// - Generic types: base type (List<String> -> list)
fn infer_param_name(param_type: &str) -> String {
    // Handle primitives
    if matches!(
        param_type,
        "int" | "long" | "double" | "float" | "boolean" | "byte" | "short" | "char"
    ) {
        return param_type.to_string();
    }

    // Handle generic type parameters (T, U, R, etc.)
    if param_type.len() == 1 && param_type.chars().next().is_some_and(char::is_uppercase) {
        return param_type.to_lowercase();
    }

    // Extract simple type name (remove package and generics)
    let simple_type = param_type
        .split('<')
        .next()
        .unwrap_or(param_type)
        .split('.')
        .next_back()
        .unwrap_or(param_type);

    // Convert to camelCase
    if simple_type.is_empty() {
        return "arg".to_string();
    }

    // Common abbreviations
    let abbrev = match simple_type {
        "String" => "str",
        "StringBuilder" => "sb",
        "Integer" => "i",
        "Long" => "l",
        "Double" => "d",
        "Boolean" => "b",
        "List" => "list",
        "Set" => "set",
        "Map" => "map",
        "Collection" => "collection",
        "Optional" => "opt",
        "Stream" => "stream",
        _ => {
            // Use first character lowercase
            let Some(first_char) = simple_type.chars().next() else {
                return "arg".to_string();
            };
            return first_char.to_lowercase().to_string();
        }
    };

    abbrev.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_param_name() {
        assert_eq!(infer_param_name("String"), "str");
        assert_eq!(infer_param_name("User"), "u");
        assert_eq!(infer_param_name("int"), "int");
        assert_eq!(infer_param_name("List"), "list");
        assert_eq!(infer_param_name("T"), "t");
        assert_eq!(infer_param_name("java.lang.String"), "str");
        assert_eq!(infer_param_name("List<String>"), "list");
    }

    #[test]
    fn test_generate_param_names_single() {
        let types = vec!["String".to_string()];
        let names = generate_param_names(&types);
        assert_eq!(names, vec!["str"]);
    }

    #[test]
    fn test_generate_param_names_multiple() {
        let types = vec!["String".to_string(), "Integer".to_string()];
        let names = generate_param_names(&types);
        assert_eq!(names, vec!["str", "i"]);
    }

    #[test]
    fn test_generate_param_names_duplicates() {
        let types = vec!["String".to_string(), "String".to_string()];
        let names = generate_param_names(&types);
        assert_eq!(names, vec!["str", "str2"]);
    }

    #[test]
    fn test_generate_lambda_single_param() {
        let completions = generate_lambda_completions("Consumer", &["String".to_string()], "void");

        assert!(completions.len() >= 3);
        assert!(completions.iter().any(|c| c.label == "str -> "));
        assert!(completions.iter().any(|c| c.label == "str -> {}"));
        assert!(completions
            .iter()
            .all(|c| c.kind == Some(CompletionItemKind::SNIPPET)));
    }

    #[test]
    fn test_generate_lambda_multiple_params() {
        let completions = generate_lambda_completions(
            "BiConsumer",
            &["String".to_string(), "Integer".to_string()],
            "void",
        );

        assert!(completions.len() >= 2);
        assert!(completions.iter().any(|c| c.label == "(str, i) -> "));
        assert!(completions.iter().any(|c| c.label == "(str, i) -> {}"));
    }

    #[test]
    fn test_generate_lambda_no_params() {
        let completions = generate_lambda_completions("Supplier", &[], "String");

        assert!(completions.len() >= 2);
        assert!(completions.iter().any(|c| c.label == "() -> "));
        assert!(completions.iter().any(|c| c.label == "() -> {}"));
    }
}
