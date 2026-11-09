// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Postfix Completion Templates
//!
//! Provides IntelliJ-style postfix completions that transform expressions:
//! - `.for` → `for (Type item : items) { }`
//! - `.fori` → `for (int i = 0; i < items.length; i++) { }`
//! - `.if` → `if (condition) { }`
//! - `.else` → `if (!condition) { }`
//! - `.null` → `if (expr == null) { }`
//! - `.nn` / `.notnull` → `if (expr != null) { }`
//! - `.var` → `Type var = expr;`
//! - `.return` → `return expr;`
//! - `.throw` → `throw new Exception(expr);`
//! - `.not` → `!expr`
//! - `.par` → `(expr)`
//! - `.cast` → `(Type) expr`

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use tracing::debug;

/// Postfix template definition
#[derive(Debug, Clone)]
pub struct PostfixTemplate {
    /// Template key (e.g., "for", "if", "null")
    pub key: &'static str,
    /// Description shown in completion
    pub description: &'static str,
    /// Label shown in completion list
    pub label: &'static str,
    /// Function to generate the expansion
    pub expander: fn(&str, Option<&str>) -> String,
    /// Whether this template requires a non-null expression
    pub requires_expression: bool,
}

/// Get all available postfix templates
pub fn get_postfix_templates() -> Vec<PostfixTemplate> {
    vec![
        PostfixTemplate {
            key: "for",
            label: "for - iterate over collection",
            description: "Iterate with enhanced for loop",
            expander: expand_for,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "fori",
            label: "fori - iterate with index",
            description: "Iterate with index-based for loop",
            expander: expand_fori,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "if",
            label: "if - check condition",
            description: "Create if statement",
            expander: expand_if,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "else",
            label: "else - negate condition",
            description: "Create if statement with negated condition",
            expander: expand_else,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "null",
            label: "null - check if null",
            description: "Check if expression is null",
            expander: expand_null,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "nn",
            label: "nn - check if not null",
            description: "Check if expression is not null",
            expander: expand_notnull,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "notnull",
            label: "notnull - check if not null",
            description: "Check if expression is not null",
            expander: expand_notnull,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "var",
            label: "var - introduce variable",
            description: "Assign to a new variable",
            expander: expand_var,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "return",
            label: "return - return expression",
            description: "Return the expression",
            expander: expand_return,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "throw",
            label: "throw - throw exception",
            description: "Throw expression as exception",
            expander: expand_throw,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "not",
            label: "not - negate expression",
            description: "Negate boolean expression",
            expander: expand_not,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "par",
            label: "par - wrap in parentheses",
            description: "Wrap expression in parentheses",
            expander: expand_par,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "switch",
            label: "switch - switch on expression",
            description: "Create switch statement",
            expander: expand_switch,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "sout",
            label: "sout - print to console",
            description: "Print expression with System.out.println",
            expander: expand_sout,
            requires_expression: true,
        },
        // New templates
        PostfixTemplate {
            key: "cast",
            label: "cast - cast to type",
            description: "Cast expression to a type",
            expander: expand_cast,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "inst",
            label: "inst - instanceof check",
            description: "Check instanceof with pattern variable (Java 16+)",
            expander: expand_instanceof,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "stream",
            label: "stream - convert to stream",
            description: "Convert collection/array to stream",
            expander: expand_stream,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "opt",
            label: "opt - wrap in Optional",
            description: "Wrap expression in Optional.ofNullable",
            expander: expand_optional,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "optional",
            label: "optional - wrap in Optional",
            description: "Wrap expression in Optional.ofNullable",
            expander: expand_optional,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "async",
            label: "async - run asynchronously",
            description: "Wrap in CompletableFuture.supplyAsync",
            expander: expand_async,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "try",
            label: "try - wrap in try-catch",
            description: "Wrap expression in try-catch block",
            expander: expand_try,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "while",
            label: "while - while loop",
            description: "Create while loop with expression as condition",
            expander: expand_while,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "assert",
            label: "assert - add assertion",
            description: "Add assertion for expression",
            expander: expand_assert,
            requires_expression: true,
        },
        PostfixTemplate {
            key: "sync",
            label: "sync - synchronized block",
            description: "Wrap in synchronized block",
            expander: expand_synchronized,
            requires_expression: true,
        },
    ]
}

/// Extract the last expression from text, handling parentheses, brackets, and operators
fn extract_last_expression(text: &str) -> &str {
    let bytes = text.as_bytes();
    let len = bytes.len();

    if len == 0 {
        return text;
    }

    // Start from the end and work backwards
    let mut i = len;
    let mut paren_depth = 0;
    let mut bracket_depth = 0;
    let mut angle_depth = 0;

    // First, trim any trailing whitespace
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }

    if i == 0 {
        return "";
    }

    let trimmed_end = i;

    // Scan backwards to find the start of the expression
    while i > 0 {
        i -= 1;
        let ch = bytes[i] as char;

        match ch {
            ')' => paren_depth += 1,
            '(' => {
                if paren_depth == 0 {
                    // Unmatched opening paren - start of expression
                    return &text[i + 1..trimmed_end];
                }
                paren_depth -= 1;
            }
            ']' => bracket_depth += 1,
            '[' => {
                if bracket_depth == 0 {
                    // Unmatched opening bracket - start of expression
                    return &text[i + 1..trimmed_end];
                }
                bracket_depth -= 1;
            }
            '>' => angle_depth += 1,
            '<' if angle_depth > 0 => {
                angle_depth -= 1;
            }
            '.' if paren_depth == 0 && bracket_depth == 0 && angle_depth == 0 => {
                // Only treat as separator if we're at depth 0
                return &text[i + 1..trimmed_end];
            }
            c if (c.is_whitespace() || "{}[];,=+-*/<>!&|^%".contains(c))
                // Only treat as separator if we're at depth 0
                && paren_depth == 0 && bracket_depth == 0 && angle_depth == 0 =>
            {
                return &text[i + 1..trimmed_end];
            }
            _ => {
                // Regular character, continue
            }
        }
    }

    // Reached the start - entire text is the expression
    &text[..trimmed_end]
}

/// Check if we're in a postfix completion context
/// Returns (expression_text, template_key) if matched
pub fn match_postfix_pattern(text_before_cursor: &str) -> Option<(String, String)> {
    // Pattern: <expression>.<template_key>
    // Example: "items.for" or "user.null"

    let trimmed = text_before_cursor.trim_end();

    // Find the last dot
    let dot_pos = trimmed.rfind('.')?;

    // Get the template key after the dot
    let template_key = &trimmed[dot_pos + 1..];

    // Template key should be non-empty, alphabetic, and start with lowercase
    // (to distinguish from qualified class names like com.example.Class)
    if template_key.is_empty()
        || !template_key.chars().all(char::is_alphabetic)
        || !template_key.chars().next().is_some_and(char::is_lowercase)
    {
        return None;
    }

    // Get the expression before the dot
    let expression = &trimmed[..dot_pos];

    // Expression should not be empty and should not end with a dot
    if expression.is_empty() || expression.ends_with('.') {
        return None;
    }

    // Extract just the last expression, handling various patterns
    let last_expr = extract_last_expression(expression);

    if last_expr.is_empty() {
        return None;
    }

    debug!(
        "Matched postfix pattern: expr='{}', key='{}'",
        last_expr, template_key
    );
    Some((last_expr.to_string(), template_key.to_string()))
}

/// Generate completion items for postfix templates
pub fn get_postfix_completions(
    expression: &str,
    template_prefix: &str,
    inferred_type: Option<&str>,
) -> Vec<CompletionItem> {
    let templates = get_postfix_templates();
    let mut items = Vec::new();

    for template in templates {
        // Filter by prefix (case-insensitive for better UX)
        if !template
            .key
            .to_lowercase()
            .starts_with(&template_prefix.to_lowercase())
        {
            continue;
        }

        // Generate expansion
        let expansion = (template.expander)(expression, inferred_type);

        items.push(CompletionItem {
            label: template.label.to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(template.description.to_string()),
            insert_text: Some(expansion.clone()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            // We'll set the text_edit in the provider to handle ranges properly
            documentation: Some(tower_lsp::lsp_types::Documentation::String(format!(
                "Expands to:\n```java\n{}\n```",
                expansion.replace("$0", "")
            ))),
            ..Default::default()
        });
    }

    items
}

// Template expanders

fn expand_for(expr: &str, type_hint: Option<&str>) -> String {
    let item_type = type_hint.unwrap_or("Object");
    // Extract element type from collection type if possible
    let element_type = if item_type.contains('<') {
        // Extract from List<String> → String
        item_type
            .split('<')
            .nth(1)
            .and_then(|s| s.split('>').next())
            .unwrap_or("Object")
    } else {
        // Array type: String[] → String, or fallback to Object
        item_type.strip_suffix("[]").unwrap_or("Object")
    };

    format!("for ({element_type} ${{1:item}} : {expr}) {{\n    $0\n}}")
}

fn expand_fori(expr: &str, _type_hint: Option<&str>) -> String {
    format!("for (int ${{1:i}} = 0; ${{1:i}} < {expr}.length; ${{1:i}}++) {{\n    $0\n}}")
}

fn expand_if(expr: &str, _type_hint: Option<&str>) -> String {
    format!("if ({expr}) {{\n    $0\n}}")
}

fn expand_else(expr: &str, _type_hint: Option<&str>) -> String {
    format!("if (!({expr})) {{\n    $0\n}}")
}

fn expand_null(expr: &str, _type_hint: Option<&str>) -> String {
    format!("if ({expr} == null) {{\n    $0\n}}")
}

fn expand_notnull(expr: &str, _type_hint: Option<&str>) -> String {
    format!("if ({expr} != null) {{\n    $0\n}}")
}

fn expand_var(expr: &str, type_hint: Option<&str>) -> String {
    let var_type = type_hint.unwrap_or("var");
    format!("{var_type} ${{1:name}} = {expr};$0")
}

fn expand_return(expr: &str, _type_hint: Option<&str>) -> String {
    format!("return {expr};$0")
}

fn expand_throw(expr: &str, _type_hint: Option<&str>) -> String {
    // If expression is already an exception, just throw it
    // Otherwise, wrap in RuntimeException
    if expr.contains("Exception") || expr.contains("Error") {
        format!("throw {expr};$0")
    } else {
        format!("throw new RuntimeException({expr});$0")
    }
}

fn expand_not(expr: &str, _type_hint: Option<&str>) -> String {
    format!("!({expr}$0)")
}

fn expand_par(expr: &str, _type_hint: Option<&str>) -> String {
    format!("({expr}$0)")
}

fn expand_switch(expr: &str, _type_hint: Option<&str>) -> String {
    format!(
        "switch ({expr}) {{\n    case ${{1:value}}:\n        $0\n        break;\n    default:\n        break;\n}}"
    )
}

fn expand_sout(expr: &str, _type_hint: Option<&str>) -> String {
    format!("System.out.println({expr});$0")
}

fn expand_cast(expr: &str, type_hint: Option<&str>) -> String {
    let target_type = type_hint.unwrap_or("${1:Type}");
    format!("(({target_type}) {expr})$0")
}

fn expand_instanceof(expr: &str, _type_hint: Option<&str>) -> String {
    // Java 16+ pattern matching with instanceof
    format!("if ({expr} instanceof ${{1:Type}} ${{2:name}}) {{\n    $0\n}}")
}

fn expand_stream(expr: &str, type_hint: Option<&str>) -> String {
    // Check if it's an array or collection
    let is_array = type_hint.is_some_and(|t| t.ends_with("[]"));
    if is_array {
        format!("Arrays.stream({expr})$0")
    } else {
        format!("{expr}.stream()$0")
    }
}

fn expand_optional(expr: &str, _type_hint: Option<&str>) -> String {
    format!("Optional.ofNullable({expr})$0")
}

fn expand_async(expr: &str, _type_hint: Option<&str>) -> String {
    format!("CompletableFuture.supplyAsync(() -> {expr})$0")
}

fn expand_try(expr: &str, _type_hint: Option<&str>) -> String {
    format!("try {{\n    {expr};\n}} catch (${{1:Exception}} ${{2:e}}) {{\n    $0\n}}")
}

fn expand_while(expr: &str, _type_hint: Option<&str>) -> String {
    format!("while ({expr}) {{\n    $0\n}}")
}

fn expand_assert(expr: &str, _type_hint: Option<&str>) -> String {
    format!("assert {expr} : \"${{1:assertion message}}\";$0")
}

fn expand_synchronized(expr: &str, _type_hint: Option<&str>) -> String {
    format!("synchronized ({expr}) {{\n    $0\n}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_postfix_pattern() {
        assert_eq!(
            match_postfix_pattern("items.for"),
            Some(("items".to_string(), "for".to_string()))
        );

        assert_eq!(
            match_postfix_pattern("user.name.null"),
            Some(("name".to_string(), "null".to_string()))
        );

        assert_eq!(
            match_postfix_pattern("getValue().nn"),
            Some(("getValue()".to_string(), "nn".to_string()))
        );

        // Should not match - ends with dot
        assert_eq!(match_postfix_pattern("items."), None);

        // Should not match - no dot
        assert_eq!(match_postfix_pattern("items"), None);

        // Should not match - template key has numbers
        assert_eq!(match_postfix_pattern("items.for2"), None);
    }

    #[test]
    fn test_expand_for() {
        let result = expand_for("items", Some("List<String>"));
        assert!(result.contains("for (String"));
        assert!(result.contains(": items)"));
    }

    #[test]
    fn test_expand_if() {
        let result = expand_if("condition", None);
        assert_eq!(result, "if (condition) {\n    $0\n}");
    }

    #[test]
    fn test_expand_null() {
        let result = expand_null("user", None);
        assert_eq!(result, "if (user == null) {\n    $0\n}");
    }

    #[test]
    fn test_expand_var() {
        let result = expand_var("getValue()", Some("String"));
        assert!(result.contains("String"));
        assert!(result.contains("= getValue()"));
    }

    #[test]
    fn test_expand_cast() {
        let result = expand_cast("obj", Some("String"));
        assert_eq!(result, "((String) obj)$0");
    }

    #[test]
    fn test_expand_instanceof() {
        let result = expand_instanceof("obj", None);
        assert!(result.contains("if (obj instanceof"));
        assert!(result.contains("${1:Type}"));
    }

    #[test]
    fn test_expand_stream_collection() {
        let result = expand_stream("items", Some("List<String>"));
        assert_eq!(result, "items.stream()$0");
    }

    #[test]
    fn test_expand_stream_array() {
        let result = expand_stream("arr", Some("int[]"));
        assert_eq!(result, "Arrays.stream(arr)$0");
    }

    #[test]
    fn test_expand_optional() {
        let result = expand_optional("value", None);
        assert_eq!(result, "Optional.ofNullable(value)$0");
    }

    #[test]
    fn test_expand_async() {
        let result = expand_async("compute()", None);
        assert_eq!(result, "CompletableFuture.supplyAsync(() -> compute())$0");
    }

    #[test]
    fn test_expand_try() {
        let result = expand_try("riskyCall()", None);
        assert!(result.contains("try {"));
        assert!(result.contains("riskyCall();"));
        assert!(result.contains("catch"));
    }

    #[test]
    fn test_expand_synchronized() {
        let result = expand_synchronized("lock", None);
        assert!(result.contains("synchronized (lock)"));
    }

    #[test]
    fn test_postfix_template_count() {
        let templates = get_postfix_templates();
        // We should now have 24 templates
        assert!(
            templates.len() >= 20,
            "Should have at least 20 postfix templates, got {}",
            templates.len()
        );
    }
}
