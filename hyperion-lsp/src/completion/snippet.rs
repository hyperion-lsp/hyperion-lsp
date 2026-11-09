// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Snippet syntax handling for clients without `snippetSupport`.

use tower_lsp::lsp_types::{CompletionItem, CompletionTextEdit, InsertTextFormat};

/// Render LSP snippet syntax as the plain text a snippet-less client should
/// insert: placeholders become their default text, tab stops disappear, and
/// escapes are resolved.
#[must_use]
pub fn snippet_to_plain(snippet: &str) -> String {
    let chars: Vec<char> = snippet.chars().collect();
    let mut out = String::with_capacity(snippet.len());
    render(&chars, &mut 0, &mut out, false);
    out
}

fn render(chars: &[char], index: &mut usize, out: &mut String, nested: bool) {
    while *index < chars.len() {
        let c = chars[*index];
        match c {
            '\\' if chars
                .get(*index + 1)
                .is_some_and(|next| matches!(next, '$' | '}' | '\\')) =>
            {
                out.push(chars[*index + 1]);
                *index += 2;
            }
            '}' if nested => return,
            '$' => {
                *index += 1;
                match chars.get(*index) {
                    Some('{') => {
                        *index += 1;
                        while chars.get(*index).is_some_and(char::is_ascii_digit) {
                            *index += 1;
                        }
                        match chars.get(*index) {
                            Some(':') => {
                                *index += 1;
                                render(chars, index, out, true);
                                *index += 1; // closing brace
                            }
                            Some('|') => {
                                *index += 1;
                                while chars.get(*index).is_some_and(|c| !matches!(c, ',' | '|')) {
                                    out.push(chars[*index]);
                                    *index += 1;
                                }
                                while chars.get(*index).is_some_and(|c| *c != '}') {
                                    *index += 1;
                                }
                                *index += 1;
                            }
                            _ => {
                                while chars.get(*index).is_some_and(|c| *c != '}') {
                                    *index += 1;
                                }
                                *index += 1;
                            }
                        }
                    }
                    Some(digit) if digit.is_ascii_digit() => {
                        while chars.get(*index).is_some_and(char::is_ascii_digit) {
                            *index += 1;
                        }
                    }
                    _ => out.push('$'),
                }
            }
            other => {
                out.push(other);
                *index += 1;
            }
        }
    }
}

/// Rewrite every snippet item to plain text.
pub fn downgrade_snippets(items: &mut [CompletionItem]) {
    for item in items {
        if item.insert_text_format != Some(InsertTextFormat::SNIPPET) {
            continue;
        }
        item.insert_text_format = Some(InsertTextFormat::PLAIN_TEXT);
        if let Some(text) = &item.insert_text {
            item.insert_text = Some(snippet_to_plain(text));
        }
        match &mut item.text_edit {
            Some(CompletionTextEdit::Edit(edit)) => {
                edit.new_text = snippet_to_plain(&edit.new_text);
            }
            Some(CompletionTextEdit::InsertAndReplace(edit)) => {
                edit.new_text = snippet_to_plain(&edit.new_text);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::snippet_to_plain;

    #[test]
    fn placeholders_expand_to_defaults() {
        assert_eq!(
            snippet_to_plain("for (${1:int i = 0}; ${2:i < n}; ${3:i++}) {\n\t$0\n}"),
            "for (int i = 0; i < n; i++) {\n\t\n}"
        );
    }

    #[test]
    fn nested_choice_and_escapes() {
        assert_eq!(
            snippet_to_plain("${1:a ${2:b}} ${3|x,y|} \\$5 $1"),
            "a b x $5 "
        );
    }
}
