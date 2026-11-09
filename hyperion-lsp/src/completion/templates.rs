// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Live Templates for Java Completion
//!
//! Provides IntelliJ-style live templates that expand into code snippets.
//! Templates use LSP snippet syntax for placeholders:
//! - `${1:placeholder}` - tabstop with default text
//! - `${2:text}` - second tabstop
//! - `$0` - final cursor position

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Position};
use tracing::debug;

/// Context where a template is valid
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateContext {
    /// Inside method body, at statement level
    Statement,
    /// Inside class body (not in method)
    ClassBody,
    /// Inside expression (after =, in method args, etc.)
    Expression,
    /// Type position (in declarations, extends, etc.)
    TypePosition,
    /// Valid anywhere
    Any,
}

/// A live template definition
#[derive(Debug, Clone)]
pub struct LiveTemplate {
    /// Abbreviation that triggers the template (e.g., "sout")
    pub abbreviation: &'static str,
    /// User-visible description
    pub description: &'static str,
    /// The template expansion using LSP snippet syntax
    pub template: &'static str,
    /// Context where this template is valid
    pub context: TemplateContext,
}

/// Get all built-in live templates
pub fn get_live_templates() -> Vec<LiveTemplate> {
    vec![
        // ===== Print Statements =====
        LiveTemplate {
            abbreviation: "sout",
            description: "Print to stdout",
            template: "System.out.println(${1});$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "souf",
            description: "Print formatted to stdout",
            template: "System.out.printf(\"${1:%s}\", ${2});$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "soutv",
            description: "Print variable value",
            template: "System.out.println(\"${1:var} = \" + ${1:var});$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "serr",
            description: "Print to stderr",
            template: "System.err.println(${1});$0",
            context: TemplateContext::Statement,
        },

        // ===== Main Method =====
        LiveTemplate {
            abbreviation: "psvm",
            description: "public static void main",
            template: "public static void main(String[] args) {\n\t${0}\n}",
            context: TemplateContext::ClassBody,
        },
        LiveTemplate {
            abbreviation: "main",
            description: "public static void main",
            template: "public static void main(String[] args) {\n\t${0}\n}",
            context: TemplateContext::ClassBody,
        },

        // ===== Constants =====
        LiveTemplate {
            abbreviation: "psfs",
            description: "public static final String",
            template: "public static final String ${1:NAME} = \"${2}\";$0",
            context: TemplateContext::ClassBody,
        },
        LiveTemplate {
            abbreviation: "psfi",
            description: "public static final int",
            template: "public static final int ${1:NAME} = ${2:0};$0",
            context: TemplateContext::ClassBody,
        },
        LiveTemplate {
            abbreviation: "prsf",
            description: "private static final",
            template: "private static final ${1:String} ${2:NAME} = ${3};$0",
            context: TemplateContext::ClassBody,
        },

        // ===== Loops =====
        LiveTemplate {
            abbreviation: "fori",
            description: "for loop with index",
            template: "for (int ${1:i} = 0; ${1:i} < ${2:length}; ${1:i}++) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "iter",
            description: "enhanced for loop",
            template: "for (${1:Object} ${2:item} : ${3:collection}) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "itar",
            description: "iterate array with index",
            template: "for (int ${1:i} = 0; ${1:i} < ${2:array}.length; ${1:i}++) {\n\t${3:Object} ${4:item} = ${2:array}[${1:i}];\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "itli",
            description: "iterate List",
            template: "for (int ${1:i} = 0; ${1:i} < ${2:list}.size(); ${1:i}++) {\n\t${3:Object} ${4:item} = ${2:list}.get(${1:i});\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "while",
            description: "while loop",
            template: "while (${1:condition}) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },

        // ===== Conditionals =====
        LiveTemplate {
            abbreviation: "ifn",
            description: "if null check",
            template: "if (${1:var} == null) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "inn",
            description: "if not null check",
            template: "if (${1:var} != null) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "ife",
            description: "if-else",
            template: "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "inst",
            description: "instanceof check",
            template: "if (${1:var} instanceof ${2:Type}) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },

        // ===== Exception Handling =====
        LiveTemplate {
            abbreviation: "trycatch",
            description: "try-catch block",
            template: "try {\n\t${1}\n} catch (${2:Exception} ${3:e}) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "tryf",
            description: "try-finally block",
            template: "try {\n\t${1}\n} finally {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "trywr",
            description: "try-with-resources",
            template: "try (${1:Resource} ${2:res} = ${3:new Resource()}) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },

        // ===== Methods =====
        LiveTemplate {
            abbreviation: "pvm",
            description: "private void method",
            template: "private void ${1:name}(${2}) {\n\t${0}\n}",
            context: TemplateContext::ClassBody,
        },
        LiveTemplate {
            abbreviation: "puvm",
            description: "public void method",
            template: "public void ${1:name}(${2}) {\n\t${0}\n}",
            context: TemplateContext::ClassBody,
        },

        // ===== Common Patterns =====
        LiveTemplate {
            abbreviation: "lazy",
            description: "lazy initialization",
            template: "if (${1:field} == null) {\n\t${1:field} = new ${2:Type}();\n}\nreturn ${1:field};$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "singleton",
            description: "singleton pattern",
            template: "private static ${1:Class} instance;\n\nprivate ${1:Class}() {}\n\npublic static ${1:Class} getInstance() {\n\tif (instance == null) {\n\t\tinstance = new ${1:Class}();\n\t}\n\treturn instance;\n}$0",
            context: TemplateContext::ClassBody,
        },

        // ===== Assertions & Testing =====
        LiveTemplate {
            abbreviation: "asrt",
            description: "assert statement",
            template: "assert ${1:condition} : \"${2:message}\";$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "test",
            description: "JUnit test method",
            template: "@Test\npublic void ${1:testName}() {\n\t${0}\n}",
            context: TemplateContext::ClassBody,
        },

        // ===== Collections =====
        LiveTemplate {
            abbreviation: "toar",
            description: "convert to array",
            template: "${1:list}.toArray(new ${2:Type}[0])$0",
            context: TemplateContext::Expression,
        },
        LiveTemplate {
            abbreviation: "lst",
            description: "new ArrayList",
            template: "List<${1:Type}> ${2:list} = new ArrayList<>();$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "map",
            description: "new HashMap",
            template: "Map<${1:Key}, ${2:Value}> ${3:map} = new HashMap<>();$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "set",
            description: "new HashSet",
            template: "Set<${1:Type}> ${2:set} = new HashSet<>();$0",
            context: TemplateContext::Statement,
        },

        // ===== Synchronization =====
        LiveTemplate {
            abbreviation: "sync",
            description: "synchronized block",
            template: "synchronized (${1:this}) {\n\t${0}\n}",
            context: TemplateContext::Statement,
        },

        // ===== Logging =====
        LiveTemplate {
            abbreviation: "logd",
            description: "Logger debug",
            template: "logger.debug(\"${1}\");$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "logi",
            description: "Logger info",
            template: "logger.info(\"${1}\");$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "logw",
            description: "Logger warn",
            template: "logger.warn(\"${1}\");$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "loge",
            description: "Logger error",
            template: "logger.error(\"${1}\", ${2:e});$0",
            context: TemplateContext::Statement,
        },
        LiveTemplate {
            abbreviation: "logger",
            description: "SLF4J Logger field",
            template: "private static final Logger logger = LoggerFactory.getLogger(${1:ClassName}.class);$0",
            context: TemplateContext::ClassBody,
        },
    ]
}

/// Get template completions for the given prefix
///
/// # Arguments
/// * `prefix` - The text typed by the user (e.g., "sou")
/// * `context` - Current editing context (statement, class body, etc.)
///
/// # Returns
/// Completion items for matching templates
pub fn get_template_completions(prefix: &str, context: TemplateContext) -> Vec<CompletionItem> {
    let templates = get_live_templates();
    let prefix_lower = prefix.to_lowercase();

    templates
        .into_iter()
        .filter(|t| {
            // Match prefix
            let matches_prefix = t.abbreviation.to_lowercase().starts_with(&prefix_lower);

            // Match context
            let matches_context = t.context == TemplateContext::Any || t.context == context;

            matches_prefix && matches_context
        })
        .map(|t| {
            debug!("Creating completion for template: {}", t.abbreviation);
            CompletionItem {
                label: t.abbreviation.to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(t.description.to_string()),
                documentation: Some(tower_lsp::lsp_types::Documentation::String(format!(
                    "Expands to:\n{}",
                    preview_template(t.template)
                ))),
                insert_text: Some(t.template.to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                // Sort templates after regular completions but before keywords
                sort_text: Some(format!("1_{}", t.abbreviation)),
                filter_text: Some(t.abbreviation.to_string()),
                ..Default::default()
            }
        })
        .collect()
}

/// Create a preview of the template (strip snippet markers for display)
fn preview_template(template: &str) -> String {
    // Replace ${1:text} with just text
    let mut result = template.to_string();

    // Simple regex-like replacement for ${N:text} patterns
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let placeholder = &result[start..=(start + end)];
            // Extract the text part after ':'
            let replacement = if let Some(colon_pos) = placeholder.find(':') {
                placeholder[colon_pos + 1..placeholder.len() - 1].to_string()
            } else {
                // Just ${N} without text
                String::new()
            };
            result = result.replace(placeholder, &replacement);
        } else {
            break;
        }
    }

    // Remove $0 (final cursor)
    result = result.replace("$0", "|");

    result
}

/// Detect the template context (Statement, ClassBody, etc.)
/// This determines which templates are applicable at the cursor position
///
/// Uses heuristics to detect:
/// - ClassBody: Inside class braces but outside methods (depth 1, no method signature)
/// - Statement: Inside method/constructor/initializer body (depth 2+)
/// - Expression: After = or inside method call arguments (not yet implemented)
/// - TypePosition: In type declarations (not yet implemented)
pub fn detect_template_context(source_text: &str, position: Position) -> TemplateContext {
    use crate::position_utils::position_to_offset;

    let offset = match position_to_offset(source_text, position) {
        Ok(o) => o,
        Err(_) => return TemplateContext::Statement,
    };

    let before_cursor = &source_text[..offset];

    // Count brace depth while properly handling strings/chars/comments
    let brace_depth = count_brace_depth(before_cursor);

    // Look for method/constructor signature patterns before cursor
    // Check within the current brace scope (find last opening brace at this depth)
    let in_method_body = is_in_method_body(before_cursor);

    // Detect expression context (after = sign or inside parens)
    let in_expression = is_in_expression_context(before_cursor);

    if in_expression {
        TemplateContext::Expression
    } else if brace_depth <= 1 && !in_method_body {
        // At class level (depth 1 = inside class, outside methods)
        TemplateContext::ClassBody
    } else {
        // Inside method body
        TemplateContext::Statement
    }
}

/// Count brace nesting depth, properly handling strings, chars, and comments
fn count_brace_depth(text: &str) -> i32 {
    let mut depth: i32 = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Skip string literals
        if c == '"' {
            i += 1;
            // Check for text block (triple quote)
            if i + 1 < chars.len() && chars[i] == '"' && chars[i + 1] == '"' {
                i += 2; // Skip opening """
                        // Find closing """
                while i + 2 < chars.len() {
                    if chars[i] == '"' && chars[i + 1] == '"' && chars[i + 2] == '"' {
                        i += 3;
                        break;
                    }
                    i += 1;
                }
            } else {
                // Regular string - find closing "
                while i < chars.len() {
                    if chars[i] == '"' && (i == 0 || chars[i - 1] != '\\') {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            continue;
        }

        // Skip char literals
        if c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                if chars[i] == '\\' {
                    i += 1; // Skip escaped char
                }
                i += 1;
            }
            i += 1; // Skip closing '
            continue;
        }

        // Skip line comments
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Skip block comments
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() {
                if chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Count braces
        match c {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            _ => {}
        }
        i += 1;
    }

    depth
}

fn last_n_chars(text: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }

    let mut count = 0usize;
    for (idx, _) in text.char_indices().rev() {
        count += 1;
        if count == n {
            return &text[idx..];
        }
    }

    text
}

/// Check if cursor is inside a method/constructor body
fn is_in_method_body(text: &str) -> bool {
    // Look backwards for method signature patterns
    // Pattern: ) { or ) throws ... { before cursor
    // But not if we see class/interface/enum after the last {

    // Find the position of the last opening brace
    let mut depth = 0;
    let mut last_open_brace = None;
    let target_depth = count_brace_depth(text);
    let mut i = 0usize;

    while i < text.len() {
        let Some(c) = text[i..].chars().next() else {
            break;
        };
        let c_len = c.len_utf8();

        // Skip strings (simplified - full version in count_brace_depth)
        if c == '"' {
            i += c_len;
            while i < text.len() {
                let Some(inner) = text[i..].chars().next() else {
                    break;
                };
                let inner_len = inner.len_utf8();

                if inner == '\\' {
                    i += inner_len;
                    if i < text.len() {
                        if let Some(escaped) = text[i..].chars().next() {
                            i += escaped.len_utf8();
                        }
                    }
                    continue;
                }

                i += inner_len;
                if inner == '"' {
                    break;
                }
            }
            continue;
        }

        match c {
            '{' => {
                depth += 1;
                if depth == target_depth {
                    last_open_brace = Some(i);
                }
            }
            '}' => depth = (depth - 1).max(0),
            _ => {}
        }
        i += c_len;
    }

    if let Some(brace_pos) = last_open_brace {
        // Look at what comes before this brace
        let before_brace = &text[..brace_pos];
        let trimmed = before_brace.trim_end();

        // Check for method signature: ends with ) or throws clause
        if trimmed.ends_with(')') || trimmed.ends_with("throws") || trimmed.contains(") throws ") {
            return true;
        }

        // Check for lambda: ends with ->
        if trimmed.ends_with("->") {
            return true;
        }

        // Check if it's a class/interface/enum body (NOT a method)
        // Look for keywords before the brace
        let recent = last_n_chars(trimmed, 100);
        if recent.contains("class ")
            || recent.contains("interface ")
            || recent.contains("enum ")
            || recent.contains("record ")
        {
            // Check that these keywords are after the last )
            // which would indicate class declaration, not method
            if let Some(last_paren) = recent.rfind(')') {
                let after_paren = &recent[last_paren..];
                if !after_paren.contains("class ")
                    && !after_paren.contains("interface ")
                    && !after_paren.contains("enum ")
                    && !after_paren.contains("record ")
                {
                    return true;
                }
            } else {
                return false; // No ) means it's a class body
            }
        }

        // Default: if depth > 1, we're likely in a method
        count_brace_depth(text) > 1
    } else {
        false
    }
}

/// Check if cursor is in an expression context (after = or inside parens)
fn is_in_expression_context(text: &str) -> bool {
    let trimmed = text.trim_end();

    // After assignment
    if trimmed.ends_with('=') && !trimmed.ends_with("==") && !trimmed.ends_with("!=") {
        return true;
    }

    // Inside method call arguments (more open parens than close)
    let open_parens = trimmed.chars().filter(|&c| c == '(').count();
    let close_parens = trimmed.chars().filter(|&c| c == ')').count();

    if open_parens > close_parens {
        // Check if we're not in a method signature (which also has open parens)
        // Method signatures have a return type before the paren
        let before_last_paren = if let Some(pos) = trimmed.rfind('(') {
            trimmed[..pos].trim_end()
        } else {
            trimmed
        };

        // If the last word before ( is a method name (lowercase), we're in a call
        // If it's a type name (uppercase) followed by method name, we're in signature
        if let Some(last_word) = before_last_paren.split_whitespace().last() {
            // Method calls: foo(, bar(, this.method(
            // Signatures: void foo(, String bar(, public void test(
            if !is_likely_type_keyword(last_word) {
                return true;
            }
        }
    }

    false
}

/// Check if a word is likely a type keyword (suggesting method signature, not call)
fn is_likely_type_keyword(word: &str) -> bool {
    matches!(
        word,
        "void" | "int" | "long" | "double" | "float" | "boolean" | "byte" | "short" | "char"
    ) || (word.chars().next().is_some_and(char::is_uppercase)
        && word.chars().all(char::is_alphanumeric))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_live_templates() {
        let templates = get_live_templates();
        assert!(templates.len() >= 30, "Should have at least 30 templates");

        // Check some key templates exist
        let abbrevs: Vec<_> = templates.iter().map(|t| t.abbreviation).collect();
        assert!(abbrevs.contains(&"sout"), "Should have sout template");
        assert!(abbrevs.contains(&"psvm"), "Should have psvm template");
        assert!(abbrevs.contains(&"fori"), "Should have fori template");
    }

    #[test]
    fn test_get_template_completions() {
        let completions = get_template_completions("sou", TemplateContext::Statement);
        assert!(!completions.is_empty(), "Should find sout template");

        let completions = get_template_completions("ps", TemplateContext::ClassBody);
        assert!(completions.len() >= 2, "Should find psvm, psfs, etc.");
    }

    #[test]
    fn test_context_filtering() {
        // psvm should only appear in ClassBody context
        let completions = get_template_completions("psvm", TemplateContext::Statement);
        assert!(
            completions.is_empty(),
            "psvm should not appear in statement context"
        );

        let completions = get_template_completions("psvm", TemplateContext::ClassBody);
        assert!(
            !completions.is_empty(),
            "psvm should appear in class body context"
        );
    }

    #[test]
    fn test_preview_template() {
        let preview = preview_template("System.out.println(${1:text});$0");
        assert_eq!(preview, "System.out.println(text);|");
    }

    #[test]
    fn test_count_brace_depth_simple() {
        assert_eq!(count_brace_depth(""), 0);
        assert_eq!(count_brace_depth("{"), 1);
        assert_eq!(count_brace_depth("{}"), 0);
        assert_eq!(count_brace_depth("{{"), 2);
        assert_eq!(count_brace_depth("{{}"), 1);
    }

    #[test]
    fn test_count_brace_depth_with_strings() {
        // Braces inside strings should not count
        assert_eq!(count_brace_depth(r#""{""#), 0);
        assert_eq!(count_brace_depth(r#""{" {"#), 1);
        assert_eq!(count_brace_depth(r#"class Foo { String s = "{";"#), 1);
    }

    #[test]
    fn test_count_brace_depth_with_comments() {
        // Braces inside comments should not count
        assert_eq!(count_brace_depth("// {"), 0);
        assert_eq!(count_brace_depth("/* { */"), 0);
        assert_eq!(count_brace_depth("{ // {"), 1);
        assert_eq!(count_brace_depth("{ /* { */ {"), 2);
    }

    #[test]
    fn test_is_in_method_body() {
        // Inside method body
        assert!(is_in_method_body("class Foo { void bar() { "));
        assert!(is_in_method_body("class Foo { public void bar() { "));
        assert!(is_in_method_body(
            "class Foo { void bar() throws Exception { "
        ));

        // Inside class body (not method)
        assert!(!is_in_method_body("class Foo { "));
        assert!(!is_in_method_body("interface Bar { "));
        assert!(!is_in_method_body("enum Baz { "));
    }

    #[test]
    fn test_is_in_expression_context() {
        // After assignment
        assert!(is_in_expression_context("int x = "));
        assert!(is_in_expression_context("String s = "));

        // Inside parens (including if conditions) is expression context
        assert!(is_in_expression_context("if (x == ")); // Inside if parens = expression
        assert!(is_in_expression_context("if (x != ")); // Inside if parens = expression

        // Inside method call
        assert!(is_in_expression_context("foo("));
        assert!(is_in_expression_context("bar.method("));

        // Balanced parens = not expression context (unless after =)
        assert!(!is_in_expression_context("if (x == y) { "));
        assert!(!is_in_expression_context("class Foo { "));
    }

    #[test]
    fn test_detect_context_class_body() {
        let source = "class Foo {\n    \n}";
        let context = detect_template_context(source, Position::new(1, 4));
        assert_eq!(context, TemplateContext::ClassBody);
    }

    #[test]
    fn test_detect_context_method_body() {
        let source = "class Foo {\n    void bar() {\n        \n    }\n}";
        let context = detect_template_context(source, Position::new(2, 8));
        assert_eq!(context, TemplateContext::Statement);
    }
}
