// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Style-related inspections: unused variables/imports/parameters, dead code,
//! empty catch blocks, string comparison, redundant modifiers, missing override,
//! and magic numbers.

use crate::position_utils::text_range_to_lsp_range;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;
use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString};

use super::find_containing_scope;

/// Check for unused local variables
pub(crate) fn check_unused_variables(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Collect all local variable declarations with their scope and range
    // Structure: (var_name, var_range, scope_range) -> is_used
    let mut local_vars: Vec<(String, TextRange, TextRange)> = Vec::new();

    // First pass: collect all LOCAL_VAR_DECL with their containing scope
    for node in syntax.descendants() {
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
            // Find the containing scope for this declaration
            let scope = match find_containing_scope(&node) {
                Some(s) => s,
                None => continue, // Skip if we can't find a scope
            };
            let scope_range = scope.text_range();

            // Find the variable name - it's the IDENT that comes after the type
            // but BEFORE the '=' sign (not in the initializer expression)
            let mut found_type = false;
            for child in node.children_with_tokens() {
                match child.kind() {
                    // Type markers - after seeing one, next IDENT is the variable name
                    SyntaxKind::CLASS_TYPE
                    | SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::ARRAY_TYPE
                    | SyntaxKind::VAR_KW => {
                        found_type = true;
                    }
                    // Stop at '=' - anything after is the initializer
                    SyntaxKind::EQ => {
                        break;
                    }
                    SyntaxKind::IDENT
                        // Only capture IDENT if we've seen a type (this is the variable name)
                        if found_type => {
                            if let Some(token) = child.as_token() {
                                let name = token.text().to_string();
                                let range = token.text_range();
                                // Skip if starts with underscore (intentionally unused)
                                if !name.starts_with('_') {
                                    local_vars.push((name, range, scope_range));
                                }
                            }
                            break; // Only one variable name per declaration
                        }
                    _ => {}
                }
            }
        }
    }

    // Track which variables are used
    let mut used_vars: HashSet<usize> = HashSet::new();

    // Second pass: mark variables as used if they appear in their scope
    for node in syntax.descendants() {
        // Skip the declaration nodes themselves
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
            continue;
        }

        // Check for identifier references
        for child in node.children_with_tokens() {
            if child.kind() == SyntaxKind::IDENT {
                if let Some(token) = child.as_token() {
                    let name = token.text();
                    let ident_pos = token.text_range().start();

                    // Find matching variable declarations where this identifier
                    // is within the variable's scope AND after the declaration
                    for (idx, (var_name, var_range, scope_range)) in local_vars.iter().enumerate() {
                        if var_name == name
                            && scope_range.contains(ident_pos)
                            && ident_pos > var_range.end()
                        {
                            used_vars.insert(idx);
                        }
                    }
                }
            }
        }
    }

    // Generate diagnostics for unused variables
    for (idx, (name, range, _scope_range)) in local_vars.iter().enumerate() {
        if !used_vars.contains(&idx) {
            let lsp_range = text_range_to_lsp_range(*range, source_text);
            diagnostics.push(Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("unused_variable".to_string())),
                code_description: None,
                source: Some("hyperion-lint".to_string()),
                message: format!("Variable '{name}' is declared but never used"),
                related_information: None,
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                data: None,
            });
        }
    }

    diagnostics
}

/// Check for unused imports
///
/// Detects imports that are declared but never used in the code. This helps identify
/// dead code and unnecessary dependencies.
///
/// The function tracks both regular and static imports:
/// - Regular imports: `import java.util.List;` → tracks usage of `List`
/// - Static imports: `import static Arrays.asList;` → tracks usage of `asList()`
///
/// Usage is detected in multiple contexts:
/// - Type annotations (variable types, parameters, return types, fields)
/// - Generic type arguments (`List<User>`)
/// - `new` expressions (constructor calls)
/// - Cast expressions (`(User) obj`)
/// - `instanceof` checks
/// - Annotations (`@RequestBody`, `@Test`)
/// - `extends` and `implements` clauses
/// - `throws` clauses
/// - Static method/field access
/// - Bare identifier usage (for static imports)
pub(crate) fn check_unused_imports(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Collect all imports (both regular and static)
    // Tuple: (simple_name, text_range, is_static)
    let mut imports: Vec<(String, TextRange, bool)> = Vec::new();
    for node in syntax.descendants() {
        if node.kind() == SyntaxKind::IMPORT_DECL {
            // Extract the imported name (last part of the import path)
            let import_text = node.text().to_string();
            let range = node.text_range();
            let is_static = import_text.contains("static ");

            // Get the simple name (last segment before semicolon)
            if let Some(simple_name) = extract_import_simple_name(&import_text) {
                // Skip wildcard imports - they're too complex to analyze accurately
                if simple_name != "*" {
                    imports.push((simple_name, range, is_static));
                }
            }
        }
    }

    // Collect type names and identifiers used throughout the code
    // This set will contain all symbols that are referenced, which we'll use
    // to determine if an import is actually used
    let mut used_type_names: HashSet<String> = HashSet::new();

    for node in syntax.descendants() {
        // Skip import and package declarations (and their children)
        if node.ancestors().any(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::IMPORT_DECL | SyntaxKind::PACKAGE_DECL
            )
        }) {
            continue;
        }

        // Type positions where imports are actually used:
        // 1. CLASS_TYPE - type annotations (List, String, etc.)
        // 2. Extends/implements clauses
        // 3. Annotations
        // 4. new expressions (constructor calls)
        // 5. Throw/catch types

        match node.kind() {
            // Qualified names in expression positions (e.g., Api.V1)
            SyntaxKind::QUALIFIED_NAME => {
                for token in node.descendants_with_tokens() {
                    if token.kind() == SyntaxKind::IDENT {
                        if let Some(t) = token.as_token() {
                            used_type_names.insert(t.text().to_string());
                        }
                    }
                }
            }
            // Type annotations - the primary use of imports
            SyntaxKind::CLASS_TYPE => {
                for name in get_type_identifiers(&node) {
                    used_type_names.insert(name);
                }
            }

            // Extends keyword: class Foo extends Bar
            SyntaxKind::EXTENDS_KW => {
                // Look at sibling types after extends keyword
                if let Some(parent) = node.parent() {
                    for child in parent.descendants_with_tokens() {
                        if child.kind() == SyntaxKind::IDENT {
                            if let Some(token) = child.as_token() {
                                used_type_names.insert(token.text().to_string());
                            }
                        }
                    }
                }
            }

            // Implements keyword: class Foo implements Bar, Baz
            SyntaxKind::IMPLEMENTS_KW => {
                // Look at sibling types after implements keyword
                if let Some(parent) = node.parent() {
                    for child in parent.descendants_with_tokens() {
                        if child.kind() == SyntaxKind::IDENT {
                            if let Some(token) = child.as_token() {
                                used_type_names.insert(token.text().to_string());
                            }
                        }
                    }
                }
            }

            // Annotations: @Override, @Test, @RequestBody
            SyntaxKind::ANNOTATION => {
                // Annotation names can be direct IDENT or inside QUALIFIED_NAME
                for child in node.children() {
                    if child.kind() == SyntaxKind::QUALIFIED_NAME {
                        // Get the last IDENT in the qualified name (the simple name)
                        let mut last_ident: Option<String> = None;
                        for token in child.descendants_with_tokens() {
                            if token.kind() == SyntaxKind::IDENT {
                                if let Some(t) = token.as_token() {
                                    last_ident = Some(t.text().to_string());
                                }
                            }
                        }
                        if let Some(name) = last_ident {
                            used_type_names.insert(name);
                        }
                    }
                }
                // Also check for direct IDENT children (shouldn't happen, but be safe)
                for child in node.children_with_tokens() {
                    if child.kind() == SyntaxKind::IDENT {
                        if let Some(token) = child.as_token() {
                            used_type_names.insert(token.text().to_string());
                        }
                    }
                }
            }

            // Throw statements: throw new SomeException()
            SyntaxKind::THROW_STMT => {
                for child in node.descendants_with_tokens() {
                    if child.kind() == SyntaxKind::IDENT {
                        if let Some(token) = child.as_token() {
                            // Only add if it looks like a type (starts with uppercase)
                            let name = token.text();
                            if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                                used_type_names.insert(name.to_string());
                            }
                        }
                    }
                }
            }

            // Catch clauses: catch (SomeException e)
            SyntaxKind::CATCH_CLAUSE => {
                for child in node.descendants() {
                    if child.kind() == SyntaxKind::CLASS_TYPE {
                        for name in get_type_identifiers(&child) {
                            used_type_names.insert(name);
                        }
                    }
                }
            }

            // Static method/field access: ClassName.method() or ClassName.CONSTANT
            // Look for FIELD_ACCESS_EXPR or METHOD_CALL_EXPR patterns
            SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::METHOD_CALL_EXPR => {
                // Get the text and check if it starts with an uppercase identifier
                // (suggesting static access like List.of() or Arrays.asList())
                let text = node.text().to_string();
                if let Some(first_part) = text.split('.').next() {
                    let first_part = first_part.trim();
                    if first_part.chars().next().is_some_and(|c| c.is_uppercase())
                        && first_part.chars().all(|c| c.is_alphanumeric())
                    {
                        used_type_names.insert(first_part.to_string());
                    }
                }
            }

            _ => {}
        }
    }

    // Additional pass: collect identifiers that could be static imports
    //
    // Static imports allow bare usage without qualification:
    //   - `import static Arrays.asList;` → usage: `asList("a", "b")`  (not `Arrays.asList(...)`)
    //   - `import static Math.PI;` → usage: `2 * PI * radius`  (not `Math.PI`)
    //
    // We only check identifiers that match a known static import name to avoid false negatives.
    let static_import_names: HashSet<&str> = imports
        .iter()
        .filter(|(_, _, is_static)| *is_static)
        .map(|(name, _, _)| name.as_str())
        .collect();

    if !static_import_names.is_empty() {
        for node in syntax.descendants() {
            // Skip import and package declarations
            if node.kind() == SyntaxKind::IMPORT_DECL || node.kind() == SyntaxKind::PACKAGE_DECL {
                continue;
            }

            // Bare method call: helper() → METHOD_CALL_EXPR contains NAME_EXPR > IDENT
            if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
                for child in node.children() {
                    if child.kind() == SyntaxKind::NAME_EXPR {
                        for token in child.children_with_tokens() {
                            if token.kind() == SyntaxKind::IDENT {
                                if let Some(t) = token.as_token() {
                                    let name = t.text().to_string();
                                    if static_import_names.contains(name.as_str()) {
                                        used_type_names.insert(name);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Bare identifier: PI, MAX_VALUE, defaultValue → NAME_EXPR > IDENT
            if node.kind() == SyntaxKind::NAME_EXPR {
                // Skip if this NAME_EXPR is the callee of a METHOD_CALL_EXPR (handled above)
                if node
                    .parent()
                    .is_some_and(|p| p.kind() == SyntaxKind::METHOD_CALL_EXPR)
                {
                    continue;
                }
                for child in node.children_with_tokens() {
                    if child.kind() == SyntaxKind::IDENT {
                        if let Some(t) = child.as_token() {
                            let name = t.text().to_string();
                            if static_import_names.contains(name.as_str()) {
                                used_type_names.insert(name);
                            }
                        }
                    }
                }
            }
        }
    }

    // Check which imports are unused
    for (import_name, range, _is_static) in imports {
        if !used_type_names.contains(&import_name) {
            let lsp_range = text_range_to_lsp_range(range, source_text);
            diagnostics.push(Diagnostic {
                range: lsp_range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String("unused_import".to_string())),
                code_description: None,
                source: Some("hyperion-lint".to_string()),
                message: format!("Unused import '{import_name}'"),
                related_information: None,
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                data: None,
            });
        }
    }

    diagnostics
}

/// Get all identifiers from a CLASS_TYPE node (including qualified segments).
fn get_type_identifiers(type_node: &SyntaxNode) -> Vec<String> {
    let mut idents = Vec::new();

    for child in type_node.children_with_tokens() {
        if child.kind() == SyntaxKind::IDENT {
            if let Some(token) = child.as_token() {
                idents.push(token.text().to_string());
            }
        }
        if child.kind() == SyntaxKind::QUALIFIED_NAME {
            if let Some(n) = child.as_node() {
                for token in n.descendants_with_tokens() {
                    if token.kind() == SyntaxKind::IDENT {
                        if let Some(t) = token.as_token() {
                            idents.push(t.text().to_string());
                        }
                    }
                }
            }
        }
    }

    idents
}

/// Extract the simple name from an import statement
/// e.g., "import java.util.List;" -> "List"
pub(crate) fn extract_import_simple_name(import_text: &str) -> Option<String> {
    let trimmed = import_text.trim();
    // Remove "import " prefix and ";" suffix
    let path = trimmed.strip_prefix("import ")?;

    // Handle optional "static " modifier
    let path = path.strip_prefix("static ").unwrap_or(path);

    let path = path.strip_suffix(';')?.trim();

    // Get the last segment
    let simple_name = path.split('.').next_back()?;
    Some(simple_name.to_string())
}

/// Check for methods that override parent methods but lack @Override annotation
pub(crate) fn check_missing_override(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Find all method declarations
    for node in syntax.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            // Check if method has @Override annotation
            let has_override = has_override_annotation(&node);

            if !has_override {
                // Check if method name suggests it's an override
                let method_name = extract_method_name(&node);

                // Common override methods from Object
                let common_overrides = ["equals", "hashCode", "toString", "clone", "finalize"];

                if common_overrides.contains(&method_name.as_str()) {
                    let range = node.text_range();
                    // Get just the method signature line
                    let method_start = range.start();
                    let lsp_range = text_range_to_lsp_range(
                        TextRange::new(
                            method_start,
                            method_start + rowan::TextSize::from(method_name.len() as u32 + 20),
                        ),
                        source_text,
                    );

                    diagnostics.push(Diagnostic {
                        range: lsp_range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        code: Some(NumberOrString::String("missing_override".to_string())),
                        code_description: None,
                        source: Some("hyperion-lint".to_string()),
                        message: format!(
                            "Method '{method_name}' overrides Object method but lacks @Override annotation"
                        ),
                        related_information: None,
                        tags: None,
                        data: None,
                    });
                }
            }
        }
    }

    diagnostics
}

/// Check if a method has @Override annotation
fn has_override_annotation(method_node: &SyntaxNode) -> bool {
    // Look at siblings before the method for annotations
    // Annotations are typically inside MODIFIER_LIST nodes
    let mut sibling = method_node.prev_sibling();
    while let Some(sib) = sibling {
        // Check for MODIFIER_LIST which may contain annotations
        if sib.kind() == SyntaxKind::MODIFIER_LIST {
            // Search for ANNOTATION nodes inside the MODIFIER_LIST
            for child in sib.descendants() {
                if child.kind() == SyntaxKind::ANNOTATION {
                    let text = child.text().to_string();
                    if text.contains("Override") {
                        return true;
                    }
                }
            }
        }

        // Also check for direct ANNOTATION siblings (less common, but possible)
        if sib.kind() == SyntaxKind::ANNOTATION {
            let text = sib.text().to_string();
            if text.contains("Override") {
                return true;
            }
        }

        // Stop if we hit another declaration
        if matches!(
            sib.kind(),
            SyntaxKind::METHOD_DECL | SyntaxKind::FIELD_DECL | SyntaxKind::CLASS_DECL
        ) {
            break;
        }
        sibling = sib.prev_sibling();
    }

    // Also check children (some parsers put annotations as children)
    for child in method_node.children() {
        if child.kind() == SyntaxKind::ANNOTATION {
            let text = child.text().to_string();
            if text.contains("Override") {
                return true;
            }
        }
    }

    false
}

/// Extract method name from method declaration
fn extract_method_name(method_node: &SyntaxNode) -> String {
    for child in method_node.children_with_tokens() {
        if child.kind() == SyntaxKind::IDENT {
            if let Some(token) = child.as_token() {
                return token.text().to_string();
            }
        }
    }
    String::new()
}

/// Check for empty catch blocks
pub(crate) fn check_empty_catch_blocks(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in syntax.descendants() {
        if node.kind() == SyntaxKind::CATCH_CLAUSE {
            // Find the block statement in catch clause
            let mut has_statements = false;
            for child in node.descendants() {
                if child.kind() == SyntaxKind::BLOCK_STMT {
                    // Check if block has any statements
                    for block_child in child.children() {
                        if matches!(
                            block_child.kind(),
                            SyntaxKind::EXPR_STMT
                                | SyntaxKind::LOCAL_VAR_DECL
                                | SyntaxKind::RETURN_STMT
                                | SyntaxKind::THROW_STMT
                                | SyntaxKind::IF_STMT
                                | SyntaxKind::FOR_STMT
                                | SyntaxKind::WHILE_STMT
                                | SyntaxKind::TRY_STMT
                        ) {
                            has_statements = true;
                            break;
                        }
                    }
                    break;
                }
            }

            // IntelliJ's defaults: a catch block that contains a comment (the
            // author explained why it is empty) or whose parameter is named
            // `ignore`/`ignored` is intentional.
            let has_comment = node.descendants_with_tokens().any(|element| {
                matches!(
                    element.kind(),
                    SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
                )
            });
            let parameter_ignored = node
                .children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .filter(|token| token.kind().is_identifier())
                .last()
                .is_some_and(|token| matches!(token.text(), "ignore" | "ignored" | "_"));
            if !has_statements && !has_comment && !parameter_ignored {
                let range = node.text_range();
                let lsp_range = text_range_to_lsp_range(range, source_text);

                diagnostics.push(Diagnostic {
                    range: lsp_range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(NumberOrString::String("empty_catch_block".to_string())),
                    code_description: None,
                    source: Some("hyperion-lint".to_string()),
                    message: "Empty catch block - exceptions should be logged or rethrown"
                        .to_string(),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
        }
    }

    diagnostics
}

/// Check for string comparison using == instead of .equals()
///
/// Only a comparison one of whose *operands* is a string literal (or a
/// `toString()` call) is reported: the type of any other operand is not known
/// from syntax, and a string literal appearing elsewhere in a larger
/// expression (`n == 1 && "a".equals(s)`) says nothing about the comparison.
pub(crate) fn check_string_comparison(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in syntax.descendants() {
        if node.kind() != SyntaxKind::BINARY_EXPR {
            continue;
        }
        let Some(operator) = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| !token.kind().is_trivia())
        else {
            continue;
        };
        if !matches!(operator.kind(), SyntaxKind::EQ_EQ | SyntaxKind::BANG_EQ) {
            continue;
        }
        // Same-precedence chains parse as continuation nodes `[op, right]`
        // whose left operand is the preceding sibling.
        let mut operands = node.children();
        let (left, right) = match (operands.next(), operands.next()) {
            (Some(left), Some(right)) => (Some(left), Some(right)),
            (Some(right), None) => (node.prev_sibling(), Some(right)),
            _ => (None, None),
        };
        let (Some(left), Some(right)) = (left, right) else {
            continue;
        };
        if is_null_literal(&left) || is_null_literal(&right) {
            continue;
        }
        if !is_string_operand(&left) && !is_string_operand(&right) {
            continue;
        }
        diagnostics.push(Diagnostic {
            range: text_range_to_lsp_range(node.text_range(), source_text),
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("string_comparison".to_string())),
            code_description: None,
            source: Some("hyperion-lint".to_string()),
            message: "String comparison using == may not work as expected. Use .equals() instead."
                .to_string(),
            related_information: None,
            tags: None,
            data: None,
        });
    }

    diagnostics
}

fn is_null_literal(node: &SyntaxNode) -> bool {
    node.kind() == SyntaxKind::LITERAL_EXPR && node.text().to_string().trim() == "null"
}

/// A string literal, a parenthesized one, or an argument-less `toString()` call.
fn is_string_operand(node: &SyntaxNode) -> bool {
    match node.kind() {
        SyntaxKind::LITERAL_EXPR => node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .any(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL
                )
            }),
        SyntaxKind::PAREN_EXPR => node
            .children()
            .next()
            .is_some_and(|inner| is_string_operand(&inner)),
        SyntaxKind::METHOD_CALL_EXPR => {
            let text = node.text().to_string();
            let compact: String = text.split_whitespace().collect();
            compact.ends_with(".toString()")
        }
        _ => false,
    }
}

/// Check for redundant modifiers (e.g., public on interface methods)
pub(crate) fn check_redundant_modifiers(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Find interface declarations
    for node in syntax.descendants() {
        if node.kind() == SyntaxKind::INTERFACE_DECL {
            // Find method declarations within the interface
            for child in node.descendants() {
                if child.kind() == SyntaxKind::METHOD_DECL {
                    // Look for MODIFIER_LIST sibling which contains the modifiers
                    let mut sibling = child.prev_sibling();
                    while let Some(sib) = sibling {
                        if sib.kind() == SyntaxKind::MODIFIER_LIST {
                            let modifier_text = sib.text().to_string();

                            // Check for redundant 'public' modifier
                            if modifier_text.contains("public") {
                                // Find the PUBLIC_KW token within the MODIFIER_LIST
                                for descendant in sib.descendants_with_tokens() {
                                    if descendant.kind() == SyntaxKind::PUBLIC_KW {
                                        if let Some(token) = descendant.as_token() {
                                            let range = token.text_range();
                                            let lsp_range =
                                                text_range_to_lsp_range(range, source_text);

                                            diagnostics.push(Diagnostic {
                                                range: lsp_range,
                                                severity: Some(DiagnosticSeverity::HINT),
                                                code: Some(NumberOrString::String(
                                                    "redundant_modifier".to_string(),
                                                )),
                                                code_description: None,
                                                source: Some("hyperion-lint".to_string()),
                                                message: "Modifier 'public' is redundant for interface methods"
                                                    .to_string(),
                                                related_information: None,
                                                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                                                data: None,
                                            });
                                        }
                                    }
                                }
                            }

                            // Check for redundant 'abstract' modifier (skip if method has 'default')
                            if modifier_text.contains("abstract")
                                && !modifier_text.contains("default")
                            {
                                // Find the ABSTRACT_KW token within the MODIFIER_LIST
                                for descendant in sib.descendants_with_tokens() {
                                    if descendant.kind() == SyntaxKind::ABSTRACT_KW {
                                        if let Some(token) = descendant.as_token() {
                                            let range = token.text_range();
                                            let lsp_range =
                                                text_range_to_lsp_range(range, source_text);

                                            diagnostics.push(Diagnostic {
                                                range: lsp_range,
                                                severity: Some(DiagnosticSeverity::HINT),
                                                code: Some(NumberOrString::String(
                                                    "redundant_modifier".to_string(),
                                                )),
                                                code_description: None,
                                                source: Some("hyperion-lint".to_string()),
                                                message: "Modifier 'abstract' is redundant for interface methods"
                                                    .to_string(),
                                                related_information: None,
                                                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                                                data: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }

                        // Stop if we hit another declaration
                        if matches!(
                            sib.kind(),
                            SyntaxKind::METHOD_DECL
                                | SyntaxKind::FIELD_DECL
                                | SyntaxKind::CLASS_DECL
                        ) {
                            break;
                        }
                        sibling = sib.prev_sibling();
                    }
                }
            }
        }
    }

    diagnostics
}

/// Check for unused method parameters
pub(crate) fn check_unused_parameters(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in syntax.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            // Skip abstract methods (they have no body)
            if is_abstract_method(&node) {
                continue;
            }

            // A parameter is only provably unused when nothing else can
            // constrain the signature: an overridable or framework-invoked
            // method must keep it (IntelliJ applies the same rule).
            if !method_signature_is_local(&node) {
                continue;
            }

            // Collect parameter names
            let mut params: HashMap<String, TextRange> = HashMap::new();
            for child in node.children() {
                if child.kind() == SyntaxKind::PARAM_LIST {
                    for param in child.children() {
                        if param.kind() == SyntaxKind::PARAM {
                            // Find the parameter name (IDENT after the type)
                            let mut found_type = false;
                            for param_child in param.children_with_tokens() {
                                match param_child.kind() {
                                    SyntaxKind::CLASS_TYPE
                                    | SyntaxKind::PRIMITIVE_TYPE
                                    | SyntaxKind::ARRAY_TYPE => {
                                        found_type = true;
                                    }
                                    SyntaxKind::IDENT if found_type => {
                                        if let Some(token) = param_child.as_token() {
                                            let name = token.text().to_string();
                                            // Skip if starts with underscore, or when the
                                            // parameter itself is annotated (`@Valid`,
                                            // `@RequestParam`, ... injected by a framework)
                                            if !name.starts_with('_')
                                                && !hyperion_parser::modifiers::declaration_modifiers(&param)
                                                    .has_any_annotation()
                                            {
                                                params.insert(name, token.text_range());
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }

            // Find the method body and check for parameter usage
            for child in node.children() {
                if child.kind() == SyntaxKind::BLOCK_STMT {
                    // Mark parameters as used if they appear in the body
                    let mut used_params: HashSet<String> = HashSet::new();
                    for descendant in child.descendants_with_tokens() {
                        if let Some(token) = descendant.as_token() {
                            if token.kind() == SyntaxKind::IDENT {
                                let name = token.text().to_string();
                                if params.contains_key(&name) {
                                    used_params.insert(name);
                                }
                            }
                        }
                    }

                    // Report unused parameters
                    for (param_name, range) in &params {
                        if !used_params.contains(param_name) {
                            let lsp_range = text_range_to_lsp_range(*range, source_text);

                            diagnostics.push(Diagnostic {
                                range: lsp_range,
                                severity: Some(DiagnosticSeverity::WARNING),
                                code: Some(NumberOrString::String("unused_parameter".to_string())),
                                code_description: None,
                                source: Some("hyperion-lint".to_string()),
                                message: format!("Parameter '{param_name}' is never used"),
                                related_information: None,
                                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                                data: None,
                            });
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

/// True when the method's signature is decided by this declaration alone.
///
/// That is the case for methods nothing can override or call reflectively:
/// `private`, `static` and `final` methods and methods of `final` classes,
/// enums and records. `main`, annotated methods (`@Override`, `@Bean`,
/// `@EventListener`, ...), `native` methods and methods of anonymous classes
/// (which almost always implement a supertype's method) are never local.
fn method_signature_is_local(method: &SyntaxNode) -> bool {
    use hyperion_parser::modifiers::declaration_modifiers;

    let modifiers = declaration_modifiers(method);
    if modifiers.has_any_annotation() || modifiers.has(SyntaxKind::NATIVE_KW) {
        return false;
    }
    if modifiers.is_static() && extract_method_name(method) == "main" {
        return false;
    }
    if modifiers.is_private() || modifiers.is_static() || modifiers.is_final() {
        // `static` methods of an anonymous class do not exist; the rest is local.
        return method
            .ancestors()
            .skip(1)
            .find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    SyntaxKind::CLASS_DECL
                        | SyntaxKind::INTERFACE_DECL
                        | SyntaxKind::ENUM_DECL
                        | SyntaxKind::RECORD_DECL
                        | SyntaxKind::NEW_EXPR
                )
            })
            .is_none_or(|owner| owner.kind() != SyntaxKind::NEW_EXPR);
    }
    let Some(owner) = method.ancestors().skip(1).find(|ancestor| {
        matches!(
            ancestor.kind(),
            SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::NEW_EXPR
        )
    }) else {
        return false;
    };
    match owner.kind() {
        SyntaxKind::ENUM_DECL | SyntaxKind::RECORD_DECL => true,
        SyntaxKind::CLASS_DECL => declaration_modifiers(&owner).is_final(),
        _ => false,
    }
}

/// Check if a method is abstract (no body)
fn is_abstract_method(method_node: &SyntaxNode) -> bool {
    // Abstract methods have no block statement
    for child in method_node.children() {
        if child.kind() == SyntaxKind::BLOCK_STMT {
            return false;
        }
    }
    true
}

/// Check for magic numbers (numeric literals that should be constants)
pub(crate) fn check_magic_numbers(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Common acceptable "magic numbers" that don't need warnings
    let acceptable = [
        "0", "1", "-1", "2", "10", "100", "1000", "0.0", "1.0", "0.5", "0L", "1L",
    ];

    for node in syntax.descendants() {
        // Only check numeric literals in method bodies, not field initializers
        if node.kind() == SyntaxKind::METHOD_DECL {
            for descendant in node.descendants() {
                if descendant.kind() == SyntaxKind::LITERAL_EXPR {
                    for child in descendant.children_with_tokens() {
                        if let Some(token) = child.as_token() {
                            let kind = token.kind();
                            if matches!(
                                kind,
                                SyntaxKind::INT_LITERAL
                                    | SyntaxKind::LONG_LITERAL
                                    | SyntaxKind::FLOAT_LITERAL
                                    | SyntaxKind::DOUBLE_LITERAL
                            ) {
                                let text = token.text();

                                // Skip acceptable values
                                if acceptable.contains(&text) {
                                    continue;
                                }

                                // Skip small integers (0-10)
                                if let Ok(val) = text.parse::<i64>() {
                                    if (-1..=10).contains(&val) {
                                        continue;
                                    }
                                }

                                let range = token.text_range();
                                let lsp_range = text_range_to_lsp_range(range, source_text);

                                diagnostics.push(Diagnostic {
                                    range: lsp_range,
                                    severity: Some(DiagnosticSeverity::HINT),
                                    code: Some(NumberOrString::String("magic_number".to_string())),
                                    code_description: None,
                                    source: Some("hyperion-lint".to_string()),
                                    message: format!("Magic number '{text}'. Consider extracting to a named constant."),
                                    related_information: None,
                                    tags: None,
                                    data: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    diagnostics
}
