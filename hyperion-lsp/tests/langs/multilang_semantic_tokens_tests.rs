// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Product contracts for tree-sitter language semantic highlighting.

use hyperion_lsp::semantic_host::SemanticHost;
use std::sync::Arc;

use hyperion_lsp::semantic_tokens::SemanticTokensProvider;
use hyperion_semantics::db::FileId;
use hyperion_workspace::Language;

const CLASS: u32 = 0;
const INTERFACE: u32 = 1;
const METHOD: u32 = 4;
const VARIABLE: u32 = 6;
const PARAMETER: u32 = 7;
const COMMENT: u32 = 10;
const STRING: u32 = 11;
const DECLARATION: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
struct Fact {
    text: String,
    token_type: u32,
    modifiers: u32,
}

fn facts(source: &str, language: Language) -> Vec<Fact> {
    let host = SemanticHost::for_tests();
    let file_id = FileId::new(0);
    host.with_db(|db| db.register_file(file_id, Arc::new(source.to_string()), language));
    let tokens = SemanticTokensProvider::new()
        .semantic_tokens(&host.read_now(), file_id, source)
        .expect("semantic token request");
    assert!(
        !tokens.is_empty(),
        "{language:?} must produce semantic tokens"
    );

    let lines: Vec<_> = source
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let mut line = 0u32;
    let mut column = 0u32;
    tokens
        .into_iter()
        .map(|token| {
            if token.delta_line == 0 {
                column += token.delta_start;
            } else {
                line += token.delta_line;
                column = token.delta_start;
            }
            let text = utf16_slice(lines[line as usize], column, column + token.length);
            Fact {
                text,
                token_type: token.token_type,
                modifiers: token.token_modifiers_bitset,
            }
        })
        .collect()
}

fn utf16_slice(line: &str, start: u32, end: u32) -> String {
    let mut utf16_offset = 0u32;
    let mut start_byte = None;
    let mut end_byte = None;
    for (byte, character) in line.char_indices() {
        if utf16_offset == start {
            start_byte = Some(byte);
        }
        if utf16_offset == end {
            end_byte = Some(byte);
            break;
        }
        utf16_offset += character.len_utf16() as u32;
    }
    if utf16_offset == start {
        start_byte.get_or_insert(line.len());
    }
    if utf16_offset == end {
        end_byte.get_or_insert(line.len());
    }
    line[start_byte.expect("token starts at UTF-16 boundary")
        ..end_byte.expect("token ends at UTF-16 boundary")]
        .to_string()
}

fn has(facts: &[Fact], text: &str, token_type: u32, modifiers: u32) -> bool {
    facts.iter().any(|fact| {
        fact.text == text && fact.token_type == token_type && fact.modifiers == modifiers
    })
}

#[test]
fn groovy_tokens_preserve_meaning_and_utf16_ranges() {
    let source = "class Café {\r\n  def greet(String who) { \"hello\" }\r\n  // note\r\n}";
    let facts = facts(source, Language::Groovy);

    assert!(has(&facts, "Café", CLASS, DECLARATION));
    assert!(has(&facts, "greet", METHOD, DECLARATION));
    assert!(has(&facts, "who", PARAMETER, DECLARATION));
    assert!(has(&facts, "\"hello\"", STRING, 0));
    assert!(has(&facts, "// note", COMMENT, 0));
}

#[test]
fn scala_tokens_preserve_meaning_and_utf16_ranges() {
    let source = "trait Gréeter {\r\n  val message = \"hello\"\r\n  def greet(name: String): String\r\n  // note\r\n}";
    let facts = facts(source, Language::Scala);

    assert!(has(&facts, "Gréeter", INTERFACE, DECLARATION));
    assert!(has(&facts, "message", VARIABLE, DECLARATION | (1 << 2)));
    assert!(has(&facts, "greet", METHOD, DECLARATION));
    assert!(has(&facts, "name", PARAMETER, DECLARATION));
    assert!(has(&facts, "\"hello\"", STRING, 0));
    assert!(has(&facts, "// note", COMMENT, 0));
}
