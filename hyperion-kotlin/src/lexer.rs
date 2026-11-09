// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Lexer Implementation with Rowan Integration
//!
//! This module provides a high-performance lexer for Kotlin source code using the `logos`
//! crate for efficient token pattern matching, integrated with the rowan-based CST system.
//!
//! # Architecture
//!
//! The lexer uses a two-layer architecture:
//!
//! 1. **Internal Layer (LogosToken)**: Private enum used by logos for pattern matching.
//!    This enum is optimized for lexer performance and matches Kotlin source patterns.
//!
//! 2. **External Layer (SyntaxKind)**: Public rowan-compatible enum from syntax.rs.
//!    This is what the parser and CST consume. Every LogosToken maps to a SyntaxKind.
//!
//! The public `Lexer` API wraps the logos lexer and yields `(SyntaxKind, &str, TextRange)`
//! tuples, making it directly compatible with rowan's green tree builder.
//!
//! # Features
//!
//! - Zero-copy tokenization using string slices
//! - Support for all Kotlin keywords
//! - Complete operator and delimiter support (including Kotlin-specific operators)
//! - String literals (including raw strings with triple quotes)
//! - Numeric literals (binary, octal, decimal, hexadecimal with underscores)
//! - Comments (line and block)
//! - Error recovery for invalid tokens
//! - Rowan-compatible iterator-based API

use crate::syntax::SyntaxKind;
use logos::{Lexer as LogosLexer, Logos, SpannedIter};
use rowan::{TextRange, TextSize};

/// Callback to lex block comments (`/* ... */`).
/// Handles edge cases like `/* test **/` correctly.
fn lex_block_comment(lex: &mut LogosLexer<LogosToken>) -> bool {
    let remainder = lex.remainder();
    let mut chars = remainder.chars().peekable();
    let mut len = 0;

    // Already matched `/*`, scan until `*/`
    loop {
        match chars.next() {
            Some('*') => {
                len += 1;
                // Check if followed by `/`
                if chars.peek() == Some(&'/') {
                    chars.next();
                    len += 1;
                    lex.bump(len);
                    return true;
                }
            }
            Some(c) => {
                len += c.len_utf8();
            }
            None => {
                // Unterminated comment - don't match
                return false;
            }
        }
    }
}

/// Scan a raw string body starting right after the opening `"""`.
/// Returns the byte length up to and including the closing quotes.
fn scan_raw_string(rest: &str) -> Option<usize> {
    let close = rest.find("\"\"\"")?;
    let extra_quotes = rest[close + 3..].bytes().take_while(|b| *b == b'"').count();
    Some(close + 3 + extra_quotes)
}

/// Scan a `${ ... }` template expression body starting right after `${`.
/// Returns the byte length up to and including the matching `}`.
fn scan_template_expression(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut depth = 1usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            b'"' => {
                i += if rest[i..].starts_with("\"\"\"") {
                    3 + scan_raw_string(&rest[i + 3..])?
                } else {
                    1 + scan_string_body(&rest[i + 1..])?
                };
                continue;
            }
            b'\'' => {
                // Character literal such as '"' or '\''
                let mut j = i + 1;
                if bytes.get(j) == Some(&b'\\') {
                    j += 2;
                } else {
                    j += rest[j..].chars().next().map_or(1, char::len_utf8);
                }
                while j < bytes.len() && bytes[j] != b'\'' && bytes[j] != b'\n' {
                    j += 1;
                }
                i = j + 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Scan a regular string body starting right after the opening `"`.
/// Returns the byte length up to and including the closing quote.
fn scan_string_body(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if matches!(bytes.get(i + 1), None | Some(b'\n' | b'\r')) {
                    return None;
                }
                i += 2;
                continue;
            }
            b'"' => return Some(i + 1),
            b'\n' | b'\r' => return None,
            b'$' if bytes.get(i + 1) == Some(&b'{') => {
                i += 2 + scan_template_expression(&rest[i + 2..])?;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn lex_raw_string(lex: &mut LogosLexer<LogosToken>) -> bool {
    match scan_raw_string(lex.remainder()) {
        Some(len) => {
            lex.bump(len);
            true
        }
        None => false,
    }
}

fn lex_multi_dollar_string(lex: &mut LogosLexer<LogosToken>) -> bool {
    let rest = lex.remainder();
    if let Some(after_raw) = rest.strip_prefix("\"\"") {
        // `$$"""..."""`
        return match scan_raw_string(after_raw) {
            Some(len) => {
                lex.bump(2 + len);
                true
            }
            None => false,
        };
    }
    // Plain string body: no templates are active, only escapes end it.
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => {
                lex.bump(i + 1);
                return true;
            }
            b'\n' | b'\r' => return false,
            _ => i += 1,
        }
    }
    false
}

fn lex_string(lex: &mut LogosLexer<LogosToken>) -> bool {
    match scan_string_body(lex.remainder()) {
        Some(len) => {
            lex.bump(len);
            true
        }
        None => false,
    }
}

/// Internal logos token enum for pattern matching.
///
/// This enum is private to the lexer module and is used by the logos library
/// for efficient pattern matching. Each variant maps to a corresponding
/// `SyntaxKind` variant via the `map_token` function.
///
/// Note: Whitespace is NOT skipped here - we manually handle it to preserve
/// it for rowan's lossless CST representation.
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum LogosToken {
    // ========== Trivia (Whitespace and Comments) ==========
    /// Whitespace: spaces, tabs, newlines
    #[regex(r"[ \t\n\r\f]+")]
    Whitespace,

    /// Single-line comment (`//...`)
    #[regex(r"//[^\r\n]*")]
    LineComment,

    /// Block comment (`/* ... */`)
    /// Uses callback to handle edge cases like `/* test **/` correctly.
    #[token("/*", lex_block_comment)]
    BlockComment,

    // ========== Keywords ==========
    #[token("fun")]
    Fun,
    #[token("val")]
    Val,
    #[token("var")]
    Var,
    #[token("class")]
    Class,
    #[token("enum")]
    Enum,
    #[token("annotation")]
    Annotation,
    #[token("object")]
    Object,
    #[token("interface")]
    Interface,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("when")]
    When,
    #[token("for")]
    For,
    #[token("while")]
    While,
    #[token("return")]
    Return,
    #[token("package")]
    Package,
    #[token("import")]
    Import,
    #[token("as")]
    As,
    #[token("is")]
    Is,
    #[token("in")]
    In,
    #[token("out")]
    Out,
    #[token("override")]
    Override,
    #[token("open")]
    Open,
    #[token("abstract")]
    Abstract,
    #[token("data")]
    Data,
    #[token("sealed")]
    Sealed,
    #[token("internal")]
    Internal,
    #[token("private")]
    Private,
    #[token("public")]
    Public,
    #[token("protected")]
    Protected,
    #[token("suspend")]
    Suspend,
    #[token("inline")]
    Inline,
    #[token("noinline")]
    Noinline,
    #[token("crossinline")]
    Crossinline,
    #[token("vararg")]
    Vararg,
    #[token("operator")]
    Operator,
    #[token("infix")]
    Infix,
    #[token("companion")]
    Companion,
    #[token("init")]
    Init,
    #[token("constructor")]
    Constructor,
    #[token("this")]
    This,
    #[token("super")]
    Super,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("null")]
    Null,
    #[token("try")]
    Try,
    #[token("catch")]
    Catch,
    #[token("finally")]
    Finally,
    #[token("throw")]
    Throw,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("lateinit")]
    Lateinit,
    #[token("by")]
    By,
    #[token("typealias")]
    Typealias,
    #[token("expect")]
    Expect,
    #[token("actual")]
    Actual,
    #[token("do")]
    Do,
    #[token("where")]
    Where,
    #[token("reified")]
    Reified,

    // ========== Multi-character Operators (must come before single-char) ==========
    #[token("===")]
    EqEqEq,
    #[token("!==")]
    BangEqEq,
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token("<=")]
    LtEq,
    #[token(">=")]
    GtEq,
    #[token("&&")]
    AmpAmp,
    #[token("||")]
    PipePipe,
    #[token("->")]
    Arrow,
    #[token("=>")]
    DoubleArrow,
    #[token("?:")]
    Elvis,
    #[token("?.")]
    SafeCall,
    #[token("!!")]
    NotNull,
    #[token("..")]
    Range,
    #[token("..<")]
    RangeUntil,
    #[token("++")]
    PlusPlus,
    #[token("--")]
    MinusMinus,
    #[token("+=")]
    PlusEq,
    #[token("-=")]
    MinusEq,
    #[token("*=")]
    StarEq,
    #[token("/=")]
    SlashEq,
    #[token("%=")]
    PercentEq,
    #[token("::")]
    ColonColon,
    #[token("!in")]
    NotIn,
    #[token("!is")]
    NotIs,
    #[token("as?")]
    SafeCast,

    // ========== Single-character Operators ==========
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("=")]
    Eq,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("!")]
    Bang,
    #[token("&")]
    Amp,
    #[token("|")]
    Pipe,
    #[token("^")]
    Caret,
    #[token("~")]
    Tilde,

    // ========== Delimiters ==========
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(";")]
    Semicolon,
    #[token(":")]
    Colon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("?")]
    Question,
    #[token("@")]
    At,

    // ========== Literals ==========
    /// Raw string literal (triple-quoted). Ends at the last quote of a run of
    /// three or more, so `""" "x" """"` keeps its trailing quote.
    #[token("\"\"\"", lex_raw_string)]
    RawStringLiteral,

    /// Multi-dollar interpolation string (Kotlin 2.2): `$$"text ${not a template}"`.
    /// Only `$$${..}` interpolates, so the body is scanned without templates.
    #[regex(r#"\$+""#, lex_multi_dollar_string)]
    MultiDollarStringLiteral,

    /// Regular string literal, including `${...}` templates that themselves
    /// contain strings: `"${items.joinToString(", ")}"`.
    #[token("\"", lex_string)]
    StringLiteral,

    /// Character literal
    #[regex(r"'([^'\\]|\\u[0-9a-fA-F]{4}|\\[^\r\n])'")]
    CharLiteral,

    /// Fraction without a leading digit: `.5f`, `.25`
    #[regex(r"\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?[fF]", priority = 3)]
    LeadingDotFloatLiteral,

    #[regex(r"\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?", priority = 2)]
    LeadingDotDoubleLiteral,

    /// Long literal (ends with L)
    #[regex(r"[0-9][0-9_]*L")]
    #[regex(r"0[xX][0-9a-fA-F][0-9a-fA-F_]*L")]
    #[regex(r"0[bB][01][01_]*L")]
    LongLiteral,

    /// Float literal (ends with f or F)
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*[fF]")]
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*[fF]")]
    #[regex(r"[0-9][0-9_]*[fF]")]
    #[regex(r"[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*[fF]")]
    FloatLiteral,

    /// Double literal
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*")]
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*")]
    #[regex(r"[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*")]
    DoubleLiteral,

    /// Integer literal (decimal, hex, binary)
    #[regex(r"[0-9][0-9_]*")]
    #[regex(r"0[xX][0-9a-fA-F][0-9a-fA-F_]*")]
    #[regex(r"0[bB][01][01_]*")]
    IntLiteral,

    // ========== Identifiers ==========
    /// Backtick-quoted identifier (allows reserved words as identifiers)
    #[regex(r"`[^`\r\n]+`")]
    BacktickIdent,

    /// Regular identifier (must come after keywords to avoid shadowing)
    #[regex(r"[a-zA-Z_\u{80}-\u{10FFFF}][a-zA-Z0-9_\u{80}-\u{10FFFF}]*")]
    Ident,

    Error,
}

/// Map a LogosToken to its corresponding SyntaxKind
#[allow(clippy::too_many_lines)] // Simple lookup table
const fn map_token(token: LogosToken) -> SyntaxKind {
    match token {
        LogosToken::Whitespace => SyntaxKind::WHITESPACE,
        LogosToken::LineComment => SyntaxKind::LINE_COMMENT,
        LogosToken::BlockComment => SyntaxKind::BLOCK_COMMENT,

        // Keywords
        LogosToken::Fun => SyntaxKind::FUN_KW,
        LogosToken::Val => SyntaxKind::VAL_KW,
        LogosToken::Var => SyntaxKind::VAR_KW,
        LogosToken::Class => SyntaxKind::CLASS_KW,
        LogosToken::Enum => SyntaxKind::ENUM_KW,
        LogosToken::Annotation => SyntaxKind::ANNOTATION_KW,
        LogosToken::Object => SyntaxKind::OBJECT_KW,
        LogosToken::Interface => SyntaxKind::INTERFACE_KW,
        LogosToken::If => SyntaxKind::IF_KW,
        LogosToken::Else => SyntaxKind::ELSE_KW,
        LogosToken::When => SyntaxKind::WHEN_KW,
        LogosToken::For => SyntaxKind::FOR_KW,
        LogosToken::While => SyntaxKind::WHILE_KW,
        LogosToken::Return => SyntaxKind::RETURN_KW,
        LogosToken::Package => SyntaxKind::PACKAGE_KW,
        LogosToken::Import => SyntaxKind::IMPORT_KW,
        LogosToken::As => SyntaxKind::AS_KW,
        LogosToken::Is => SyntaxKind::IS_KW,
        LogosToken::In => SyntaxKind::IN_KW,
        LogosToken::Out => SyntaxKind::OUT_KW,
        LogosToken::Override => SyntaxKind::OVERRIDE_KW,
        LogosToken::Open => SyntaxKind::OPEN_KW,
        LogosToken::Abstract => SyntaxKind::ABSTRACT_KW,
        LogosToken::Data => SyntaxKind::DATA_KW,
        LogosToken::Sealed => SyntaxKind::SEALED_KW,
        LogosToken::Internal => SyntaxKind::INTERNAL_KW,
        LogosToken::Private => SyntaxKind::PRIVATE_KW,
        LogosToken::Public => SyntaxKind::PUBLIC_KW,
        LogosToken::Protected => SyntaxKind::PROTECTED_KW,
        LogosToken::Suspend => SyntaxKind::SUSPEND_KW,
        LogosToken::Inline => SyntaxKind::INLINE_KW,
        LogosToken::Noinline => SyntaxKind::NOINLINE_KW,
        LogosToken::Crossinline => SyntaxKind::CROSSINLINE_KW,
        LogosToken::Vararg => SyntaxKind::VARARG_KW,
        LogosToken::Operator => SyntaxKind::OPERATOR_KW,
        LogosToken::Infix => SyntaxKind::INFIX_KW,
        LogosToken::Companion => SyntaxKind::COMPANION_KW,
        LogosToken::Init => SyntaxKind::INIT_KW,
        LogosToken::Constructor => SyntaxKind::CONSTRUCTOR_KW,
        LogosToken::This => SyntaxKind::THIS_KW,
        LogosToken::Super => SyntaxKind::SUPER_KW,
        LogosToken::True => SyntaxKind::TRUE_KW,
        LogosToken::False => SyntaxKind::FALSE_KW,
        LogosToken::Null => SyntaxKind::NULL_KW,
        LogosToken::Try => SyntaxKind::TRY_KW,
        LogosToken::Catch => SyntaxKind::CATCH_KW,
        LogosToken::Finally => SyntaxKind::FINALLY_KW,
        LogosToken::Throw => SyntaxKind::THROW_KW,
        LogosToken::Break => SyntaxKind::BREAK_KW,
        LogosToken::Continue => SyntaxKind::CONTINUE_KW,
        LogosToken::Lateinit => SyntaxKind::LATEINIT_KW,
        LogosToken::By => SyntaxKind::BY_KW,
        LogosToken::Typealias => SyntaxKind::TYPEALIAS_KW,
        LogosToken::Expect => SyntaxKind::EXPECT_KW,
        LogosToken::Actual => SyntaxKind::ACTUAL_KW,
        LogosToken::Do => SyntaxKind::DO_KW,
        LogosToken::Where => SyntaxKind::WHERE_KW,
        LogosToken::Reified => SyntaxKind::REIFIED_KW,

        // Operators
        LogosToken::Plus => SyntaxKind::PLUS,
        LogosToken::Minus => SyntaxKind::MINUS,
        LogosToken::Star => SyntaxKind::STAR,
        LogosToken::Slash => SyntaxKind::SLASH,
        LogosToken::Percent => SyntaxKind::PERCENT,
        LogosToken::Eq => SyntaxKind::EQ,
        LogosToken::EqEqEq => SyntaxKind::EQEQEQ,
        LogosToken::BangEqEq => SyntaxKind::BANGEQEQ,
        LogosToken::EqEq => SyntaxKind::EQEQ,
        LogosToken::BangEq => SyntaxKind::BANGEQ,
        LogosToken::Lt => SyntaxKind::LT,
        LogosToken::Gt => SyntaxKind::GT,
        LogosToken::LtEq => SyntaxKind::LTEQ,
        LogosToken::GtEq => SyntaxKind::GTEQ,
        LogosToken::AmpAmp => SyntaxKind::AMPAMP,
        LogosToken::PipePipe => SyntaxKind::PIPEPIPE,
        LogosToken::Bang => SyntaxKind::BANG,
        LogosToken::Arrow => SyntaxKind::ARROW,
        LogosToken::DoubleArrow => SyntaxKind::DOUBLE_ARROW,
        LogosToken::Elvis => SyntaxKind::ELVIS,
        LogosToken::SafeCall => SyntaxKind::SAFE_CALL,
        LogosToken::NotNull => SyntaxKind::NOT_NULL,
        LogosToken::Range => SyntaxKind::RANGE,
        LogosToken::RangeUntil => SyntaxKind::RANGE_UNTIL,
        LogosToken::PlusPlus => SyntaxKind::PLUSPLUS,
        LogosToken::PlusEq => SyntaxKind::PLUSEQ,
        LogosToken::MinusEq => SyntaxKind::MINUSEQ,
        LogosToken::StarEq => SyntaxKind::STAREQ,
        LogosToken::SlashEq => SyntaxKind::SLASHEQ,
        LogosToken::PercentEq => SyntaxKind::PERCENTEQ,
        LogosToken::MinusMinus => SyntaxKind::MINUSMINUS,
        LogosToken::Amp => SyntaxKind::AMP,
        LogosToken::Pipe => SyntaxKind::PIPE,
        LogosToken::Caret => SyntaxKind::CARET,
        LogosToken::Tilde => SyntaxKind::TILDE,
        LogosToken::ColonColon => SyntaxKind::COLONCOLON,
        LogosToken::NotIn => SyntaxKind::NOT_IN,
        LogosToken::NotIs => SyntaxKind::NOT_IS,
        LogosToken::SafeCast => SyntaxKind::SAFE_CAST,

        // Delimiters
        LogosToken::LParen => SyntaxKind::L_PAREN,
        LogosToken::RParen => SyntaxKind::R_PAREN,
        LogosToken::LBrace => SyntaxKind::L_BRACE,
        LogosToken::RBrace => SyntaxKind::R_BRACE,
        LogosToken::LBracket => SyntaxKind::L_BRACKET,
        LogosToken::RBracket => SyntaxKind::R_BRACKET,
        LogosToken::Semicolon => SyntaxKind::SEMICOLON,
        LogosToken::Colon => SyntaxKind::COLON,
        LogosToken::Comma => SyntaxKind::COMMA,
        LogosToken::Dot => SyntaxKind::DOT,
        LogosToken::Question => SyntaxKind::QUESTION,
        LogosToken::At => SyntaxKind::AT,

        // Literals
        LogosToken::IntLiteral => SyntaxKind::INT_LITERAL,
        LogosToken::LongLiteral => SyntaxKind::LONG_LITERAL,
        LogosToken::FloatLiteral => SyntaxKind::FLOAT_LITERAL,
        LogosToken::DoubleLiteral => SyntaxKind::DOUBLE_LITERAL,
        LogosToken::StringLiteral => SyntaxKind::STRING_LITERAL,
        LogosToken::CharLiteral => SyntaxKind::CHAR_LITERAL,
        LogosToken::MultiDollarStringLiteral => SyntaxKind::STRING_LITERAL,
        LogosToken::LeadingDotFloatLiteral => SyntaxKind::FLOAT_LITERAL,
        LogosToken::LeadingDotDoubleLiteral => SyntaxKind::DOUBLE_LITERAL,
        LogosToken::RawStringLiteral => SyntaxKind::RAW_STRING_LITERAL,

        // Identifiers
        LogosToken::Ident => SyntaxKind::IDENT,
        LogosToken::BacktickIdent => SyntaxKind::BACKTICK_IDENT,

        LogosToken::Error => SyntaxKind::ERROR,
    }
}

/// Kotlin lexer that produces rowan-compatible tokens
///
/// This lexer wraps the logos lexer and yields tuples of:
/// - `SyntaxKind`: The token type
/// - `&str`: The token text (zero-copy slice into source)
/// - `TextRange`: The token's position in the source
///
/// # Example
///
/// ```
/// use hyperion_kotlin::lexer::Lexer;
/// use hyperion_kotlin::syntax::SyntaxKind;
///
/// let source = "fun main() { }";
/// let lexer = Lexer::new(source);
///
/// for (kind, text, range) in lexer {
///     println!("{:?} at {:?}: {}", kind, range, text);
/// }
/// ```
pub struct Lexer<'source> {
    /// Original source code
    source: &'source str,
    /// Byte offset of `inner`'s input within `source`
    offset: usize,
    /// Inner logos lexer with span information
    inner: SpannedIter<'source, LogosToken>,
}

impl<'source> Lexer<'source> {
    /// Create a new lexer for the given source code
    pub fn new(source: &'source str) -> Self {
        Self {
            source,
            offset: 0,
            inner: LogosToken::lexer(source).spanned(),
        }
    }
}

impl<'source> Iterator for Lexer<'source> {
    type Item = (SyntaxKind, &'source str, TextRange);

    fn next(&mut self) -> Option<Self::Item> {
        let (token, span) = self.inner.next()?;
        let mut kind = map_token(token.unwrap_or(LogosToken::Error));
        let mut span = span.start + self.offset..span.end + self.offset;

        // `!is` / `!in` are operators only as whole words: `!isWriter` and
        // `!index` are `!` applied to an identifier. Logos cannot look ahead,
        // so split here and restart lexing right after the `!`.
        if matches!(kind, SyntaxKind::NOT_IS | SyntaxKind::NOT_IN)
            && self.source[span.end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            kind = SyntaxKind::BANG;
            span = span.start..span.start + 1;
            self.offset = span.end;
            self.inner = LogosToken::lexer(&self.source[self.offset..]).spanned();
        }

        let text = &self.source[span.clone()];
        let range = TextRange::new(
            TextSize::from(u32::try_from(span.start).expect("span.start exceeds u32::MAX")),
            TextSize::from(u32::try_from(span.end).expect("span.end exceeds u32::MAX")),
        );
        Some((kind, text, range))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords() {
        let source = "fun val var class object";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert_eq!(tokens[0].0, SyntaxKind::FUN_KW);
        assert_eq!(tokens[2].0, SyntaxKind::VAL_KW);
        assert_eq!(tokens[4].0, SyntaxKind::VAR_KW);
        assert_eq!(tokens[6].0, SyntaxKind::CLASS_KW);
        assert_eq!(tokens[8].0, SyntaxKind::OBJECT_KW);
    }

    #[test]
    fn test_operators() {
        let source = "-> ?: ?. !! ..";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert_eq!(tokens[0].0, SyntaxKind::ARROW);
        assert_eq!(tokens[2].0, SyntaxKind::ELVIS);
        assert_eq!(tokens[4].0, SyntaxKind::SAFE_CALL);
        assert_eq!(tokens[6].0, SyntaxKind::NOT_NULL);
        assert_eq!(tokens[8].0, SyntaxKind::RANGE);
    }

    #[test]
    fn test_literals() {
        let source = r#"42 42L 3.14 3.14f "hello" 'a' """raw""""#;
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[1].0, SyntaxKind::LONG_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::DOUBLE_LITERAL);
        assert_eq!(tokens[3].0, SyntaxKind::FLOAT_LITERAL);
        assert_eq!(tokens[4].0, SyntaxKind::STRING_LITERAL);
        assert_eq!(tokens[5].0, SyntaxKind::CHAR_LITERAL);
        assert_eq!(tokens[6].0, SyntaxKind::RAW_STRING_LITERAL);
    }

    #[test]
    fn test_simple_function() {
        let source = "fun main() { }";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert!(!tokens.is_empty());
        assert_eq!(tokens[0].0, SyntaxKind::FUN_KW);
        assert_eq!(tokens[0].1, "fun");
    }

    #[test]
    fn test_comments() {
        let source = "// line comment\n/* block comment */";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert_eq!(tokens[0].0, SyntaxKind::LINE_COMMENT);
        assert_eq!(tokens[1].0, SyntaxKind::WHITESPACE);
        assert_eq!(tokens[2].0, SyntaxKind::BLOCK_COMMENT);
    }

    #[test]
    fn test_identifiers() {
        let source = "myVar _underscore `fun` `reserved word`";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::IDENT);
        assert_eq!(tokens[1].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].0, SyntaxKind::BACKTICK_IDENT);
        assert_eq!(tokens[3].0, SyntaxKind::BACKTICK_IDENT);
    }

    #[test]
    fn test_new_keywords() {
        let source = "lateinit by typealias expect actual";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::LATEINIT_KW);
        assert_eq!(tokens[0].1, "lateinit");
        assert_eq!(tokens[1].0, SyntaxKind::BY_KW);
        assert_eq!(tokens[1].1, "by");
        assert_eq!(tokens[2].0, SyntaxKind::TYPEALIAS_KW);
        assert_eq!(tokens[2].1, "typealias");
        assert_eq!(tokens[3].0, SyntaxKind::EXPECT_KW);
        assert_eq!(tokens[3].1, "expect");
        assert_eq!(tokens[4].0, SyntaxKind::ACTUAL_KW);
        assert_eq!(tokens[4].1, "actual");
    }

    #[test]
    fn test_new_operators() {
        let source = "!in !is as?";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::NOT_IN);
        assert_eq!(tokens[0].1, "!in");
        assert_eq!(tokens[1].0, SyntaxKind::NOT_IS);
        assert_eq!(tokens[1].1, "!is");
        assert_eq!(tokens[2].0, SyntaxKind::SAFE_CAST);
        assert_eq!(tokens[2].1, "as?");
    }

    #[test]
    fn test_kotlin_property_delegation() {
        let source = "val name: String by lazy { \"Hello\" }";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::VAL_KW);
        assert_eq!(tokens[1].0, SyntaxKind::IDENT);
        assert_eq!(tokens[1].1, "name");
        assert_eq!(tokens[2].0, SyntaxKind::COLON);
        assert_eq!(tokens[3].0, SyntaxKind::IDENT);
        assert_eq!(tokens[3].1, "String");
        assert_eq!(tokens[4].0, SyntaxKind::BY_KW);
        assert_eq!(tokens[4].1, "by");
        assert_eq!(tokens[5].0, SyntaxKind::IDENT);
        assert_eq!(tokens[5].1, "lazy");
    }

    #[test]
    fn test_kotlin_nullable_checks() {
        let source = "if (x !is String) return";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::IF_KW);
        assert_eq!(tokens[1].0, SyntaxKind::L_PAREN);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
        assert_eq!(tokens[3].0, SyntaxKind::NOT_IS);
        assert_eq!(tokens[3].1, "!is");
        assert_eq!(tokens[4].0, SyntaxKind::IDENT);
        assert_eq!(tokens[5].0, SyntaxKind::R_PAREN);
        assert_eq!(tokens[6].0, SyntaxKind::RETURN_KW);
    }
}
