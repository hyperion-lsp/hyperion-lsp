// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Snippet detection and wrapping for eval
//!
//! Detects whether input is an expression, statement, or full class,
//! and wraps snippets in appropriate class structures for compilation.

use crate::executor::Language;

/// Type of code snippet being evaluated
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetType {
    /// A single expression that returns a value: `1+1`, `"hello".length()`
    Expression,
    /// A statement that may or may not produce output: `System.out.println("Hi");`
    Statement,
    /// A complete class definition: `class Foo { ... }`
    FullClass,
}

/// Detect the type of code snippet
pub fn detect_snippet_type(code: &str, lang: Language) -> SnippetType {
    let trimmed = code.trim();

    // Check for full class/object definitions
    match lang {
        Language::Java => {
            if looks_like_java_compilation_unit(trimmed) {
                return SnippetType::FullClass;
            }
        }
        Language::Kotlin => {
            if trimmed.starts_with("class ")
                || trimmed.starts_with("data class ")
                || trimmed.starts_with("object ")
                || trimmed.starts_with("interface ")
                || trimmed.starts_with("enum class ")
                || trimmed.starts_with("sealed class ")
            {
                return SnippetType::FullClass;
            }
        }
        Language::Groovy => {
            if trimmed.starts_with("class ")
                || trimmed.starts_with("interface ")
                || trimmed.starts_with("enum ")
                || trimmed.starts_with("trait ")
            {
                return SnippetType::FullClass;
            }
        }
    }

    // Check for statements (end with semicolon, or are assignments/declarations)
    if trimmed.ends_with(';')
        || trimmed.starts_with("var ")
        || trimmed.starts_with("val ")
        || trimmed.starts_with("def ")
        || trimmed.starts_with("int ")
        || trimmed.starts_with("String ")
        || trimmed.starts_with("boolean ")
        || trimmed.starts_with("long ")
        || trimmed.starts_with("double ")
        || trimmed.starts_with("float ")
        || trimmed.starts_with("if ")
        || trimmed.starts_with("if(")
        || trimmed.starts_with("for ")
        || trimmed.starts_with("for(")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("while(")
    {
        return SnippetType::Statement;
    }

    // Default: assume expression
    SnippetType::Expression
}

/// Heuristically decide whether Java source is a full compilation unit (a real
/// `.java` file) rather than a bare expression/statement snippet.
///
/// Real files commonly begin with a `package`/`import` declaration, license
/// comments, or annotations, and declare the type with modifiers (`public
/// final class ...`). We scan to the first significant line and check it.
fn looks_like_java_compilation_unit(code: &str) -> bool {
    let mut in_block_comment = false;
    for line in code.lines() {
        let mut l = line.trim();
        if in_block_comment {
            if let Some(idx) = l.find("*/") {
                l = l[idx + 2..].trim();
                in_block_comment = false;
            } else {
                continue;
            }
        }
        if l.is_empty() || l.starts_with("//") || l.starts_with('@') {
            continue;
        }
        if l.starts_with("/*") {
            if !l.contains("*/") {
                in_block_comment = true;
            }
            continue;
        }
        if l.starts_with("package ") || l.starts_with("import ") {
            return true;
        }
        // Strip leading type modifiers, then look for a type-declaration keyword.
        for kw in [
            "public ",
            "private ",
            "protected ",
            "final ",
            "abstract ",
            "sealed ",
            "non-sealed ",
            "strictfp ",
            "static ",
        ] {
            while let Some(rest) = l.strip_prefix(kw) {
                l = rest.trim_start();
            }
        }
        return l.starts_with("class ")
            || l.starts_with("interface ")
            || l.starts_with("enum ")
            || l.starts_with("record ")
            || l.starts_with("@interface ");
    }
    false
}

/// Wrap a code snippet in an appropriate class structure for compilation
pub fn wrap_snippet(code: &str, snippet_type: SnippetType, lang: Language) -> String {
    match snippet_type {
        SnippetType::FullClass => code.to_string(),
        SnippetType::Expression => wrap_expression(code, lang),
        SnippetType::Statement => wrap_statement(code, lang),
    }
}

fn wrap_expression(code: &str, lang: Language) -> String {
    match lang {
        Language::Java => {
            format!(
                r"public class __Eval__ {{
    public static Object __eval__() {{
        return ({code});
    }}
}}"
            )
        }
        Language::Kotlin => {
            // Kotlin uses fun main() style or top-level functions
            // For eval, we wrap in a simple function
            format!(
                r"fun __eval__(): Any? {{
    return ({code})
}}"
            )
        }
        Language::Groovy => {
            // Groovy scripts use implicit return
            format!("return ({code})")
        }
    }
}

fn wrap_statement(code: &str, lang: Language) -> String {
    match lang {
        Language::Java => {
            format!(
                r"public class __Eval__ {{
    public static void __eval__() {{
        {code}
    }}
}}"
            )
        }
        Language::Kotlin => {
            format!(
                r"fun __eval__() {{
    {code}
}}"
            )
        }
        Language::Groovy => {
            // Groovy: if code doesn't end with semicolon or brace, the last part might be
            // an expression that should be returned. Try to detect and wrap appropriately.
            let trimmed = code.trim();
            if !trimmed.ends_with(';') && !trimmed.ends_with('}') {
                // Code ends with an expression - find the last semicolon and wrap
                // the trailing part with return
                if let Some(last_semi) = trimmed.rfind(';') {
                    let (statements, expr) = trimmed.split_at(last_semi + 1);
                    let expr = expr.trim();
                    if !expr.is_empty() {
                        return format!("{statements}\nreturn ({expr})");
                    }
                }
            }
            // Groovy scripts can execute statements directly
            code.to_string()
        }
    }
}

/// Detect language from file extension
pub fn detect_language_from_extension(path: &str) -> Option<Language> {
    let ext = std::path::Path::new(path).extension()?;
    let ext = ext.to_ascii_lowercase();
    match ext.to_str()? {
        "java" => Some(Language::Java),
        "kt" | "kts" => Some(Language::Kotlin),
        "groovy" | "gradle" => Some(Language::Groovy),
        _ => None,
    }
}

/// Detect language from code content heuristics
pub fn detect_language_from_content(code: &str) -> Language {
    let trimmed = code.trim();

    // Kotlin indicators
    if trimmed.contains("fun ")
        || trimmed.contains("val ")
        || trimmed.starts_with("package ")
            && (trimmed.contains("import kotlin") || trimmed.contains("import kotlinx"))
    {
        return Language::Kotlin;
    }

    // Groovy indicators
    if trimmed.starts_with("def ")
        || trimmed.contains("def ")
        || trimmed.contains("->")
        || trimmed.contains(".each")
        || trimmed.contains(".collect")
        || trimmed.contains("GString")
    {
        return Language::Groovy;
    }

    // Default to Java
    Language::Java
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_expression() {
        assert_eq!(
            detect_snippet_type("1 + 1", Language::Java),
            SnippetType::Expression
        );
        assert_eq!(
            detect_snippet_type("\"hello\".length()", Language::Java),
            SnippetType::Expression
        );
        assert_eq!(
            detect_snippet_type("Math.max(1, 2)", Language::Java),
            SnippetType::Expression
        );
    }

    #[test]
    fn test_detect_statement() {
        assert_eq!(
            detect_snippet_type("int x = 5;", Language::Java),
            SnippetType::Statement
        );
        assert_eq!(
            detect_snippet_type("System.out.println(\"Hi\");", Language::Java),
            SnippetType::Statement
        );
        assert_eq!(
            detect_snippet_type("var x = 10", Language::Kotlin),
            SnippetType::Statement
        );
        assert_eq!(
            detect_snippet_type("def x = 5", Language::Groovy),
            SnippetType::Statement
        );
    }

    #[test]
    fn test_detect_full_class() {
        assert_eq!(
            detect_snippet_type("class Foo { }", Language::Java),
            SnippetType::FullClass
        );
        assert_eq!(
            detect_snippet_type("public class Bar { }", Language::Java),
            SnippetType::FullClass
        );
        assert_eq!(
            detect_snippet_type("data class User(val name: String)", Language::Kotlin),
            SnippetType::FullClass
        );
        assert_eq!(
            detect_snippet_type("class Test { def run() {} }", Language::Groovy),
            SnippetType::FullClass
        );
    }

    #[test]
    fn test_detect_full_class_real_files() {
        // Real .java files lead with package/imports/comments and use modifiers.
        let pkg = "package com.example;\n\npublic final class Foo {\n  public static void main(String[] a) {}\n}";
        assert_eq!(
            detect_snippet_type(pkg, Language::Java),
            SnippetType::FullClass
        );

        let commented = "/*\n * License header\n */\n// note\npublic class Bar {}";
        assert_eq!(
            detect_snippet_type(commented, Language::Java),
            SnippetType::FullClass
        );

        let modifiers = "final class Baz {}";
        assert_eq!(
            detect_snippet_type(modifiers, Language::Java),
            SnippetType::FullClass
        );

        let import_only = "import java.util.List;\nclass Qux {}";
        assert_eq!(
            detect_snippet_type(import_only, Language::Java),
            SnippetType::FullClass
        );

        // A bare statement must NOT be misclassified as a full class.
        assert_eq!(
            detect_snippet_type("int x = 5;", Language::Java),
            SnippetType::Statement
        );
        // ...nor should an expression.
        assert_eq!(
            detect_snippet_type("1 + 2", Language::Java),
            SnippetType::Expression
        );
    }

    #[test]
    fn test_wrap_expression_java() {
        let wrapped = wrap_snippet("1 + 2", SnippetType::Expression, Language::Java);
        assert!(wrapped.contains("public class __Eval__"));
        assert!(wrapped.contains("return (1 + 2)"));
    }

    #[test]
    fn test_wrap_expression_groovy() {
        let wrapped = wrap_snippet("1 + 2", SnippetType::Expression, Language::Groovy);
        assert!(wrapped.contains("return (1 + 2)"));
    }

    #[test]
    fn test_detect_language_from_extension() {
        assert_eq!(
            detect_language_from_extension("Test.java"),
            Some(Language::Java)
        );
        assert_eq!(
            detect_language_from_extension("Main.kt"),
            Some(Language::Kotlin)
        );
        assert_eq!(
            detect_language_from_extension("build.gradle"),
            Some(Language::Groovy)
        );
        assert_eq!(detect_language_from_extension("README.md"), None);
    }

    #[test]
    fn test_detect_language_from_content() {
        assert_eq!(detect_language_from_content("def x = 5"), Language::Groovy);
        assert_eq!(detect_language_from_content("fun test()"), Language::Kotlin);
        // Default to Java
        assert_eq!(detect_language_from_content("int x = 5;"), Language::Java);
        assert_eq!(detect_language_from_content("1 + 2"), Language::Java);
    }
}
