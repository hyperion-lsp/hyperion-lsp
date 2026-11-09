// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Small lexical helpers shared by the static Gradle model extractors.

/// Strip line and block comments without treating comment markers inside quoted strings as syntax.
pub(crate) fn strip_comments(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut chars = content.chars().peekable();
    let mut quote = None;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if let Some(delimiter) = quote {
            result.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == delimiter {
                quote = None;
            }
            continue;
        }

        if matches!(c, '\'' | '"') {
            quote = Some(c);
            result.push(c);
            continue;
        }

        if c != '/' {
            result.push(c);
            continue;
        }

        match chars.peek() {
            Some('/') => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        result.push('\n');
                        break;
                    }
                }
            }
            Some('*') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => result.push(c),
        }
    }

    result
}

/// Split one script line at statement and block boundaries outside quoted text.
pub(crate) fn split_statements(line: &str) -> impl Iterator<Item = &str> {
    let mut quote = None;
    let mut escaped = false;
    line.split(move |c| {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == delimiter {
                quote = None;
            }
            false
        } else if matches!(c, '\'' | '"' | '`') {
            quote = Some(c);
            false
        } else {
            matches!(c, '{' | '}' | ';')
        }
    })
}

/// Return the contents of the first named brace block, accounting for nested braces and strings.
pub(crate) fn find_named_block<'a>(content: &'a str, name: &str) -> Option<&'a str> {
    let mut outer_quote = None;
    let mut outer_escaped = false;
    for (name_start, c) in content.char_indices() {
        if let Some(delimiter) = outer_quote {
            if outer_escaped {
                outer_escaped = false;
            } else if c == '\\' {
                outer_escaped = true;
            } else if c == delimiter {
                outer_quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"' | '`') {
            outer_quote = Some(c);
            continue;
        }
        if !content[name_start..].starts_with(name) {
            continue;
        }

        let name_end = name_start + name.len();
        let valid_start = content[..name_start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_identifier_char(c));
        let valid_end = content[name_end..]
            .chars()
            .next()
            .is_none_or(|c| !is_identifier_char(c));
        if !valid_start || !valid_end {
            continue;
        }

        let after_name = &content[name_end..];
        let whitespace = after_name.len() - after_name.trim_start().len();
        let open = name_end + whitespace;
        if content.as_bytes().get(open) != Some(&b'{') {
            continue;
        }

        let body_start = open + 1;
        let mut depth = 1_u32;
        let mut quote = None;
        let mut escaped = false;
        for (relative, c) in content[body_start..].char_indices() {
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == delimiter {
                    quote = None;
                }
                continue;
            }

            if matches!(c, '\'' | '"' | '`') {
                quote = Some(c);
            } else if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                if depth == 0 {
                    return Some(&content[body_start..body_start + relative]);
                }
            }
        }
        return None;
    }
    None
}

fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statement_boundaries_preserve_quoted_delimiters() {
        let statements: Vec<_> = split_statements(
            r#"dependencies { implementation "group:semi;brace{}:1"; runtimeOnly 'group:other:2' }"#,
        ).map(str::trim).filter(|part| !part.is_empty()).collect();
        assert_eq!(
            statements,
            vec![
                "dependencies",
                r#"implementation "group:semi;brace{}:1""#,
                "runtimeOnly 'group:other:2'"
            ]
        );
    }

    #[test]
    fn comments_preserve_markers_inside_escaped_strings() {
        assert_eq!(
            strip_comments("include 'a' // comment\ninclude \"https://repo/\\\"stable\""),
            "include 'a' \ninclude \"https://repo/\\\"stable\""
        );
        assert_eq!(
            strip_comments("include 'a' /* comment */ include 'b'"),
            "include 'a'  include 'b'"
        );
    }

    #[test]
    fn named_block_handles_nesting_strings_and_identifier_boundaries() {
        let source = r#"
            notplugins { ignored }
            val description = "plugins { alsoIgnored }"
            plugins {
                id("example.with.a.}")
                custom { nested() }
                `java-library`
            }
        "#;
        let block = find_named_block(source, "plugins").expect("plugins block");
        assert!(block.contains("example.with.a.}"));
        assert!(block.contains("custom { nested() }"));
        assert!(block.contains("`java-library`"));
        assert!(!block.contains("ignored"));
        assert!(!block.contains("alsoIgnored"));
    }
}
