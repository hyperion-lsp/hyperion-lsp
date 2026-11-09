// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Minimal recovery lexer for Groovy member access and declaration scopes.
//!
//! String/comment/import ranges are supplied by the CST walker and become
//! barriers, so identifiers on opposite sides can never form a fabricated
//! access. Consecutive irrelevant punctuation is coalesced to bound memory.

#[derive(Clone, Copy, Debug)]
pub(super) struct IdentifierToken {
    pub(super) start: usize,
    pub(super) end: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Token {
    Identifier(IdentifierToken),
    Punctuation(Punctuation, usize),
    Barrier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Punctuation {
    Dot,
    SafeDot,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Equals,
}

impl Token {
    pub(super) fn is_punctuation(self, punctuation: Punctuation) -> bool {
        matches!(self, Self::Punctuation(actual, _) if actual == punctuation)
    }

    pub(super) fn is_member_access(self) -> bool {
        matches!(
            self,
            Self::Punctuation(Punctuation::Dot | Punctuation::SafeDot, _)
        )
    }

    pub(super) fn start(self) -> usize {
        match self {
            Self::Identifier(identifier) => identifier.start,
            Self::Punctuation(_, offset) => offset,
            Self::Barrier => 0,
        }
    }

    pub(super) fn end(self) -> usize {
        match self {
            Self::Identifier(identifier) => identifier.end,
            Self::Punctuation(Punctuation::SafeDot, offset) => offset + 2,
            Self::Punctuation(_, offset) => offset + 1,
            Self::Barrier => 0,
        }
    }
}

pub(super) fn matching_pairs(
    tokens: &[Token],
    open: Punctuation,
    close: Punctuation,
) -> Vec<Option<usize>> {
    let mut matches = vec![None; tokens.len()];
    let mut pending = Vec::new();
    for (index, token) in tokens.iter().copied().enumerate() {
        if token.is_punctuation(open) {
            pending.push(index);
        } else if token.is_punctuation(close) {
            if let Some(open_index) = pending.pop() {
                matches[open_index] = Some(index);
                matches[index] = Some(open_index);
            }
        }
    }
    matches
}

pub(super) fn enclosing_brace_scope(
    tokens: &[Token],
    brace_pairs: &[Option<usize>],
    target_index: usize,
) -> Option<(usize, usize)> {
    (0..target_index)
        .rev()
        .filter(|index| tokens[*index].is_punctuation(Punctuation::LeftBrace))
        .find_map(|open| {
            let close = brace_pairs.get(open).copied().flatten()?;
            (target_index < close).then(|| (tokens[open].start(), tokens[close].end()))
        })
}

pub(super) fn member_tokens(source: &str, excluded_ranges: &[(usize, usize)]) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut offset = 0;
    let mut excluded = 0;

    while offset < bytes.len() {
        while excluded < excluded_ranges.len() && excluded_ranges[excluded].1 <= offset {
            excluded += 1;
        }
        if let Some(&(start, end)) = excluded_ranges.get(excluded) {
            if start <= offset && offset < end {
                push_barrier(&mut tokens);
                offset = end;
                continue;
            }
        }

        match bytes[offset] {
            byte if byte.is_ascii_whitespace() => offset += 1,
            byte if byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') => {
                let start = offset;
                offset += 1;
                while bytes
                    .get(offset)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
                {
                    offset += 1;
                }
                tokens.push(Token::Identifier(IdentifierToken { start, end: offset }));
            }
            b'?' if bytes.get(offset + 1) == Some(&b'.') => {
                tokens.push(Token::Punctuation(Punctuation::SafeDot, offset));
                offset += 2;
            }
            b'.' => {
                tokens.push(Token::Punctuation(Punctuation::Dot, offset));
                offset += 1;
            }
            b'(' => {
                tokens.push(Token::Punctuation(Punctuation::LeftParen, offset));
                offset += 1;
            }
            b')' => {
                tokens.push(Token::Punctuation(Punctuation::RightParen, offset));
                offset += 1;
            }
            b'{' => {
                tokens.push(Token::Punctuation(Punctuation::LeftBrace, offset));
                offset += 1;
            }
            b'}' => {
                tokens.push(Token::Punctuation(Punctuation::RightBrace, offset));
                offset += 1;
            }
            b'=' => {
                tokens.push(Token::Punctuation(Punctuation::Equals, offset));
                offset += 1;
            }
            _ => {
                push_barrier(&mut tokens);
                offset += 1;
            }
        }
    }
    tokens
}

fn push_barrier(tokens: &mut Vec<Token>) {
    if !matches!(tokens.last(), Some(Token::Barrier)) {
        tokens.push(Token::Barrier);
    }
}
