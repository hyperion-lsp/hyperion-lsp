// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java Lexer Implementation with Rowan Integration
//!
//! This module provides a high-performance lexer for Java source code (versions 8-24)
//! using the `logos` crate for efficient token pattern matching, integrated with
//! the rowan-based CST (Concrete Syntax Tree) system.
//!
//! # Architecture
//!
//! The lexer uses a two-layer architecture:
//!
//! 1. **Internal Layer (LogosToken)**: Private enum used by logos for pattern matching.
//!    This enum is optimized for lexer performance and matches Java source patterns.
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
//! - Support for all Java keywords (including contextual keywords)
//! - Complete operator and delimiter support
//! - String literals (including text blocks from Java 15+)
//! - Numeric literals (binary, octal, decimal, hexadecimal with underscores)
//! - Comments (line and block, including Javadoc)
//! - Unicode escape sequences
//! - Error recovery for invalid tokens
//! - Rowan-compatible iterator-based API
//!
//! # Example
//!
//! ```
//! use hyperion_parser::lexer::Lexer;
//! use hyperion_parser::syntax::SyntaxKind;
//!
//! let source = r#"
//!     public class HelloWorld {
//!         public static void main(String[] args) {
//!             System.out.println("Hello, World!");
//!         }
//!     }
//! "#;
//!
//! let lexer = Lexer::new(source);
//! for (kind, text, range) in lexer {
//!     println!("{:?} at {:?}: {}", kind, range, text);
//! }
//! ```

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

/// Callback to lex text block literals (Java 15+, `""" ... """`).
///
/// The opening `"""` has already been matched. Per JLS §3.10.6 the closing
/// delimiter is a sequence of three `"`; content may itself contain one or two
/// consecutive `"`, and quotes immediately preceding the closing delimiter are
/// content (so `"""\ntext""""` has content `text"`). Equivalently, the closing
/// delimiter is the first `"""` that is not itself followed by another `"`.
/// Backslash escapes the following character so `\"` does not count toward a
/// closing run. Returns `false` for an unterminated block (mirrors comments).
fn lex_text_block(lex: &mut LogosLexer<LogosToken>) -> bool {
    let bytes = lex.remainder().as_bytes();
    let n = bytes.len();
    let mut i = 0;

    while i < n {
        match bytes[i] {
            b'\\' => {
                // Escape: skip the backslash and the whole following char.
                i += 1;
                if i < n {
                    i += 1;
                    while i < n && (bytes[i] & 0xC0) == 0x80 {
                        i += 1;
                    }
                }
            }
            b'"' => {
                // Count the maximal run of quotes at this position.
                let mut run = 0;
                while i + run < n && bytes[i + run] == b'"' {
                    run += 1;
                }
                if run >= 3 {
                    // Last three quotes of the run are the closing delimiter;
                    // any preceding quotes in the run are content. Consume the
                    // whole run (content quotes + closing delimiter).
                    lex.bump(i + run);
                    return true;
                }
                // One or two quotes are content.
                i += run;
            }
            _ => {
                // Advance one full UTF-8 character.
                i += 1;
                while i < n && (bytes[i] & 0xC0) == 0x80 {
                    i += 1;
                }
            }
        }
    }

    // Unterminated text block - reject the token.
    false
}

/// Callback to lex javadoc comments (`/** ... */`).
/// Handles edge cases like `/** test **/` correctly.
/// Accepts `/**/` as a minimal javadoc comment.
fn lex_javadoc_comment(lex: &mut LogosLexer<LogosToken>) -> bool {
    let remainder = lex.remainder();
    let mut chars = remainder.chars().peekable();

    // Check for `/**/` case - accept it as javadoc (empty doc comment)
    // We've matched `/**`, if next char is `/`, accept immediately
    if chars.peek() == Some(&'/') {
        lex.bump(1); // Just the `/`
        return true;
    }

    let mut len = 0;

    // Already matched `/**`, scan until `*/`
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
    /// Whitespace: spaces, tabs, newlines, form feeds
    #[regex(r"[ \t\n\r\f]+")]
    Whitespace,

    /// Single-line comment (`//...`)
    #[regex(r"//[^\r\n]*")]
    LineComment,

    /// Block comment (`/* ... */`)
    /// Uses callback to handle edge cases like `/* test **/` correctly.
    #[token("/*", lex_block_comment)]
    BlockComment,

    /// Javadoc comment (`/** ... */`)
    /// Uses callback to handle edge cases like `/** test **/` correctly.
    /// Priority 2 ensures `/**` is matched before `/*` for javadoc.
    #[token("/**", lex_javadoc_comment, priority = 2)]
    JavadocComment,

    // ========== Keywords ==========
    #[token("abstract")]
    Abstract,
    #[token("assert")]
    Assert,
    #[token("boolean")]
    Boolean,
    #[token("break")]
    Break,
    #[token("byte")]
    Byte,
    #[token("case")]
    Case,
    #[token("catch")]
    Catch,
    #[token("char")]
    Char,
    #[token("class")]
    Class,
    #[token("const")]
    Const,
    #[token("continue")]
    Continue,
    #[token("default")]
    Default,
    #[token("do")]
    Do,
    #[token("double")]
    Double,
    #[token("else")]
    Else,
    #[token("enum")]
    Enum,
    #[token("extends")]
    Extends,
    #[token("final")]
    Final,
    #[token("finally")]
    Finally,
    #[token("float")]
    Float,
    #[token("for")]
    For,
    #[token("goto")]
    Goto,
    #[token("if")]
    If,
    #[token("implements")]
    Implements,
    #[token("import")]
    Import,
    #[token("instanceof")]
    Instanceof,
    #[token("int")]
    Int,
    #[token("interface")]
    Interface,
    #[token("long")]
    Long,
    #[token("native")]
    Native,
    #[token("new")]
    New,
    #[token("package")]
    Package,
    #[token("private")]
    Private,
    #[token("protected")]
    Protected,
    #[token("public")]
    Public,
    #[token("return")]
    Return,
    #[token("short")]
    Short,
    #[token("static")]
    Static,
    #[token("strictfp")]
    Strictfp,
    #[token("super")]
    Super,
    #[token("switch")]
    Switch,
    #[token("synchronized")]
    Synchronized,
    #[token("this")]
    This,
    #[token("throw")]
    Throw,
    #[token("throws")]
    Throws,
    #[token("transient")]
    Transient,
    #[token("try")]
    Try,
    #[token("void")]
    Void,
    #[token("volatile")]
    Volatile,
    #[token("while")]
    While,

    // ========== Contextual Keywords (Java 9+) ==========
    #[token("exports")]
    Exports,
    #[token("module")]
    Module,
    #[token("open")]
    Open,
    #[token("opens")]
    Opens,
    #[token("permits")]
    Permits,
    #[token("provides")]
    Provides,
    #[token("record")]
    Record,
    #[token("requires")]
    Requires,
    #[token("sealed")]
    Sealed,
    #[token("to")]
    To,
    #[token("transitive")]
    Transitive,
    #[token("uses")]
    Uses,
    #[token("var")]
    Var,
    #[token("when")]
    When,
    #[token("with")]
    With,
    #[token("yield")]
    Yield,

    // ========== Literals ==========
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("null")]
    Null,

    /// Integer literal (decimal, hex, octal, binary)
    /// Supports underscores for readability (Java 7+)
    #[regex(r"[0-9][0-9_]*[lL]?")]
    #[regex(r"0[xX][0-9a-fA-F][0-9a-fA-F_]*[lL]?")]
    #[regex(r"0[bB][01][01_]*[lL]?")]
    #[regex(r"0[0-7][0-7_]*[lL]?")]
    IntegerLiteral,

    /// Floating-point literal
    /// Supports underscores, exponents, and hex floats (Java 17+)
    #[regex(r"[0-9][0-9_]*\.[0-9_]*([eE][+-]?[0-9][0-9_]*)?[fFdD]?")]
    #[regex(r"\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?[fFdD]?")]
    #[regex(r"[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*[fFdD]?")]
    #[regex(r"[0-9][0-9_]*[fFdD]")]
    #[regex(r"0[xX][0-9a-fA-F][0-9a-fA-F_]*\.[0-9a-fA-F_]*[pP][+-]?[0-9][0-9_]*[fFdD]?")]
    #[regex(r"0[xX][0-9a-fA-F][0-9a-fA-F_]*[pP][+-]?[0-9][0-9_]*[fFdD]?")]
    FloatingPointLiteral,

    /// Character literal: one UTF-16 code unit, with no raw line terminator. A
    /// three-digit octal escape must start with 0-3 (JLS §3.10.4, §3.10.7).
    #[regex(r#"'([^'\\\r\n\u{10000}-\u{10FFFF}]|\\[btnfrs'"\\]|\\[0-7]{1,2}|\\[0-3][0-7]{2}|\\u[0-9a-fA-F]{4})'"#)]
    CharacterLiteral,

    /// Text block literal (Java 13+, preview in 13-14, standard in 15+)
    /// MUST be before `StringLiteral` so """ is recognized as text block, not start of string.
    /// Uses a callback so content quotes adjacent to the closing delimiter
    /// (e.g. `"""\ntext""""` → content `text"`) are lexed correctly.
    #[token("\"\"\"", lex_text_block, priority = 3)]
    TextBlockLiteral,

    /// String literal, including the Java 15 `\s` space escape. Raw CR/LF are
    /// only valid inside text blocks, not ordinary strings (JLS §3.10.5).
    #[regex(r#""([^"\\\r\n]|\\[btnfrs'"\\]|\\[0-7]{1,2}|\\[0-3][0-7]{2}|\\u[0-9a-fA-F]{4})*""#)]
    StringLiteral,

    // ========== Identifiers ==========
    /// Java identifier (supports Unicode as per JLS)
    /// Start: Unicode letter, underscore, or dollar sign
    /// Continue: Unicode letter or digit, underscore, or dollar sign
    #[regex(r"[\p{L}_$][\p{L}\p{N}_$]*")]
    Identifier,

    // ========== Operators ==========

    // Multi-character operators (order matters for longest match)
    #[token(">>>=")]
    UnsignedRightShiftAssign,
    #[token(">>>")]
    UnsignedRightShift,
    #[token("<<=")]
    LeftShiftAssign,
    #[token(">>=")]
    RightShiftAssign,
    #[token("<<")]
    LeftShift,
    #[token(">>")]
    RightShift,
    #[token("==")]
    Equal,
    #[token("!=")]
    NotEqual,
    #[token("<=")]
    LessEqual,
    #[token(">=")]
    GreaterEqual,
    #[token("&&")]
    And,
    #[token("||")]
    Or,
    #[token("++")]
    Increment,
    #[token("--")]
    Decrement,
    #[token("+=")]
    PlusAssign,
    #[token("-=")]
    MinusAssign,
    #[token("*=")]
    MultiplyAssign,
    #[token("/=")]
    DivideAssign,
    #[token("&=")]
    BitwiseAndAssign,
    #[token("|=")]
    BitwiseOrAssign,
    #[token("^=")]
    BitwiseXorAssign,
    #[token("%=")]
    ModuloAssign,
    #[token("->")]
    Arrow,
    #[token("::")]
    DoubleColon,
    #[token("...")]
    Ellipsis,

    // Single-character operators
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Multiply,
    #[token("/")]
    Divide,
    #[token("%")]
    Modulo,
    #[token("&")]
    BitwiseAnd,
    #[token("|")]
    BitwiseOr,
    #[token("^")]
    BitwiseXor,
    #[token("~")]
    BitwiseComplement,
    #[token("!")]
    Not,
    #[token("=")]
    Assign,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,
    #[token("?")]
    Question,
    #[token(":")]
    Colon,

    // ========== Punctuation/Delimiters ==========
    #[token("(")]
    LeftParen,
    #[token(")")]
    RightParen,
    #[token("{")]
    LeftBrace,
    #[token("}")]
    RightBrace,
    #[token("[")]
    LeftBracket,
    #[token("]")]
    RightBracket,
    #[token(";")]
    Semicolon,
    #[token(",")]
    Comma,
    #[token(".")]
    Dot,
    #[token("@")]
    At,

    // ========== Error Recovery ==========
    /// Invalid or unrecognized token
    #[regex(r".", priority = 0)]
    Error,
}

/// Maps a logos token to its corresponding `SyntaxKind`.
///
/// This function provides the bridge between the internal logos token representation
/// and the rowan-compatible `SyntaxKind` enum. Every `LogosToken` has a corresponding
/// `SyntaxKind` variant.
///
/// # Architecture Note
///
/// This mapping is the core of the lexer's integration with rowan:
/// - `LogosToken`: Optimized for pattern matching performance
/// - `SyntaxKind`: Optimized for CST representation and semantic queries
///
/// The mapping is exhaustive and must be kept in sync with both enums.
const fn map_token(token: LogosToken) -> SyntaxKind {
    match token {
        // Trivia
        LogosToken::Whitespace => SyntaxKind::WHITESPACE,
        LogosToken::LineComment => SyntaxKind::LINE_COMMENT,
        LogosToken::BlockComment => SyntaxKind::BLOCK_COMMENT,
        LogosToken::JavadocComment => SyntaxKind::DOC_COMMENT,

        // Keywords
        LogosToken::Abstract => SyntaxKind::ABSTRACT_KW,
        LogosToken::Assert => SyntaxKind::ASSERT_KW,
        LogosToken::Boolean => SyntaxKind::BOOLEAN_KW,
        LogosToken::Break => SyntaxKind::BREAK_KW,
        LogosToken::Byte => SyntaxKind::BYTE_KW,
        LogosToken::Case => SyntaxKind::CASE_KW,
        LogosToken::Catch => SyntaxKind::CATCH_KW,
        LogosToken::Char => SyntaxKind::CHAR_KW,
        LogosToken::Class => SyntaxKind::CLASS_KW,
        LogosToken::Const => SyntaxKind::CONST_KW,
        LogosToken::Continue => SyntaxKind::CONTINUE_KW,
        LogosToken::Default => SyntaxKind::DEFAULT_KW,
        LogosToken::Do => SyntaxKind::DO_KW,
        LogosToken::Double => SyntaxKind::DOUBLE_KW,
        LogosToken::Else => SyntaxKind::ELSE_KW,
        LogosToken::Enum => SyntaxKind::ENUM_KW,
        LogosToken::Extends => SyntaxKind::EXTENDS_KW,
        LogosToken::Final => SyntaxKind::FINAL_KW,
        LogosToken::Finally => SyntaxKind::FINALLY_KW,
        LogosToken::Float => SyntaxKind::FLOAT_KW,
        LogosToken::For => SyntaxKind::FOR_KW,
        LogosToken::Goto => SyntaxKind::GOTO_KW,
        LogosToken::If => SyntaxKind::IF_KW,
        LogosToken::Implements => SyntaxKind::IMPLEMENTS_KW,
        LogosToken::Import => SyntaxKind::IMPORT_KW,
        LogosToken::Instanceof => SyntaxKind::INSTANCEOF_KW,
        LogosToken::Int => SyntaxKind::INT_KW,
        LogosToken::Interface => SyntaxKind::INTERFACE_KW,
        LogosToken::Long => SyntaxKind::LONG_KW,
        LogosToken::Native => SyntaxKind::NATIVE_KW,
        LogosToken::New => SyntaxKind::NEW_KW,
        LogosToken::Package => SyntaxKind::PACKAGE_KW,
        LogosToken::Private => SyntaxKind::PRIVATE_KW,
        LogosToken::Protected => SyntaxKind::PROTECTED_KW,
        LogosToken::Public => SyntaxKind::PUBLIC_KW,
        LogosToken::Return => SyntaxKind::RETURN_KW,
        LogosToken::Short => SyntaxKind::SHORT_KW,
        LogosToken::Static => SyntaxKind::STATIC_KW,
        LogosToken::Strictfp => SyntaxKind::STRICTFP_KW,
        LogosToken::Super => SyntaxKind::SUPER_KW,
        LogosToken::Switch => SyntaxKind::SWITCH_KW,
        LogosToken::Synchronized => SyntaxKind::SYNCHRONIZED_KW,
        LogosToken::This => SyntaxKind::THIS_KW,
        LogosToken::Throw => SyntaxKind::THROW_KW,
        LogosToken::Throws => SyntaxKind::THROWS_KW,
        LogosToken::Transient => SyntaxKind::TRANSIENT_KW,
        LogosToken::Try => SyntaxKind::TRY_KW,
        LogosToken::Void => SyntaxKind::VOID_KW,
        LogosToken::Volatile => SyntaxKind::VOLATILE_KW,
        LogosToken::While => SyntaxKind::WHILE_KW,

        // Contextual Keywords (Java 9+)
        LogosToken::Exports => SyntaxKind::EXPORTS_KW,
        LogosToken::Module => SyntaxKind::MODULE_KW,
        LogosToken::Open => SyntaxKind::OPEN_KW,
        LogosToken::Opens => SyntaxKind::OPENS_KW,
        LogosToken::Permits => SyntaxKind::PERMITS_KW,
        LogosToken::Provides => SyntaxKind::PROVIDES_KW,
        LogosToken::Record => SyntaxKind::RECORD_KW,
        LogosToken::Requires => SyntaxKind::REQUIRES_KW,
        LogosToken::Sealed => SyntaxKind::SEALED_KW,
        LogosToken::To => SyntaxKind::TO_KW,
        LogosToken::Transitive => SyntaxKind::TRANSITIVE_KW,
        LogosToken::Uses => SyntaxKind::USES_KW,
        LogosToken::Var => SyntaxKind::VAR_KW,
        LogosToken::When => SyntaxKind::WHEN_KW,
        LogosToken::With => SyntaxKind::WITH_KW,
        LogosToken::Yield => SyntaxKind::YIELD_KW,

        // Literals - map true/false/null to BOOLEAN_LITERAL/NULL_LITERAL
        LogosToken::True | LogosToken::False => SyntaxKind::BOOLEAN_LITERAL,
        LogosToken::Null => SyntaxKind::NULL_LITERAL,
        LogosToken::IntegerLiteral => {
            // Check if it ends with 'L' or 'l' for long literal
            // For now, we treat all as INT_LITERAL or LONG_LITERAL
            // The actual determination would need the text, so we default to INT_LITERAL
            // The parser can refine this based on the suffix
            SyntaxKind::INT_LITERAL
        }
        LogosToken::FloatingPointLiteral => {
            // Similarly, we'd check for 'f/F' vs 'd/D' suffix
            // Default to DOUBLE_LITERAL for now
            SyntaxKind::DOUBLE_LITERAL
        }
        LogosToken::CharacterLiteral => SyntaxKind::CHAR_LITERAL,
        LogosToken::StringLiteral => SyntaxKind::STRING_LITERAL,
        LogosToken::TextBlockLiteral => SyntaxKind::TEXT_BLOCK_LITERAL,

        // Identifiers
        LogosToken::Identifier => SyntaxKind::IDENT,

        // Operators
        LogosToken::Plus => SyntaxKind::PLUS,
        LogosToken::Minus => SyntaxKind::MINUS,
        LogosToken::Multiply => SyntaxKind::STAR,
        LogosToken::Divide => SyntaxKind::SLASH,
        LogosToken::Modulo => SyntaxKind::PERCENT,
        LogosToken::BitwiseAnd => SyntaxKind::AMP,
        LogosToken::BitwiseOr => SyntaxKind::PIPE,
        LogosToken::BitwiseXor => SyntaxKind::CARET,
        LogosToken::BitwiseComplement => SyntaxKind::TILDE,
        LogosToken::LeftShift => SyntaxKind::SHL,
        LogosToken::RightShift => SyntaxKind::SHR,
        LogosToken::UnsignedRightShift => SyntaxKind::USHR,
        LogosToken::And => SyntaxKind::AMP_AMP,
        LogosToken::Or => SyntaxKind::PIPE_PIPE,
        LogosToken::Not => SyntaxKind::BANG,
        LogosToken::Equal => SyntaxKind::EQ_EQ,
        LogosToken::NotEqual => SyntaxKind::BANG_EQ,
        LogosToken::Less => SyntaxKind::LT,
        LogosToken::Greater => SyntaxKind::GT,
        LogosToken::LessEqual => SyntaxKind::LT_EQ,
        LogosToken::GreaterEqual => SyntaxKind::GT_EQ,
        LogosToken::Assign => SyntaxKind::EQ,
        LogosToken::PlusAssign => SyntaxKind::PLUS_EQ,
        LogosToken::MinusAssign => SyntaxKind::MINUS_EQ,
        LogosToken::MultiplyAssign => SyntaxKind::STAR_EQ,
        LogosToken::DivideAssign => SyntaxKind::SLASH_EQ,
        LogosToken::ModuloAssign => SyntaxKind::PERCENT_EQ,
        LogosToken::BitwiseAndAssign => SyntaxKind::AMP_EQ,
        LogosToken::BitwiseOrAssign => SyntaxKind::PIPE_EQ,
        LogosToken::BitwiseXorAssign => SyntaxKind::CARET_EQ,
        LogosToken::LeftShiftAssign => SyntaxKind::SHL_EQ,
        LogosToken::RightShiftAssign => SyntaxKind::SHR_EQ,
        LogosToken::UnsignedRightShiftAssign => SyntaxKind::USHR_EQ,
        LogosToken::Increment => SyntaxKind::PLUS_PLUS,
        LogosToken::Decrement => SyntaxKind::MINUS_MINUS,
        LogosToken::Question => SyntaxKind::QUESTION,
        LogosToken::Colon => SyntaxKind::COLON,
        LogosToken::Arrow => SyntaxKind::ARROW,
        LogosToken::DoubleColon => SyntaxKind::COLON_COLON,
        LogosToken::At => SyntaxKind::AT,
        LogosToken::Dot => SyntaxKind::DOT,
        LogosToken::Ellipsis => SyntaxKind::ELLIPSIS,

        // Punctuation
        LogosToken::LeftParen => SyntaxKind::L_PAREN,
        LogosToken::RightParen => SyntaxKind::R_PAREN,
        LogosToken::LeftBrace => SyntaxKind::L_BRACE,
        LogosToken::RightBrace => SyntaxKind::R_BRACE,
        LogosToken::LeftBracket => SyntaxKind::L_BRACKET,
        LogosToken::RightBracket => SyntaxKind::R_BRACKET,
        LogosToken::Semicolon => SyntaxKind::SEMICOLON,
        LogosToken::Comma => SyntaxKind::COMMA,

        // Error recovery
        LogosToken::Error => SyntaxKind::ERROR,
    }
}

/// Helper function to refine integer/float literal types based on suffix.
///
/// This examines the text of a numeric literal to determine if it should be
/// `INT_LITERAL` vs `LONG_LITERAL`, or `FLOAT_LITERAL` vs `DOUBLE_LITERAL`.
fn refine_numeric_literal(kind: SyntaxKind, text: &str) -> SyntaxKind {
    match kind {
        SyntaxKind::INT_LITERAL => {
            if text.ends_with('L') || text.ends_with('l') {
                SyntaxKind::LONG_LITERAL
            } else {
                SyntaxKind::INT_LITERAL
            }
        }
        SyntaxKind::DOUBLE_LITERAL => {
            if text.ends_with('f') || text.ends_with('F') {
                SyntaxKind::FLOAT_LITERAL
            } else {
                SyntaxKind::DOUBLE_LITERAL
            }
        }
        _ => kind,
    }
}

/// Public lexer that outputs rowan-compatible tokens.
///
/// This struct wraps the logos lexer and provides an iterator interface
/// that yields `(SyntaxKind, &str, TextRange)` tuples, which can be directly
/// consumed by rowan's green tree builder.
///
/// # Architecture
///
/// ```text
/// Source Code
///      |
///      v
/// LogosToken (internal, pattern matching)
///      |
///      v
/// map_token() function
///      |
///      v
/// SyntaxKind (public, rowan-compatible)
/// ```
///
/// # Example
///
/// ```
/// use hyperion_parser::lexer::Lexer;
/// use hyperion_parser::syntax::SyntaxKind;
///
/// let source = "public class Test { }";
/// let lexer = Lexer::new(source);
///
/// for (kind, text, range) in lexer {
///     println!("{:?} at {:?}: \"{}\"", kind, range, text);
/// }
/// ```
pub struct Lexer<'source> {
    source: &'source str,
    inner: SpannedIter<'source, LogosToken>,
}

impl<'source> Lexer<'source> {
    /// Creates a new lexer for the given source code.
    ///
    /// # Arguments
    ///
    /// * `source` - The Java source code to tokenize
    ///
    /// # Returns
    ///
    /// A new `Lexer` instance that yields `(SyntaxKind, &str, TextRange)` tuples.
    ///
    /// # Example
    ///
    /// ```
    /// use hyperion_parser::lexer::Lexer;
    ///
    /// let source = "int x = 42;";
    /// let lexer = Lexer::new(source);
    /// let tokens: Vec<_> = lexer.collect();
    /// ```
    #[must_use]
    pub fn new(source: &'source str) -> Self {
        Self {
            source,
            inner: LogosToken::lexer(source).spanned(),
        }
    }

    /// Returns the source code being lexed.
    #[must_use]
    pub const fn source(&self) -> &'source str {
        self.source
    }

    /// Collects all tokens into a vector.
    ///
    /// This is useful when you need random access to tokens or want to
    /// process them multiple times.
    ///
    /// # Example
    ///
    /// ```
    /// use hyperion_parser::lexer::Lexer;
    ///
    /// let source = "int x = 42;";
    /// let tokens = Lexer::new(source).collect_all();
    /// // tokens: Vec<(SyntaxKind, &str, TextRange)>
    /// ```
    #[must_use]
    pub fn collect_all(self) -> Vec<(SyntaxKind, &'source str, TextRange)> {
        self.collect()
    }

    /// Filters out trivia tokens (whitespace and comments) from the token stream.
    ///
    /// Returns a new iterator that skips all trivia, yielding only significant tokens.
    ///
    /// # Example
    ///
    /// ```
    /// use hyperion_parser::lexer::Lexer;
    ///
    /// let source = "int x; // comment";
    /// let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();
    /// // tokens will only contain: int, x, ;
    /// ```
    pub fn skip_trivia(
        self,
    ) -> impl Iterator<Item = (SyntaxKind, &'source str, TextRange)> + 'source {
        self.filter(|(kind, _, _)| !kind.is_trivia())
    }

    /// Filters out error tokens from the token stream.
    ///
    /// This can be useful for lenient parsing where you want to ignore
    /// invalid characters and continue processing.
    pub fn skip_errors(
        self,
    ) -> impl Iterator<Item = (SyntaxKind, &'source str, TextRange)> + 'source {
        self.filter(|(kind, _, _)| *kind != SyntaxKind::ERROR)
    }
}

impl<'source> Iterator for Lexer<'source> {
    type Item = (SyntaxKind, &'source str, TextRange);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(result, span)| {
            let logos_token = result.unwrap_or(LogosToken::Error);
            let text = &self.source[span.clone()];

            // Map logos token to SyntaxKind
            let mut kind = map_token(logos_token);

            // Refine numeric literal types based on suffix
            kind = refine_numeric_literal(kind, text);

            // Create TextRange from byte offsets
            let range = TextRange::new(
                TextSize::from(span.start as u32),
                TextSize::from(span.end as u32),
            );

            (kind, text, range)
        })
    }
}

// ===== Legacy Compatibility Layer (Optional) =====

/// Legacy token representation for backwards compatibility.
///
/// This is provided for compatibility with existing code that uses the old API.
/// New code should use the iterator API directly: `Lexer::new(source)`.
///
/// **Deprecated**: Use `(SyntaxKind, &str, TextRange)` directly instead.
#[deprecated(
    since = "0.2.0",
    note = "Use Lexer iterator API directly with (SyntaxKind, &str, TextRange)"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedToken<'source> {
    /// The syntax kind
    pub kind: SyntaxKind,
    /// The source text that was matched
    pub text: &'source str,
    /// The text range in the source
    pub range: TextRange,
}

#[allow(deprecated)]
impl<'source> SpannedToken<'source> {
    /// Creates a new spanned token
    #[must_use]
    pub const fn new(kind: SyntaxKind, text: &'source str, range: TextRange) -> Self {
        Self { kind, text, range }
    }

    /// Returns the line and column position of the token start.
    ///
    /// This is a convenience method for error reporting.
    #[must_use]
    pub fn position(&self, source: &str) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;

        let start: usize = self.range.start().into();
        for (i, c) in source.char_indices() {
            if i >= start {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }

        (line, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rowan_integration() {
        let source = "public class Test { }";
        let tokens: Vec<_> = Lexer::new(source).collect();

        // Should yield (SyntaxKind, &str, TextRange) tuples
        assert_eq!(tokens.len(), 9); // public, whitespace, class, whitespace, Test, whitespace, {, whitespace, }

        // First non-whitespace token should be PUBLIC_KW
        let (kind, text, _range) = &tokens[0];
        assert_eq!(*kind, SyntaxKind::PUBLIC_KW);
        assert_eq!(*text, "public");
    }

    #[test]
    fn test_whitespace_preserved() {
        let source = "int   x";
        let tokens: Vec<_> = Lexer::new(source).collect();

        // Should have: int, whitespace, x
        assert!(tokens.len() >= 3);
        assert_eq!(tokens[0].0, SyntaxKind::INT_KW);
        assert_eq!(tokens[1].0, SyntaxKind::WHITESPACE);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
    }

    #[test]
    fn test_keywords() {
        let source = "public class static void";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].0, SyntaxKind::PUBLIC_KW);
        assert_eq!(tokens[1].0, SyntaxKind::CLASS_KW);
        assert_eq!(tokens[2].0, SyntaxKind::STATIC_KW);
        assert_eq!(tokens[3].0, SyntaxKind::VOID_KW);
    }

    #[test]
    fn test_identifiers() {
        let source = "myVar _test $dollar camelCase";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 4);
        for (kind, _, _) in tokens {
            assert_eq!(kind, SyntaxKind::IDENT);
        }
    }

    #[test]
    fn test_integer_literals() {
        let source = "42 0xFF 0b1010 0777 1_000_000";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 5);
        for (kind, _, _) in tokens {
            assert_eq!(kind, SyntaxKind::INT_LITERAL);
        }
    }

    #[test]
    fn test_long_literals() {
        let source = "42L 0xFFL";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 2);
        for (kind, _, _) in tokens {
            assert_eq!(kind, SyntaxKind::LONG_LITERAL);
        }
    }

    #[test]
    fn test_floating_literals() {
        let source = "3.14 1.0e10 .5 0x1.8p2";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 4);
        for (kind, _, _) in tokens {
            assert_eq!(kind, SyntaxKind::DOUBLE_LITERAL);
        }
    }

    #[test]
    fn test_float_literals() {
        let source = "2.0f 3.14F";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 2);
        for (kind, _, _) in tokens {
            assert_eq!(kind, SyntaxKind::FLOAT_LITERAL);
        }
    }

    #[test]
    fn test_string_literals() {
        let source = r#""hello" "world\n" "unicode\u0041""#;
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 3);
        for (kind, _, _) in tokens {
            assert_eq!(kind, SyntaxKind::STRING_LITERAL);
        }
    }

    #[test]
    fn test_operators() {
        let source = "+ - * / % && || == != >= <=";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 11);
        assert_eq!(tokens[0].0, SyntaxKind::PLUS);
        assert_eq!(tokens[1].0, SyntaxKind::MINUS);
        assert_eq!(tokens[2].0, SyntaxKind::STAR);
        assert_eq!(tokens[3].0, SyntaxKind::SLASH);
        assert_eq!(tokens[4].0, SyntaxKind::PERCENT);
        assert_eq!(tokens[5].0, SyntaxKind::AMP_AMP);
        assert_eq!(tokens[6].0, SyntaxKind::PIPE_PIPE);
        assert_eq!(tokens[7].0, SyntaxKind::EQ_EQ);
        assert_eq!(tokens[8].0, SyntaxKind::BANG_EQ);
        assert_eq!(tokens[9].0, SyntaxKind::GT_EQ);
        assert_eq!(tokens[10].0, SyntaxKind::LT_EQ);
    }

    #[test]
    fn test_comments() {
        let source = r"
            // line comment
            /* block comment */
            /** javadoc comment */
        ";
        let mut comment_count = 0;
        for (kind, _, _) in Lexer::new(source) {
            if matches!(
                kind,
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT | SyntaxKind::DOC_COMMENT
            ) {
                comment_count += 1;
            }
        }

        assert_eq!(comment_count, 3);
    }

    #[test]
    fn test_java8_features() {
        let source = "-> ::";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].0, SyntaxKind::ARROW);
        assert_eq!(tokens[1].0, SyntaxKind::COLON_COLON);
    }

    #[test]
    fn test_java17_features() {
        let source = "sealed permits non-sealed";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].0, SyntaxKind::SEALED_KW);
        assert_eq!(tokens[1].0, SyntaxKind::PERMITS_KW);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].1, "non");
        assert_eq!(tokens[3].0, SyntaxKind::MINUS);
        assert_eq!(tokens[4].0, SyntaxKind::SEALED_KW);
    }

    #[test]
    fn test_skip_trivia() {
        let source = "int x; // comment";
        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].0, SyntaxKind::INT_KW);
        assert_eq!(tokens[1].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].0, SyntaxKind::SEMICOLON);
    }

    #[test]
    fn test_complex_program() {
        let source = r#"
            public class HelloWorld {
                public static void main(String[] args) {
                    System.out.println("Hello, World!");
                }
            }
        "#;

        let tokens: Vec<_> = Lexer::new(source).skip_trivia().collect();

        // Should contain: public, class, HelloWorld, {, public, static, void, main,
        // (, String, [, ], args, ), {, System, ., out, ., println, (, "Hello, World!", ), ;, }, }
        assert!(tokens.len() > 20);

        // Check first few tokens
        assert_eq!(tokens[0].0, SyntaxKind::PUBLIC_KW);
        assert_eq!(tokens[1].0, SyntaxKind::CLASS_KW);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].1, "HelloWorld");
        assert_eq!(tokens[3].0, SyntaxKind::L_BRACE);
    }

    #[test]
    fn test_error_recovery() {
        let source = "int x = @#$;";
        let tokens: Vec<_> = Lexer::new(source).collect();

        // Should tokenize: int, whitespace, x, whitespace, =, whitespace, @, #, $, ;
        // Where # and $ are error tokens
        assert!(tokens.iter().any(|(k, _, _)| *k == SyntaxKind::ERROR));
        assert!(tokens.iter().any(|(k, _, _)| *k == SyntaxKind::INT_KW));
        assert!(tokens.iter().any(|(k, _, _)| *k == SyntaxKind::SEMICOLON));
    }

    #[test]
    fn test_text_ranges() {
        let source = "int x";
        let tokens: Vec<_> = Lexer::new(source).collect();

        // int starts at 0, ends at 3
        assert_eq!(tokens[0].2.start(), 0.into());
        assert_eq!(tokens[0].2.end(), 3.into());

        // whitespace at 3-4
        assert_eq!(tokens[1].2.start(), 3.into());
        assert_eq!(tokens[1].2.end(), 4.into());

        // x at 4-5
        assert_eq!(tokens[2].2.start(), 4.into());
        assert_eq!(tokens[2].2.end(), 5.into());
    }

    #[test]
    fn test_syntax_kind_properties() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(SyntaxKind::LINE_COMMENT.is_trivia());
        assert!(SyntaxKind::CLASS_KW.is_keyword());
        assert!(SyntaxKind::INT_LITERAL.is_literal());
        assert!(SyntaxKind::PLUS.is_operator());
        assert!(SyntaxKind::L_PAREN.is_punctuation());
    }
}
