// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! # Concrete Syntax Tree (CST) for Java
//!
//! This module provides a lossless representation of Java source code using the `rowan` library.
//! Unlike traditional Abstract Syntax Trees (ASTs), a Concrete Syntax Tree preserves all
//! information from the source, including:
//! - Whitespace (spaces, tabs, newlines)
//! - Comments (line comments, block comments, doc comments)
//! - All tokens and trivia
//! - Exact source positions
//!
//! ## Architecture
//!
//! The CST is built using a layered architecture:
//!
//! ### 1. SyntaxKind - Token and Node Types
//! Defines all possible token types (keywords, operators, literals) and node types
//! (expressions, statements, declarations) in the Java grammar.
//!
//! ### 2. Green Tree (Immutable, Interned)
//! - The underlying storage layer provided by `rowan`
//! - Immutable and structural-shared
//! - Memory efficient through interning
//! - No parent pointers, no positions
//!
//! ### 3. Red Tree (Lazy, Positional)
//! - SyntaxNode and SyntaxToken provide a view over the green tree
//! - Lazily computed absolute positions
//! - Parent pointers via runtime computation
//! - Enables tree traversal (parents, siblings, children)
//!
//! ### 4. Typed AST Nodes
//! - High-level wrappers around SyntaxNode
//! - Provide typed access to children (e.g., ClassDecl, MethodDecl)
//! - Implement domain-specific query methods
//! - Zero-cost abstractions (newtype pattern)
//!
//! ## Usage Example
//!
//! ```ignore
//! use hyperion_parser::syntax::{SyntaxNode, ClassDecl};
//!
//! fn analyze_class(node: SyntaxNode) {
//!     if let Some(class) = ClassDecl::cast(node) {
//!         println!("Class name: {:?}", class.name());
//!         for method in class.methods() {
//!             println!("  Method: {:?}", method.name());
//!         }
//!     }
//! }
//! ```
//!
//! ## Performance Characteristics
//!
//! - **Memory**: O(n) where n is the source size, with structural sharing
//! - **Parse**: O(n) single-pass parsing
//! - **Traversal**: O(1) for direct children, O(log n) for ancestors
//! - **Modification**: O(log n) for incremental changes via green tree sharing
//!
//! ## Design Principles
//!
//! 1. **Lossless**: Every byte of source is represented
//! 2. **Immutable**: Trees are immutable, modifications create new trees
//! 3. **Efficient**: Structural sharing minimizes memory usage
//! 4. **Incremental**: Supports incremental reparsing for edits
//! 5. **Error-Resilient**: Can represent partially parsed or malformed code
//! 6. **Traversable**: Easy navigation in any direction (parent, child, sibling)

use rowan::Language;
use std::fmt;

/// The Java language type for rowan.
/// This is a zero-sized type that implements the `Language` trait,
/// parameterizing rowan's generic syntax tree structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JavaLanguage {}

impl Language for JavaLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        SyntaxKind::from(raw.0)
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.into())
    }
}

/// All possible syntax kinds in the Java language.
///
/// This enum represents every type of token and node in a Java program.
/// It's divided into several categories:
///
/// - **Keywords**: Java reserved words (class, if, while, etc.)
/// - **Literals**: Numeric, string, boolean, null literals
/// - **Operators**: Arithmetic, logical, bitwise operators
/// - **Punctuation**: Braces, brackets, semicolons, etc.
/// - **Trivia**: Whitespace and comments (preserved for losslessness)
/// - **Node Types**: Composite syntax constructs (expressions, statements, declarations)
/// - **Special**: Error nodes, end-of-file, tombstone markers
///
/// # Safety Invariants
///
/// This enum uses `#[repr(u16)]` to enable efficient storage and conversion to/from u16.
/// The following invariants MUST be maintained for memory safety:
///
/// 1. **Contiguous Discriminants**: All variants must have contiguous discriminants starting from 0.
///    Do NOT add explicit discriminants (e.g., `FOO = 42`) as this breaks the From<u16> conversion.
///
/// 2. **Last Variant**: EOF must remain the last variant in the enum. The From<u16> implementation
///    validates that incoming u16 values are <= EOF's discriminant.
///
/// 3. **No Gaps**: Do not delete variants from the middle of the enum, as this creates gaps in
///    the discriminant space. If a variant becomes unused, mark it as deprecated but keep it.
///
/// 4. **Transmute Safety**: The From<u16> implementation uses unsafe transmute, which is only
///    sound because we validate the discriminant range. Any changes to the enum structure must
///    preserve the validation logic in From<u16>.
///
/// Violating these invariants will cause undefined behavior in the From<u16> conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // ===== Trivia (Whitespace and Comments) =====
    /// Whitespace: spaces, tabs, newlines
    WHITESPACE,
    /// Single-line comment: // ...
    LINE_COMMENT,
    /// Multi-line comment: /* ... */
    BLOCK_COMMENT,
    /// `JavaDoc` comment: /** ... */
    DOC_COMMENT,

    // ===== Keywords =====
    ABSTRACT_KW,
    ASSERT_KW,
    BOOLEAN_KW,
    BREAK_KW,
    BYTE_KW,
    CASE_KW,
    CATCH_KW,
    CHAR_KW,
    CLASS_KW,
    CONST_KW,
    CONTINUE_KW,
    DEFAULT_KW,
    DO_KW,
    DOUBLE_KW,
    ELSE_KW,
    ENUM_KW,
    EXTENDS_KW,
    FINAL_KW,
    FINALLY_KW,
    FLOAT_KW,
    FOR_KW,
    GOTO_KW,
    IF_KW,
    IMPLEMENTS_KW,
    IMPORT_KW,
    INSTANCEOF_KW,
    INT_KW,
    INTERFACE_KW,
    LONG_KW,
    NATIVE_KW,
    NEW_KW,
    PACKAGE_KW,
    PRIVATE_KW,
    PROTECTED_KW,
    PUBLIC_KW,
    RETURN_KW,
    SHORT_KW,
    STATIC_KW,
    STRICTFP_KW,
    SUPER_KW,
    SWITCH_KW,
    SYNCHRONIZED_KW,
    THIS_KW,
    THROW_KW,
    THROWS_KW,
    TRANSIENT_KW,
    TRY_KW,
    VOID_KW,
    VOLATILE_KW,
    WHILE_KW,

    // Java 9+ Module Keywords
    MODULE_KW,
    REQUIRES_KW,
    EXPORTS_KW,
    OPENS_KW,
    PROVIDES_KW,
    USES_KW,
    TO_KW,
    WITH_KW,
    TRANSITIVE_KW,
    OPEN_KW,

    // Context-sensitive keywords
    RECORD_KW,     // Java 14+
    SEALED_KW,     // Java 15+
    PERMITS_KW,    // Java 15+
    NON_SEALED_KW, // Java 15+
    VAR_KW,        // Java 10+
    WHEN_KW,       // Java 21+ (pattern matching guards)
    YIELD_KW,      // Java 13+

    // ===== Literals =====
    /// Integer literal: 42, 0xFF, 0b1010, `123_456`
    INT_LITERAL,
    /// Long literal: 42L, 0xFFL
    LONG_LITERAL,
    /// Float literal: 3.14f, 1.0e-5f
    FLOAT_LITERAL,
    /// Double literal: 3.14, 1.0e-5, 0x1.0p-5
    DOUBLE_LITERAL,
    /// Character literal: 'a', '\n', '\u0041'
    CHAR_LITERAL,
    /// String literal: "hello", "multi\nline"
    STRING_LITERAL,
    /// Text block (Java 13+): """..."""
    TEXT_BLOCK_LITERAL,
    /// Boolean literal: true, false
    BOOLEAN_LITERAL,
    /// Null literal: null
    NULL_LITERAL,

    // ===== Identifiers =====
    /// Identifier: variable names, class names, etc.
    IDENT,

    // ===== Operators =====
    // Arithmetic
    PLUS,    // +
    MINUS,   // -
    STAR,    // *
    SLASH,   // /
    PERCENT, // %

    // Bitwise
    AMP,   // &
    PIPE,  // |
    CARET, // ^
    TILDE, // ~
    SHL,   // <<
    SHR,   // >>
    USHR,  // >>>

    // Logical
    AMP_AMP,   // &&
    PIPE_PIPE, // ||
    BANG,      // !

    // Comparison
    EQ_EQ,   // ==
    BANG_EQ, // !=
    LT,      // <
    GT,      // >
    LT_EQ,   // <=
    GT_EQ,   // >=

    // Assignment
    EQ,         // =
    PLUS_EQ,    // +=
    MINUS_EQ,   // -=
    STAR_EQ,    // *=
    SLASH_EQ,   // /=
    PERCENT_EQ, // %=
    AMP_EQ,     // &=
    PIPE_EQ,    // |=
    CARET_EQ,   // ^=
    SHL_EQ,     // <<=
    SHR_EQ,     // >>=
    USHR_EQ,    // >>>=

    // Increment/Decrement
    PLUS_PLUS,   // ++
    MINUS_MINUS, // --

    // Other
    QUESTION,    // ?
    COLON,       // :
    ARROW,       // ->
    COLON_COLON, // ::
    AT,          // @
    DOT,         // .
    ELLIPSIS,    // ...

    // ===== Punctuation =====
    L_PAREN,   // (
    R_PAREN,   // )
    L_BRACE,   // {
    R_BRACE,   // }
    L_BRACKET, // [
    R_BRACKET, // ]
    SEMICOLON, // ;
    COMMA,     // ,

    // ===== Composite Node Types =====

    // Top-level
    /// Source file root node
    SOURCE_FILE,
    /// Package declaration: package com.example;
    PACKAGE_DECL,
    /// Import declaration: import java.util.*;
    IMPORT_DECL,

    // Type Declarations
    /// Class declaration: class Foo { ... }
    CLASS_DECL,
    /// Interface declaration: interface Bar { ... }
    INTERFACE_DECL,
    /// Enum declaration: enum Color { RED, GREEN, BLUE }
    ENUM_DECL,
    /// Annotation declaration: @interface `MyAnnotation` { ... }
    ANNOTATION_DECL,
    /// Record declaration: record Point(int x, int y) { }
    RECORD_DECL,

    // Class Members
    /// Field declaration: private int x;
    FIELD_DECL,
    /// Method declaration: public void `foo()` { ... }
    METHOD_DECL,
    /// Constructor declaration: public `Foo()` { ... }
    CONSTRUCTOR_DECL,
    /// Static initializer: static { ... }
    STATIC_INIT,
    /// Instance initializer: { ... }
    INSTANCE_INIT,
    /// Enum constant: RED, GREEN(0xFF00)
    ENUM_CONSTANT,

    // Method Components
    /// Parameter list: (int x, String y)
    PARAM_LIST,
    /// Parameter: int x, final String... args
    PARAM,
    /// Throws clause: throws `IOException`, `SQLException`
    THROWS_CLAUSE,
    /// Type parameter list: <T, U extends List>
    TYPE_PARAM_LIST,
    /// Type parameter: T, U extends Comparable<U>
    TYPE_PARAM,
    /// Type bound: extends Number & Comparable
    TYPE_BOUND,

    // Statements
    /// Block statement: { ... }
    BLOCK_STMT,
    /// Expression statement: x = 5;
    EXPR_STMT,
    /// If statement: if (cond) { ... } else { ... }
    IF_STMT,
    /// While statement: while (cond) { ... }
    WHILE_STMT,
    /// Do-while statement: do { ... } while (cond);
    DO_WHILE_STMT,
    /// For statement: for (int i = 0; i < 10; i++) { ... }
    FOR_STMT,
    /// Enhanced for statement: for (String s : list) { ... }
    FOREACH_STMT,
    /// Switch statement: switch (x) { case 1: ... }
    SWITCH_STMT,
    /// Switch expression (Java 12+): x = switch (y) { ... }
    SWITCH_EXPR,
    /// Case clause: case 1, 2, 3:
    CASE_CLAUSE,
    /// When clause in pattern (Java 21+): when x > 0
    WHEN_CLAUSE,
    /// Try statement: try { ... } catch (E e) { ... } finally { ... }
    TRY_STMT,
    /// Catch clause: catch (`IOException` e) { ... }
    CATCH_CLAUSE,
    /// Finally clause: finally { ... }
    FINALLY_CLAUSE,
    /// Try-with-resources: try (Stream s = ...) { ... }
    TRY_WITH_RESOURCES_STMT,
    /// Resource specification: Stream s = ...
    RESOURCE_SPEC,
    /// Synchronized statement: synchronized (lock) { ... }
    SYNCHRONIZED_STMT,
    /// Return statement: return expr;
    RETURN_STMT,
    /// Throw statement: throw new `Exception()`;
    THROW_STMT,
    /// Break statement: break label;
    BREAK_STMT,
    /// Continue statement: continue label;
    CONTINUE_STMT,
    /// Assert statement: assert cond : message;
    ASSERT_STMT,
    /// Empty statement: ;
    EMPTY_STMT,
    /// Labeled statement: label: statement
    LABELED_STMT,
    /// Explicit constructor invocation: this(...); or super(...);
    EXPLICIT_CONSTRUCTOR_INVOCATION,
    /// Local variable declaration: int x = 5;
    LOCAL_VAR_DECL,
    /// Yield statement (Java 13+): yield value;
    YIELD_STMT,

    // Expressions
    /// Binary expression: a + b, x && y
    BINARY_EXPR,
    /// Unary expression: -x, !flag, ++i
    UNARY_EXPR,
    /// Ternary/conditional expression: cond ? a : b
    TERNARY_EXPR,
    /// Assignment expression: x = 5, y += 3
    ASSIGNMENT_EXPR,
    /// Method call: obj.method(args)
    METHOD_CALL_EXPR,
    /// Field access: obj.field
    FIELD_ACCESS_EXPR,
    /// Array access: arr[index]
    ARRAY_ACCESS_EXPR,
    /// New expression: new `Foo()`, new int[5]
    NEW_EXPR,
    /// Array initializer: {1, 2, 3}
    ARRAY_INIT_EXPR,
    /// Cast expression: (String) obj
    CAST_EXPR,
    /// Instanceof expression: obj instanceof String
    INSTANCEOF_EXPR,
    /// Pattern instanceof (Java 14+): obj instanceof String s
    PATTERN_INSTANCEOF_EXPR,
    /// Lambda expression: x -> x + 1, (a, b) -> a + b
    LAMBDA_EXPR,
    /// Method reference: `String::length`, `System.out::println`
    METHOD_REF_EXPR,
    /// Constructor reference: `ArrayList::new`
    CONSTRUCTOR_REF_EXPR,
    /// Parenthesized expression: (expr)
    PAREN_EXPR,
    /// This expression: this
    THIS_EXPR,
    /// Super expression: super
    SUPER_EXPR,
    /// Class literal: String.class
    CLASS_LITERAL_EXPR,
    /// Literal expression: 42, "hello", true
    LITERAL_EXPR,
    /// Name/identifier expression: variableName
    NAME_EXPR,

    // Types
    /// Primitive type: int, boolean, etc.
    PRIMITIVE_TYPE,
    /// Array type: int[], String[][]
    ARRAY_TYPE,
    /// Class/interface type: String, List<Integer>
    CLASS_TYPE,
    /// Type arguments: <String, Integer>
    TYPE_ARGS,
    /// Wildcard type: ?, ? extends Number, ? super Integer
    WILDCARD_TYPE,
    /// Var type (Java 10+): var
    VAR_TYPE,
    /// Qualified type: OuterClass.InnerClass
    QUALIFIED_TYPE,
    /// Union type (in catch): `IOException` | `SQLException`
    UNION_TYPE,
    /// Intersection type: T & Serializable
    INTERSECTION_TYPE,

    // Annotations
    /// Annotation: @Override, @SuppressWarnings("unchecked")
    ANNOTATION,
    /// Annotation argument list: ("value", key = "val")
    ANNOTATION_ARG_LIST,
    /// Annotation argument: value = "foo"
    ANNOTATION_ARG,
    /// Annotation array value: {val1, val2}
    ANNOTATION_ARRAY_VALUE,
    /// Annotation method default value: default -1
    DEFAULT_VALUE,

    // Modifiers
    /// Modifier list: public static final
    MODIFIER_LIST,
    /// Single modifier: public, static, final, etc.
    MODIFIER,

    // Names and Paths
    /// Simple name: foo
    SIMPLE_NAME,
    /// Qualified name: java.util.List
    QUALIFIED_NAME,

    // Argument Lists
    /// Argument list: (arg1, arg2, arg3)
    ARG_LIST,

    // Module System (Java 9+)
    /// Module declaration: module com.example { ... }
    MODULE_DECL,
    /// Requires directive: requires java.base;
    REQUIRES_DIRECTIVE,
    /// Exports directive: exports com.example.api;
    EXPORTS_DIRECTIVE,
    /// Opens directive: opens com.example.internal;
    OPENS_DIRECTIVE,
    /// Uses directive: uses com.example.Service;
    USES_DIRECTIVE,
    /// Provides directive: provides Service with `ServiceImpl`;
    PROVIDES_DIRECTIVE,

    // Patterns (Java 14+)
    /// Type pattern: String s
    TYPE_PATTERN,
    /// Record pattern (Java 19+): Point(var x, var y)
    RECORD_PATTERN,

    // Error Recovery
    /// Error node for malformed syntax
    ERROR,
    /// Tombstone marker for deleted nodes
    TOMBSTONE,

    /// End of file marker
    EOF,
}

impl From<u16> for SyntaxKind {
    fn from(raw: u16) -> Self {
        // Validate the discriminant is within the valid range
        // EOF is the last variant in the enum
        assert!(
            raw <= Self::EOF as u16,
            "Invalid SyntaxKind discriminant: {}. Max valid value is {}",
            raw,
            Self::EOF as u16
        );

        // Safety: We've validated that the discriminant is within the valid range
        // of the enum. This is a repr(u16) enum with contiguous discriminants
        // starting from 0, so any value <= EOF is a valid SyntaxKind variant.
        unsafe { std::mem::transmute(raw) }
    }
}

impl From<SyntaxKind> for u16 {
    fn from(kind: SyntaxKind) -> Self {
        kind as Self
    }
}

impl SyntaxKind {
    /// Returns true if this is a trivia token (whitespace or comment).
    #[must_use]
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE | Self::LINE_COMMENT | Self::BLOCK_COMMENT | Self::DOC_COMMENT
        )
    }

    /// Returns true if this is a comment token (line, block, or doc comment).
    #[inline]
    #[must_use]
    pub const fn is_comment(self) -> bool {
        matches!(
            self,
            Self::LINE_COMMENT | Self::BLOCK_COMMENT | Self::DOC_COMMENT
        )
    }

    /// Returns true if this is a string literal token (regular or text block).
    #[inline]
    #[must_use]
    pub const fn is_string_literal(self) -> bool {
        matches!(self, Self::STRING_LITERAL | Self::TEXT_BLOCK_LITERAL)
    }

    /// Returns true if this token is non-code (comment or string literal).
    /// Useful for skipping LSP operations on tokens that aren't navigable code.
    /// Does NOT include whitespace — use `is_trivia()` for that.
    #[inline]
    #[must_use]
    pub const fn is_non_code(self) -> bool {
        self.is_comment() || self.is_string_literal()
    }

    /// Returns true if this is a type declaration node kind
    /// (class, interface, enum, annotation, or record).
    #[inline]
    #[must_use]
    pub const fn is_type_declaration(self) -> bool {
        matches!(
            self,
            Self::CLASS_DECL
                | Self::INTERFACE_DECL
                | Self::ENUM_DECL
                | Self::ANNOTATION_DECL
                | Self::RECORD_DECL
        )
    }

    /// Returns true if this is a member declaration node kind
    /// (method, constructor, or field).
    #[inline]
    #[must_use]
    pub const fn is_member_declaration(self) -> bool {
        matches!(
            self,
            Self::METHOD_DECL | Self::CONSTRUCTOR_DECL | Self::FIELD_DECL
        )
    }

    /// Returns true if this is a keyword token.
    #[must_use]
    pub const fn is_keyword(self) -> bool {
        (self as u16) >= (Self::ABSTRACT_KW as u16) && (self as u16) <= (Self::YIELD_KW as u16)
    }

    /// Returns true if this is a literal token.
    #[must_use]
    pub const fn is_literal(self) -> bool {
        matches!(
            self,
            Self::INT_LITERAL
                | Self::LONG_LITERAL
                | Self::FLOAT_LITERAL
                | Self::DOUBLE_LITERAL
                | Self::CHAR_LITERAL
                | Self::STRING_LITERAL
                | Self::TEXT_BLOCK_LITERAL
                | Self::BOOLEAN_LITERAL
                | Self::NULL_LITERAL
        )
    }

    /// Returns true if this is an operator token.
    #[must_use]
    pub const fn is_operator(self) -> bool {
        (self as u16) >= (Self::PLUS as u16) && (self as u16) <= (Self::ELLIPSIS as u16)
    }

    /// Returns true if this is a punctuation token.
    #[must_use]
    pub const fn is_punctuation(self) -> bool {
        matches!(
            self,
            Self::L_PAREN
                | Self::R_PAREN
                | Self::L_BRACE
                | Self::R_BRACE
                | Self::L_BRACKET
                | Self::R_BRACKET
                | Self::SEMICOLON
                | Self::COMMA
        )
    }

    /// Returns true if this is a composite node (not a token).
    #[must_use]
    pub const fn is_node(self) -> bool {
        self as u16 >= Self::SOURCE_FILE as u16 && (self as u16) < Self::ERROR as u16
    }

    /// Returns the name of this syntax kind as a string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WHITESPACE => "WHITESPACE",
            Self::LINE_COMMENT => "LINE_COMMENT",
            Self::BLOCK_COMMENT => "BLOCK_COMMENT",
            Self::DOC_COMMENT => "DOC_COMMENT",
            Self::ABSTRACT_KW => "abstract",
            Self::ASSERT_KW => "assert",
            Self::BOOLEAN_KW => "boolean",
            Self::BREAK_KW => "break",
            Self::BYTE_KW => "byte",
            Self::CASE_KW => "case",
            Self::CATCH_KW => "catch",
            Self::CHAR_KW => "char",
            Self::CLASS_KW => "class",
            Self::CONST_KW => "const",
            Self::CONTINUE_KW => "continue",
            Self::DEFAULT_KW => "default",
            Self::DO_KW => "do",
            Self::DOUBLE_KW => "double",
            Self::ELSE_KW => "else",
            Self::ENUM_KW => "enum",
            Self::EXTENDS_KW => "extends",
            Self::FINAL_KW => "final",
            Self::FINALLY_KW => "finally",
            Self::FLOAT_KW => "float",
            Self::FOR_KW => "for",
            Self::GOTO_KW => "goto",
            Self::IF_KW => "if",
            Self::IMPLEMENTS_KW => "implements",
            Self::IMPORT_KW => "import",
            Self::INSTANCEOF_KW => "instanceof",
            Self::INT_KW => "int",
            Self::INTERFACE_KW => "interface",
            Self::LONG_KW => "long",
            Self::NATIVE_KW => "native",
            Self::NEW_KW => "new",
            Self::PACKAGE_KW => "package",
            Self::PRIVATE_KW => "private",
            Self::PROTECTED_KW => "protected",
            Self::PUBLIC_KW => "public",
            Self::RETURN_KW => "return",
            Self::SHORT_KW => "short",
            Self::STATIC_KW => "static",
            Self::STRICTFP_KW => "strictfp",
            Self::SUPER_KW => "super",
            Self::SWITCH_KW => "switch",
            Self::SYNCHRONIZED_KW => "synchronized",
            Self::THIS_KW => "this",
            Self::THROW_KW => "throw",
            Self::THROWS_KW => "throws",
            Self::TRANSIENT_KW => "transient",
            Self::TRY_KW => "try",
            Self::VOID_KW => "void",
            Self::VOLATILE_KW => "volatile",
            Self::WHILE_KW => "while",
            Self::MODULE_KW => "module",
            Self::REQUIRES_KW => "requires",
            Self::EXPORTS_KW => "exports",
            Self::OPENS_KW => "opens",
            Self::PROVIDES_KW => "provides",
            Self::USES_KW => "uses",
            Self::TO_KW => "to",
            Self::WITH_KW => "with",
            Self::TRANSITIVE_KW => "transitive",
            Self::OPEN_KW => "open",
            Self::RECORD_KW => "record",
            Self::SEALED_KW => "sealed",
            Self::PERMITS_KW => "permits",
            Self::NON_SEALED_KW => "non-sealed",
            Self::VAR_KW => "var",
            Self::WHEN_KW => "when",
            Self::YIELD_KW => "yield",
            Self::INT_LITERAL => "INT_LITERAL",
            Self::LONG_LITERAL => "LONG_LITERAL",
            Self::FLOAT_LITERAL => "FLOAT_LITERAL",
            Self::DOUBLE_LITERAL => "DOUBLE_LITERAL",
            Self::CHAR_LITERAL => "CHAR_LITERAL",
            Self::STRING_LITERAL => "STRING_LITERAL",
            Self::TEXT_BLOCK_LITERAL => "TEXT_BLOCK_LITERAL",
            Self::BOOLEAN_LITERAL => "BOOLEAN_LITERAL",
            Self::NULL_LITERAL => "NULL_LITERAL",
            Self::IDENT => "IDENT",
            Self::PLUS => "+",
            Self::MINUS => "-",
            Self::STAR => "*",
            Self::SLASH => "/",
            Self::PERCENT => "%",
            Self::AMP => "&",
            Self::PIPE => "|",
            Self::CARET => "^",
            Self::TILDE => "~",
            Self::SHL => "<<",
            Self::SHR => ">>",
            Self::USHR => ">>>",
            Self::AMP_AMP => "&&",
            Self::PIPE_PIPE => "||",
            Self::BANG => "!",
            Self::EQ_EQ => "==",
            Self::BANG_EQ => "!=",
            Self::LT => "<",
            Self::GT => ">",
            Self::LT_EQ => "<=",
            Self::GT_EQ => ">=",
            Self::EQ => "=",
            Self::PLUS_EQ => "+=",
            Self::MINUS_EQ => "-=",
            Self::STAR_EQ => "*=",
            Self::SLASH_EQ => "/=",
            Self::PERCENT_EQ => "%=",
            Self::AMP_EQ => "&=",
            Self::PIPE_EQ => "|=",
            Self::CARET_EQ => "^=",
            Self::SHL_EQ => "<<=",
            Self::SHR_EQ => ">>=",
            Self::USHR_EQ => ">>>=",
            Self::PLUS_PLUS => "++",
            Self::MINUS_MINUS => "--",
            Self::QUESTION => "?",
            Self::COLON => ":",
            Self::ARROW => "->",
            Self::COLON_COLON => "::",
            Self::AT => "@",
            Self::DOT => ".",
            Self::ELLIPSIS => "...",
            Self::L_PAREN => "(",
            Self::R_PAREN => ")",
            Self::L_BRACE => "{",
            Self::R_BRACE => "}",
            Self::L_BRACKET => "[",
            Self::R_BRACKET => "]",
            Self::SEMICOLON => ";",
            Self::COMMA => ",",
            Self::SOURCE_FILE => "SOURCE_FILE",
            Self::PACKAGE_DECL => "PACKAGE_DECL",
            Self::IMPORT_DECL => "IMPORT_DECL",
            Self::CLASS_DECL => "CLASS_DECL",
            Self::INTERFACE_DECL => "INTERFACE_DECL",
            Self::ENUM_DECL => "ENUM_DECL",
            Self::ANNOTATION_DECL => "ANNOTATION_DECL",
            Self::RECORD_DECL => "RECORD_DECL",
            Self::FIELD_DECL => "FIELD_DECL",
            Self::METHOD_DECL => "METHOD_DECL",
            Self::CONSTRUCTOR_DECL => "CONSTRUCTOR_DECL",
            Self::STATIC_INIT => "STATIC_INIT",
            Self::INSTANCE_INIT => "INSTANCE_INIT",
            Self::ENUM_CONSTANT => "ENUM_CONSTANT",
            Self::PARAM_LIST => "PARAM_LIST",
            Self::PARAM => "PARAM",
            Self::THROWS_CLAUSE => "THROWS_CLAUSE",
            Self::TYPE_PARAM_LIST => "TYPE_PARAM_LIST",
            Self::TYPE_PARAM => "TYPE_PARAM",
            Self::TYPE_BOUND => "TYPE_BOUND",
            Self::BLOCK_STMT => "BLOCK_STMT",
            Self::EXPR_STMT => "EXPR_STMT",
            Self::IF_STMT => "IF_STMT",
            Self::WHILE_STMT => "WHILE_STMT",
            Self::DO_WHILE_STMT => "DO_WHILE_STMT",
            Self::FOR_STMT => "FOR_STMT",
            Self::FOREACH_STMT => "FOREACH_STMT",
            Self::SWITCH_STMT => "SWITCH_STMT",
            Self::SWITCH_EXPR => "SWITCH_EXPR",
            Self::CASE_CLAUSE => "CASE_CLAUSE",
            Self::WHEN_CLAUSE => "WHEN_CLAUSE",
            Self::TRY_STMT => "TRY_STMT",
            Self::CATCH_CLAUSE => "CATCH_CLAUSE",
            Self::FINALLY_CLAUSE => "FINALLY_CLAUSE",
            Self::TRY_WITH_RESOURCES_STMT => "TRY_WITH_RESOURCES_STMT",
            Self::RESOURCE_SPEC => "RESOURCE_SPEC",
            Self::SYNCHRONIZED_STMT => "SYNCHRONIZED_STMT",
            Self::RETURN_STMT => "RETURN_STMT",
            Self::THROW_STMT => "THROW_STMT",
            Self::BREAK_STMT => "BREAK_STMT",
            Self::CONTINUE_STMT => "CONTINUE_STMT",
            Self::ASSERT_STMT => "ASSERT_STMT",
            Self::EMPTY_STMT => "EMPTY_STMT",
            Self::LABELED_STMT => "LABELED_STMT",
            Self::EXPLICIT_CONSTRUCTOR_INVOCATION => "EXPLICIT_CONSTRUCTOR_INVOCATION",
            Self::LOCAL_VAR_DECL => "LOCAL_VAR_DECL",
            Self::YIELD_STMT => "YIELD_STMT",
            Self::BINARY_EXPR => "BINARY_EXPR",
            Self::UNARY_EXPR => "UNARY_EXPR",
            Self::TERNARY_EXPR => "TERNARY_EXPR",
            Self::ASSIGNMENT_EXPR => "ASSIGNMENT_EXPR",
            Self::METHOD_CALL_EXPR => "METHOD_CALL_EXPR",
            Self::FIELD_ACCESS_EXPR => "FIELD_ACCESS_EXPR",
            Self::ARRAY_ACCESS_EXPR => "ARRAY_ACCESS_EXPR",
            Self::NEW_EXPR => "NEW_EXPR",
            Self::ARRAY_INIT_EXPR => "ARRAY_INIT_EXPR",
            Self::CAST_EXPR => "CAST_EXPR",
            Self::INSTANCEOF_EXPR => "INSTANCEOF_EXPR",
            Self::PATTERN_INSTANCEOF_EXPR => "PATTERN_INSTANCEOF_EXPR",
            Self::LAMBDA_EXPR => "LAMBDA_EXPR",
            Self::METHOD_REF_EXPR => "METHOD_REF_EXPR",
            Self::CONSTRUCTOR_REF_EXPR => "CONSTRUCTOR_REF_EXPR",
            Self::PAREN_EXPR => "PAREN_EXPR",
            Self::THIS_EXPR => "THIS_EXPR",
            Self::SUPER_EXPR => "SUPER_EXPR",
            Self::CLASS_LITERAL_EXPR => "CLASS_LITERAL_EXPR",
            Self::LITERAL_EXPR => "LITERAL_EXPR",
            Self::NAME_EXPR => "NAME_EXPR",
            Self::PRIMITIVE_TYPE => "PRIMITIVE_TYPE",
            Self::ARRAY_TYPE => "ARRAY_TYPE",
            Self::CLASS_TYPE => "CLASS_TYPE",
            Self::TYPE_ARGS => "TYPE_ARGS",
            Self::WILDCARD_TYPE => "WILDCARD_TYPE",
            Self::VAR_TYPE => "VAR_TYPE",
            Self::QUALIFIED_TYPE => "QUALIFIED_TYPE",
            Self::UNION_TYPE => "UNION_TYPE",
            Self::INTERSECTION_TYPE => "INTERSECTION_TYPE",
            Self::ANNOTATION => "ANNOTATION",
            Self::ANNOTATION_ARG_LIST => "ANNOTATION_ARG_LIST",
            Self::ANNOTATION_ARG => "ANNOTATION_ARG",
            Self::ANNOTATION_ARRAY_VALUE => "ANNOTATION_ARRAY_VALUE",
            Self::DEFAULT_VALUE => "DEFAULT_VALUE",
            Self::MODIFIER_LIST => "MODIFIER_LIST",
            Self::MODIFIER => "MODIFIER",
            Self::SIMPLE_NAME => "SIMPLE_NAME",
            Self::QUALIFIED_NAME => "QUALIFIED_NAME",
            Self::ARG_LIST => "ARG_LIST",
            Self::MODULE_DECL => "MODULE_DECL",
            Self::REQUIRES_DIRECTIVE => "REQUIRES_DIRECTIVE",
            Self::EXPORTS_DIRECTIVE => "EXPORTS_DIRECTIVE",
            Self::OPENS_DIRECTIVE => "OPENS_DIRECTIVE",
            Self::USES_DIRECTIVE => "USES_DIRECTIVE",
            Self::PROVIDES_DIRECTIVE => "PROVIDES_DIRECTIVE",
            Self::TYPE_PATTERN => "TYPE_PATTERN",
            Self::RECORD_PATTERN => "RECORD_PATTERN",
            Self::ERROR => "ERROR",
            Self::TOMBSTONE => "TOMBSTONE",
            Self::EOF => "EOF",
        }
    }

    /// Returns true if this is a contextual keyword.
    ///
    /// Contextual keywords are keywords that are only keywords in specific syntactic contexts
    /// and can be used as identifiers elsewhere (parameter names, variable names, field names, etc.).
    ///
    /// Examples:
    /// - `var` - keyword only for local variable type inference (Java 10+)
    /// - `record` - keyword only when declaring record classes (Java 14+)
    /// - `yield` - keyword only in switch expressions (Java 13+)
    /// - `sealed`, `permits`, `non-sealed` - keywords only for sealed classes (Java 17+)
    /// - `when` - keyword only in pattern guards (Java 21+)
    /// - `module`, `requires`, `exports`, `opens`, `to`, `uses`, `provides`, `with`,
    ///   `transitive`, `open` - keywords only in module declarations (Java 9+)
    ///
    /// # Examples
    ///
    /// ```
    /// use hyperion_parser::syntax::SyntaxKind;
    ///
    /// assert!(SyntaxKind::RECORD_KW.is_contextual_keyword());
    /// assert!(SyntaxKind::VAR_KW.is_contextual_keyword());
    /// assert!(SyntaxKind::TO_KW.is_contextual_keyword());
    /// assert!(!SyntaxKind::CLASS_KW.is_contextual_keyword());
    /// assert!(!SyntaxKind::IDENT.is_contextual_keyword());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_contextual_keyword(self) -> bool {
        matches!(
            self,
            // Java 10+ local variable type inference
            Self::VAR_KW
            // Java 14+ records and switch expressions
                | Self::RECORD_KW
                | Self::YIELD_KW
            // Java 17+ sealed classes
                | Self::SEALED_KW
                | Self::PERMITS_KW
                | Self::NON_SEALED_KW
            // Java 21+ pattern matching
                | Self::WHEN_KW
            // Java 9+ module system (all are contextual per JLS)
                | Self::MODULE_KW
                | Self::REQUIRES_KW
                | Self::EXPORTS_KW
                | Self::OPENS_KW
                | Self::TO_KW
                | Self::USES_KW
                | Self::PROVIDES_KW
                | Self::WITH_KW
                | Self::TRANSITIVE_KW
                | Self::OPEN_KW
        )
    }

    /// Returns true if this can act as an identifier.
    ///
    /// This includes:
    /// - `IDENT` - regular identifiers
    /// - Contextual keywords (`record`, `var`, `yield`, etc.) - can be identifiers in most contexts
    ///
    /// Contextual keywords are only keywords in specific syntactic positions and can be used
    /// as identifiers elsewhere:
    /// - Parameter names: `void replay(ReleaseRecord record)`
    /// - Variable names: `ReleaseRecord record = ...`
    /// - Field names: `private ReleaseRecord record;`
    /// - Method names: `public void record() { }`
    ///
    /// # Examples
    ///
    /// ```
    /// use hyperion_parser::syntax::SyntaxKind;
    ///
    /// assert!(SyntaxKind::IDENT.is_identifier());
    /// assert!(SyntaxKind::RECORD_KW.is_identifier());
    /// assert!(SyntaxKind::VAR_KW.is_identifier());
    /// assert!(!SyntaxKind::CLASS_KW.is_identifier());
    /// assert!(!SyntaxKind::INT_KW.is_identifier());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_identifier(self) -> bool {
        // `non-sealed` is a contextual keyword, but its hyphenated spelling can
        // never form one Java identifier outside the modifier context.
        matches!(self, Self::IDENT)
            || (self.is_contextual_keyword() && !matches!(self, Self::NON_SEALED_KW))
    }

    /// Returns true if this can be used as a type name (class, interface, enum, record name).
    ///
    /// JLS 3.8 excludes only `permits`, `record`, `sealed`, `var`, and `yield`
    /// from `TypeIdentifier`. Other contextual keywords, including module-system
    /// words and `when`, remain legal type names.
    ///
    /// # Examples
    ///
    /// ```
    /// use hyperion_parser::syntax::SyntaxKind;
    ///
    /// assert!(SyntaxKind::IDENT.is_type_identifier());
    /// assert!(!SyntaxKind::RECORD_KW.is_type_identifier());  // Cannot name class 'record'
    /// assert!(!SyntaxKind::VAR_KW.is_type_identifier());     // Cannot name class 'var'
    /// assert!(SyntaxKind::WHEN_KW.is_type_identifier());     // Legal outside guard syntax
    /// assert!(!SyntaxKind::CLASS_KW.is_type_identifier());   // Not an identifier at all
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_type_identifier(self) -> bool {
        self.is_identifier()
            && !matches!(
                self,
                Self::PERMITS_KW
                    | Self::RECORD_KW
                    | Self::SEALED_KW
                    | Self::VAR_KW
                    | Self::YIELD_KW
            )
    }
}

impl fmt::Display for SyntaxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ===== Rowan Type Aliases =====

/// A syntax node in the CST.
///
/// This is a thin wrapper around rowan's `SyntaxNode`, providing:
/// - Tree navigation (parent, children, siblings)
/// - Position information (text range, offset)
/// - Text access
/// - Descendant iteration
///
/// Syntax nodes are cheap to clone (uses `Rc` internally).
pub type SyntaxNode = rowan::SyntaxNode<JavaLanguage>;

/// A syntax token (leaf node) in the CST.
///
/// Represents an atomic piece of text like a keyword, identifier, or operator.
pub type SyntaxToken = rowan::SyntaxToken<JavaLanguage>;

/// A syntax element that can be either a node or a token.
pub type SyntaxElement = rowan::SyntaxElement<JavaLanguage>;

/// A text range in the source code.
pub type TextRange = rowan::TextRange;

/// A text size/offset in the source code.
pub type TextSize = rowan::TextSize;

// ===== AST Node Trait =====

/// Common trait for all typed AST nodes.
///
/// This trait provides the foundation for type-safe CST navigation.
/// Each typed node (`ClassDecl`, `MethodDecl`, etc.) wraps a `SyntaxNode`
/// and provides domain-specific methods.
pub trait AstNode: Clone {
    /// Try to cast a syntax node to this AST node type.
    ///
    /// Returns `Some` if the node has the correct kind, `None` otherwise.
    fn cast(node: SyntaxNode) -> Option<Self>
    where
        Self: Sized;

    /// Returns true if this syntax node can be cast to this AST node type.
    fn can_cast(kind: SyntaxKind) -> bool
    where
        Self: Sized;

    /// Returns the underlying syntax node.
    fn syntax(&self) -> &SyntaxNode;

    /// Returns the text range of this node in the source.
    fn text_range(&self) -> TextRange {
        self.syntax().text_range()
    }

    /// Returns the text of this node.
    fn text(&self) -> String {
        self.syntax().text().to_string()
    }
}

// ===== Typed AST Node Macros =====

/// Helper macro to define typed AST nodes with minimal boilerplate.
macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name {
            syntax: SyntaxNode,
        }

        impl AstNode for $name {
            fn cast(node: SyntaxNode) -> Option<Self> {
                if node.kind() == SyntaxKind::$kind {
                    Some(Self { syntax: node })
                } else {
                    None
                }
            }

            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.syntax
            }
        }

        impl $name {
            /// Creates a new instance from a syntax node.
            ///
            /// # Panics
            /// Panics if the node has the wrong kind (debug builds only).
            #[must_use]
            pub fn new(syntax: SyntaxNode) -> Self {
                debug_assert_eq!(syntax.kind(), SyntaxKind::$kind);
                Self { syntax }
            }
        }
    };
}

// ===== Top-Level Nodes =====

ast_node!(SourceFile, SOURCE_FILE);

impl SourceFile {
    /// Returns the package declaration, if present.
    pub fn package(&self) -> Option<PackageDecl> {
        self.syntax.children().find_map(PackageDecl::cast)
    }

    /// Returns all import declarations.
    pub fn imports(&self) -> impl Iterator<Item = ImportDecl> + '_ {
        self.syntax.children().filter_map(ImportDecl::cast)
    }

    /// Returns all type declarations (classes, interfaces, enums, etc.).
    pub fn types(&self) -> impl Iterator<Item = TypeDecl> + '_ {
        self.syntax.children().filter_map(TypeDecl::cast)
    }

    /// Returns the module declaration, if present (Java 9+).
    pub fn module(&self) -> Option<ModuleDecl> {
        self.syntax.children().find_map(ModuleDecl::cast)
    }
}

ast_node!(PackageDecl, PACKAGE_DECL);

impl PackageDecl {
    /// Returns the package name.
    pub fn name(&self) -> Option<QualifiedName> {
        self.syntax.children().find_map(QualifiedName::cast)
    }
}

ast_node!(ImportDecl, IMPORT_DECL);

impl ImportDecl {
    /// Returns the imported name/path.
    pub fn path(&self) -> Option<QualifiedName> {
        self.syntax.children().find_map(QualifiedName::cast)
    }

    /// Returns true if this is a static import.
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .any(|it| it.kind() == SyntaxKind::STATIC_KW)
    }

    /// Returns true if this is a wildcard import (import foo.*).
    #[must_use]
    pub fn is_wildcard(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .any(|it| it.kind() == SyntaxKind::STAR)
    }
}

// ===== Type Declarations =====

/// A type declaration (class, interface, enum, annotation, or record).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeDecl {
    Class(ClassDecl),
    Interface(InterfaceDecl),
    Enum(EnumDecl),
    Annotation(AnnotationDecl),
    Record(RecordDecl),
}

impl TypeDecl {
    #[must_use]
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::CLASS_DECL => Some(Self::Class(ClassDecl::new(node))),
            SyntaxKind::INTERFACE_DECL => Some(Self::Interface(InterfaceDecl::new(node))),
            SyntaxKind::ENUM_DECL => Some(Self::Enum(EnumDecl::new(node))),
            SyntaxKind::ANNOTATION_DECL => Some(Self::Annotation(AnnotationDecl::new(node))),
            SyntaxKind::RECORD_DECL => Some(Self::Record(RecordDecl::new(node))),
            _ => None,
        }
    }

    #[must_use]
    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Class(it) => it.syntax(),
            Self::Interface(it) => it.syntax(),
            Self::Enum(it) => it.syntax(),
            Self::Annotation(it) => it.syntax(),
            Self::Record(it) => it.syntax(),
        }
    }

    /// Returns the name of this type declaration.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        match self {
            Self::Class(it) => it.name(),
            Self::Interface(it) => it.name(),
            Self::Enum(it) => it.name(),
            Self::Annotation(it) => it.name(),
            Self::Record(it) => it.name(),
        }
    }
}

ast_node!(ClassDecl, CLASS_DECL);

impl ClassDecl {
    /// Returns the class name identifier.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    /// Returns the modifiers (public, abstract, final, etc.).
    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    /// Returns the type parameters (<T, U>), if present.
    pub fn type_params(&self) -> Option<TypeParamList> {
        self.syntax.children().find_map(TypeParamList::cast)
    }

    /// Returns the superclass type, if present (extends Foo).
    #[must_use]
    pub fn superclass(&self) -> Option<Type> {
        let mut take_next_type = false;
        for element in self.syntax.children_with_tokens() {
            if take_next_type {
                if let Some(node) = element.clone().into_node() {
                    if let Some(ty) = Type::cast(node) {
                        return Some(ty);
                    }
                }
            }

            if let Some(token) = element.as_token() {
                if token.kind() == SyntaxKind::EXTENDS_KW {
                    take_next_type = true;
                }
            }
        }
        None
    }

    /// Returns the implemented interfaces (implements Foo, Bar).
    pub fn interfaces(&self) -> impl Iterator<Item = Type> + '_ {
        let mut found_implements = false;
        self.syntax
            .children()
            .filter(move |child| {
                if child.kind() == SyntaxKind::IMPLEMENTS_KW {
                    found_implements = true;
                    false
                } else {
                    found_implements
                }
            })
            .filter_map(Type::cast)
    }

    /// Returns all field declarations.
    pub fn fields(&self) -> impl Iterator<Item = FieldDecl> + '_ {
        self.syntax.children().filter_map(FieldDecl::cast)
    }

    /// Returns all method declarations.
    pub fn methods(&self) -> impl Iterator<Item = MethodDecl> + '_ {
        self.syntax.children().filter_map(MethodDecl::cast)
    }

    /// Returns all constructor declarations.
    pub fn constructors(&self) -> impl Iterator<Item = ConstructorDecl> + '_ {
        self.syntax.children().filter_map(ConstructorDecl::cast)
    }

    /// Returns all nested type declarations.
    pub fn nested_types(&self) -> impl Iterator<Item = TypeDecl> + '_ {
        self.syntax.children().filter_map(TypeDecl::cast)
    }

    /// Returns all initializer blocks (static and instance).
    pub fn initializers(&self) -> impl Iterator<Item = Initializer> + '_ {
        self.syntax.children().filter_map(Initializer::cast)
    }
}

ast_node!(InterfaceDecl, INTERFACE_DECL);

impl InterfaceDecl {
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    pub fn type_params(&self) -> Option<TypeParamList> {
        self.syntax.children().find_map(TypeParamList::cast)
    }

    /// Returns the extended interfaces.
    pub fn extends(&self) -> impl Iterator<Item = Type> + '_ {
        let mut take_types = false;
        self.syntax
            .children_with_tokens()
            .filter_map(move |element| {
                if let Some(token) = element.as_token() {
                    if token.kind() == SyntaxKind::EXTENDS_KW {
                        take_types = true;
                    }
                    None
                } else if take_types {
                    element.into_node().and_then(Type::cast)
                } else {
                    None
                }
            })
    }

    pub fn methods(&self) -> impl Iterator<Item = MethodDecl> + '_ {
        self.syntax.children().filter_map(MethodDecl::cast)
    }

    pub fn fields(&self) -> impl Iterator<Item = FieldDecl> + '_ {
        self.syntax.children().filter_map(FieldDecl::cast)
    }
}

ast_node!(EnumDecl, ENUM_DECL);

impl EnumDecl {
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    /// Returns the implemented interfaces.
    pub fn interfaces(&self) -> impl Iterator<Item = Type> + '_ {
        let mut found_implements = false;
        self.syntax
            .children()
            .filter(move |child| {
                if child.kind() == SyntaxKind::IMPLEMENTS_KW {
                    found_implements = true;
                    false
                } else {
                    found_implements
                }
            })
            .filter_map(Type::cast)
    }

    /// Returns all enum constants.
    pub fn constants(&self) -> impl Iterator<Item = EnumConstant> + '_ {
        self.syntax.children().filter_map(EnumConstant::cast)
    }

    pub fn methods(&self) -> impl Iterator<Item = MethodDecl> + '_ {
        self.syntax.children().filter_map(MethodDecl::cast)
    }

    pub fn fields(&self) -> impl Iterator<Item = FieldDecl> + '_ {
        self.syntax.children().filter_map(FieldDecl::cast)
    }
}

ast_node!(AnnotationDecl, ANNOTATION_DECL);

impl AnnotationDecl {
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    pub fn methods(&self) -> impl Iterator<Item = MethodDecl> + '_ {
        self.syntax.children().filter_map(MethodDecl::cast)
    }
}

ast_node!(RecordDecl, RECORD_DECL);

impl RecordDecl {
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    pub fn type_params(&self) -> Option<TypeParamList> {
        self.syntax.children().find_map(TypeParamList::cast)
    }

    /// Returns the record components (parameters).
    pub fn components(&self) -> Option<ParamList> {
        self.syntax.children().find_map(ParamList::cast)
    }

    pub fn interfaces(&self) -> impl Iterator<Item = Type> + '_ {
        let mut found_implements = false;
        self.syntax
            .children()
            .filter(move |child| {
                if child.kind() == SyntaxKind::IMPLEMENTS_KW {
                    found_implements = true;
                    false
                } else {
                    found_implements
                }
            })
            .filter_map(Type::cast)
    }

    pub fn methods(&self) -> impl Iterator<Item = MethodDecl> + '_ {
        self.syntax.children().filter_map(MethodDecl::cast)
    }
}

// ===== Class Members =====

ast_node!(FieldDecl, FIELD_DECL);

impl FieldDecl {
    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    pub fn ty(&self) -> Option<Type> {
        self.syntax.children().find_map(Type::cast)
    }

    /// Returns the field name(s). Multiple names for declarations like: int x, y, z;
    pub fn names(&self) -> impl Iterator<Item = SyntaxToken> + '_ {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|it| it.kind().is_identifier())
    }

    /// Returns the initializer expression, if present.
    pub fn initializer(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

ast_node!(MethodDecl, METHOD_DECL);

impl MethodDecl {
    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    pub fn type_params(&self) -> Option<TypeParamList> {
        self.syntax.children().find_map(TypeParamList::cast)
    }

    pub fn return_type(&self) -> Option<Type> {
        self.syntax.children().find_map(Type::cast)
    }

    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_identifier())
    }

    pub fn params(&self) -> Option<ParamList> {
        self.syntax.children().find_map(ParamList::cast)
    }

    pub fn throws(&self) -> Option<ThrowsClause> {
        self.syntax.children().find_map(ThrowsClause::cast)
    }

    pub fn body(&self) -> Option<BlockStmt> {
        self.syntax.children().find_map(BlockStmt::cast)
    }

    /// Returns true if this is an abstract method (no body).
    #[must_use]
    pub fn is_abstract(&self) -> bool {
        self.body().is_none()
    }
}

ast_node!(ConstructorDecl, CONSTRUCTOR_DECL);

impl ConstructorDecl {
    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    pub fn type_params(&self) -> Option<TypeParamList> {
        self.syntax.children().find_map(TypeParamList::cast)
    }

    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    pub fn params(&self) -> Option<ParamList> {
        self.syntax.children().find_map(ParamList::cast)
    }

    pub fn throws(&self) -> Option<ThrowsClause> {
        self.syntax.children().find_map(ThrowsClause::cast)
    }

    pub fn body(&self) -> Option<BlockStmt> {
        self.syntax.children().find_map(BlockStmt::cast)
    }
}

ast_node!(EnumConstant, ENUM_CONSTANT);

impl EnumConstant {
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_identifier())
    }

    pub fn args(&self) -> Option<ArgList> {
        self.syntax.children().find_map(ArgList::cast)
    }
}

/// Initializer block (static or instance).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Initializer {
    Static(StaticInit),
    Instance(InstanceInit),
}

impl Initializer {
    #[must_use]
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::STATIC_INIT => Some(Self::Static(StaticInit::new(node))),
            SyntaxKind::INSTANCE_INIT => Some(Self::Instance(InstanceInit::new(node))),
            _ => None,
        }
    }
}

ast_node!(StaticInit, STATIC_INIT);
ast_node!(InstanceInit, INSTANCE_INIT);

// ===== Method Components =====

ast_node!(ParamList, PARAM_LIST);

impl ParamList {
    pub fn params(&self) -> impl Iterator<Item = Param> + '_ {
        self.syntax.children().filter_map(Param::cast)
    }
}

ast_node!(Param, PARAM);

impl Param {
    pub fn modifiers(&self) -> Option<ModifierList> {
        self.syntax.children().find_map(ModifierList::cast)
    }

    pub fn ty(&self) -> Option<Type> {
        self.syntax.children().find_map(Type::cast)
    }

    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_identifier())
    }

    /// Returns true if this is a varargs parameter (...).
    #[must_use]
    pub fn is_varargs(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .any(|it| it.kind() == SyntaxKind::ELLIPSIS)
    }
}

ast_node!(ThrowsClause, THROWS_CLAUSE);

impl ThrowsClause {
    pub fn exceptions(&self) -> impl Iterator<Item = Type> + '_ {
        self.syntax.children().filter_map(Type::cast)
    }
}

ast_node!(TypeParamList, TYPE_PARAM_LIST);

impl TypeParamList {
    pub fn params(&self) -> impl Iterator<Item = TypeParam> + '_ {
        self.syntax.children().filter_map(TypeParam::cast)
    }
}

ast_node!(TypeParam, TYPE_PARAM);

impl TypeParam {
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    pub fn bounds(&self) -> Option<TypeBound> {
        self.syntax.children().find_map(TypeBound::cast)
    }
}

ast_node!(TypeBound, TYPE_BOUND);

impl TypeBound {
    pub fn types(&self) -> impl Iterator<Item = Type> + '_ {
        self.syntax.children().filter_map(Type::cast)
    }
}

// ===== Statements =====

/// A statement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Stmt {
    Block(BlockStmt),
    Expr(ExprStmt),
    If(IfStmt),
    While(WhileStmt),
    DoWhile(DoWhileStmt),
    For(ForStmt),
    ForEach(ForEachStmt),
    Switch(SwitchStmt),
    Try(TryStmt),
    TryWithResources(TryWithResourcesStmt),
    Synchronized(SynchronizedStmt),
    Return(ReturnStmt),
    Throw(ThrowStmt),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Assert(AssertStmt),
    Empty(EmptyStmt),
    Labeled(LabeledStmt),
    ExplicitConstructorInvocation(ExplicitConstructorInvocation),
    LocalVar(LocalVarDecl),
    Yield(YieldStmt),
}

impl Stmt {
    #[must_use]
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::BLOCK_STMT => Some(Self::Block(BlockStmt::new(node))),
            SyntaxKind::EXPR_STMT => Some(Self::Expr(ExprStmt::new(node))),
            SyntaxKind::IF_STMT => Some(Self::If(IfStmt::new(node))),
            SyntaxKind::WHILE_STMT => Some(Self::While(WhileStmt::new(node))),
            SyntaxKind::DO_WHILE_STMT => Some(Self::DoWhile(DoWhileStmt::new(node))),
            SyntaxKind::FOR_STMT => Some(Self::For(ForStmt::new(node))),
            SyntaxKind::FOREACH_STMT => Some(Self::ForEach(ForEachStmt::new(node))),
            SyntaxKind::SWITCH_STMT => Some(Self::Switch(SwitchStmt::new(node))),
            SyntaxKind::TRY_STMT => Some(Self::Try(TryStmt::new(node))),
            SyntaxKind::TRY_WITH_RESOURCES_STMT => {
                Some(Self::TryWithResources(TryWithResourcesStmt::new(node)))
            }
            SyntaxKind::SYNCHRONIZED_STMT => Some(Self::Synchronized(SynchronizedStmt::new(node))),
            SyntaxKind::RETURN_STMT => Some(Self::Return(ReturnStmt::new(node))),
            SyntaxKind::THROW_STMT => Some(Self::Throw(ThrowStmt::new(node))),
            SyntaxKind::BREAK_STMT => Some(Self::Break(BreakStmt::new(node))),
            SyntaxKind::CONTINUE_STMT => Some(Self::Continue(ContinueStmt::new(node))),
            SyntaxKind::ASSERT_STMT => Some(Self::Assert(AssertStmt::new(node))),
            SyntaxKind::EMPTY_STMT => Some(Self::Empty(EmptyStmt::new(node))),
            SyntaxKind::LABELED_STMT => Some(Self::Labeled(LabeledStmt::new(node))),
            SyntaxKind::EXPLICIT_CONSTRUCTOR_INVOCATION => Some(
                Self::ExplicitConstructorInvocation(ExplicitConstructorInvocation::new(node)),
            ),
            SyntaxKind::LOCAL_VAR_DECL => Some(Self::LocalVar(LocalVarDecl::new(node))),
            SyntaxKind::YIELD_STMT => Some(Self::Yield(YieldStmt::new(node))),
            _ => None,
        }
    }

    #[must_use]
    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Block(it) => it.syntax(),
            Self::Expr(it) => it.syntax(),
            Self::If(it) => it.syntax(),
            Self::While(it) => it.syntax(),
            Self::DoWhile(it) => it.syntax(),
            Self::For(it) => it.syntax(),
            Self::ForEach(it) => it.syntax(),
            Self::Switch(it) => it.syntax(),
            Self::Try(it) => it.syntax(),
            Self::TryWithResources(it) => it.syntax(),
            Self::Synchronized(it) => it.syntax(),
            Self::Return(it) => it.syntax(),
            Self::Throw(it) => it.syntax(),
            Self::Break(it) => it.syntax(),
            Self::Continue(it) => it.syntax(),
            Self::Assert(it) => it.syntax(),
            Self::Empty(it) => it.syntax(),
            Self::Labeled(it) => it.syntax(),
            Self::ExplicitConstructorInvocation(it) => it.syntax(),
            Self::LocalVar(it) => it.syntax(),
            Self::Yield(it) => it.syntax(),
        }
    }
}

ast_node!(BlockStmt, BLOCK_STMT);

impl BlockStmt {
    pub fn statements(&self) -> impl Iterator<Item = Stmt> + '_ {
        self.syntax.children().filter_map(Stmt::cast)
    }
}

ast_node!(ExprStmt, EXPR_STMT);

impl ExprStmt {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

ast_node!(IfStmt, IF_STMT);

impl IfStmt {
    pub fn condition(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn then_branch(&self) -> Option<Stmt> {
        self.syntax.children().filter_map(Stmt::cast).nth(0)
    }

    pub fn else_branch(&self) -> Option<Stmt> {
        self.syntax.children().filter_map(Stmt::cast).nth(1)
    }
}

ast_node!(WhileStmt, WHILE_STMT);

impl WhileStmt {
    pub fn condition(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn body(&self) -> Option<Stmt> {
        self.syntax.children().find_map(Stmt::cast)
    }
}

ast_node!(DoWhileStmt, DO_WHILE_STMT);

impl DoWhileStmt {
    pub fn body(&self) -> Option<Stmt> {
        self.syntax.children().find_map(Stmt::cast)
    }

    pub fn condition(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

ast_node!(ForStmt, FOR_STMT);
ast_node!(ForEachStmt, FOREACH_STMT);
ast_node!(SwitchStmt, SWITCH_STMT);

impl SwitchStmt {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    pub fn cases(&self) -> impl Iterator<Item = CaseClause> + '_ {
        self.syntax.children().filter_map(CaseClause::cast)
    }
}

ast_node!(CaseClause, CASE_CLAUSE);
ast_node!(TryStmt, TRY_STMT);

impl TryStmt {
    pub fn body(&self) -> Option<BlockStmt> {
        self.syntax.children().find_map(BlockStmt::cast)
    }

    pub fn catches(&self) -> impl Iterator<Item = CatchClause> + '_ {
        self.syntax.children().filter_map(CatchClause::cast)
    }

    pub fn finally(&self) -> Option<FinallyClause> {
        self.syntax.children().find_map(FinallyClause::cast)
    }
}

ast_node!(CatchClause, CATCH_CLAUSE);
ast_node!(FinallyClause, FINALLY_CLAUSE);
ast_node!(TryWithResourcesStmt, TRY_WITH_RESOURCES_STMT);
ast_node!(SynchronizedStmt, SYNCHRONIZED_STMT);
ast_node!(ReturnStmt, RETURN_STMT);

impl ReturnStmt {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

ast_node!(ThrowStmt, THROW_STMT);

impl ThrowStmt {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }
}

ast_node!(BreakStmt, BREAK_STMT);
ast_node!(ContinueStmt, CONTINUE_STMT);
ast_node!(AssertStmt, ASSERT_STMT);
ast_node!(EmptyStmt, EMPTY_STMT);
ast_node!(LabeledStmt, LABELED_STMT);
ast_node!(
    ExplicitConstructorInvocation,
    EXPLICIT_CONSTRUCTOR_INVOCATION
);
ast_node!(LocalVarDecl, LOCAL_VAR_DECL);

impl LocalVarDecl {
    pub fn ty(&self) -> Option<Type> {
        self.syntax.children().find_map(Type::cast)
    }

    /// Returns the variable name(s). Multiple names for declarations like: int x, y, z;
    pub fn names(&self) -> impl Iterator<Item = SyntaxToken> + '_ {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|it| it.kind().is_identifier())
    }
}

ast_node!(YieldStmt, YIELD_STMT);

// ===== Expressions =====

/// An expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    Binary(BinaryExpr),
    Unary(UnaryExpr),
    Ternary(TernaryExpr),
    Assignment(AssignmentExpr),
    MethodCall(MethodCallExpr),
    FieldAccess(FieldAccessExpr),
    ArrayAccess(ArrayAccessExpr),
    New(NewExpr),
    ArrayInit(ArrayInitExpr),
    Cast(CastExpr),
    Instanceof(InstanceofExpr),
    PatternInstanceof(PatternInstanceofExpr),
    Lambda(LambdaExpr),
    MethodRef(MethodRefExpr),
    ConstructorRef(ConstructorRefExpr),
    Paren(ParenExpr),
    This(ThisExpr),
    Super(SuperExpr),
    ClassLiteral(ClassLiteralExpr),
    Literal(LiteralExpr),
    Name(NameExpr),
    Switch(SwitchExpr),
}

impl Expr {
    #[must_use]
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::BINARY_EXPR => Some(Self::Binary(BinaryExpr::new(node))),
            SyntaxKind::UNARY_EXPR => Some(Self::Unary(UnaryExpr::new(node))),
            SyntaxKind::TERNARY_EXPR => Some(Self::Ternary(TernaryExpr::new(node))),
            SyntaxKind::ASSIGNMENT_EXPR => Some(Self::Assignment(AssignmentExpr::new(node))),
            SyntaxKind::METHOD_CALL_EXPR => Some(Self::MethodCall(MethodCallExpr::new(node))),
            SyntaxKind::FIELD_ACCESS_EXPR => Some(Self::FieldAccess(FieldAccessExpr::new(node))),
            SyntaxKind::ARRAY_ACCESS_EXPR => Some(Self::ArrayAccess(ArrayAccessExpr::new(node))),
            SyntaxKind::NEW_EXPR => Some(Self::New(NewExpr::new(node))),
            SyntaxKind::ARRAY_INIT_EXPR => Some(Self::ArrayInit(ArrayInitExpr::new(node))),
            SyntaxKind::CAST_EXPR => Some(Self::Cast(CastExpr::new(node))),
            SyntaxKind::INSTANCEOF_EXPR => Some(Self::Instanceof(InstanceofExpr::new(node))),
            SyntaxKind::PATTERN_INSTANCEOF_EXPR => {
                Some(Self::PatternInstanceof(PatternInstanceofExpr::new(node)))
            }
            SyntaxKind::LAMBDA_EXPR => Some(Self::Lambda(LambdaExpr::new(node))),
            SyntaxKind::METHOD_REF_EXPR => Some(Self::MethodRef(MethodRefExpr::new(node))),
            SyntaxKind::CONSTRUCTOR_REF_EXPR => {
                Some(Self::ConstructorRef(ConstructorRefExpr::new(node)))
            }
            SyntaxKind::PAREN_EXPR => Some(Self::Paren(ParenExpr::new(node))),
            SyntaxKind::THIS_EXPR => Some(Self::This(ThisExpr::new(node))),
            SyntaxKind::SUPER_EXPR => Some(Self::Super(SuperExpr::new(node))),
            SyntaxKind::CLASS_LITERAL_EXPR => Some(Self::ClassLiteral(ClassLiteralExpr::new(node))),
            SyntaxKind::LITERAL_EXPR => Some(Self::Literal(LiteralExpr::new(node))),
            SyntaxKind::NAME_EXPR => Some(Self::Name(NameExpr::new(node))),
            SyntaxKind::SWITCH_EXPR => Some(Self::Switch(SwitchExpr::new(node))),
            _ => None,
        }
    }

    #[must_use]
    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Binary(it) => it.syntax(),
            Self::Unary(it) => it.syntax(),
            Self::Ternary(it) => it.syntax(),
            Self::Assignment(it) => it.syntax(),
            Self::MethodCall(it) => it.syntax(),
            Self::FieldAccess(it) => it.syntax(),
            Self::ArrayAccess(it) => it.syntax(),
            Self::New(it) => it.syntax(),
            Self::ArrayInit(it) => it.syntax(),
            Self::Cast(it) => it.syntax(),
            Self::Instanceof(it) => it.syntax(),
            Self::PatternInstanceof(it) => it.syntax(),
            Self::Lambda(it) => it.syntax(),
            Self::MethodRef(it) => it.syntax(),
            Self::ConstructorRef(it) => it.syntax(),
            Self::Paren(it) => it.syntax(),
            Self::This(it) => it.syntax(),
            Self::Super(it) => it.syntax(),
            Self::ClassLiteral(it) => it.syntax(),
            Self::Literal(it) => it.syntax(),
            Self::Name(it) => it.syntax(),
            Self::Switch(it) => it.syntax(),
        }
    }
}

ast_node!(BinaryExpr, BINARY_EXPR);

impl BinaryExpr {
    pub fn lhs(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(0)
    }

    pub fn rhs(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(1)
    }

    #[must_use]
    pub fn op(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|t| t.kind().is_operator())
    }
}

ast_node!(UnaryExpr, UNARY_EXPR);

impl UnaryExpr {
    pub fn expr(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    #[must_use]
    pub fn op(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|t| t.kind().is_operator())
    }
}

ast_node!(TernaryExpr, TERNARY_EXPR);

impl TernaryExpr {
    pub fn condition(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(0)
    }

    pub fn then_expr(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(1)
    }

    pub fn else_expr(&self) -> Option<Expr> {
        self.syntax.children().filter_map(Expr::cast).nth(2)
    }
}

ast_node!(AssignmentExpr, ASSIGNMENT_EXPR);
ast_node!(MethodCallExpr, METHOD_CALL_EXPR);

impl MethodCallExpr {
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_identifier())
    }

    pub fn args(&self) -> Option<ArgList> {
        self.syntax.children().find_map(ArgList::cast)
    }
}

ast_node!(FieldAccessExpr, FIELD_ACCESS_EXPR);

impl FieldAccessExpr {
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax.children().find_map(Expr::cast)
    }

    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_identifier())
    }
}

ast_node!(ArrayAccessExpr, ARRAY_ACCESS_EXPR);
ast_node!(NewExpr, NEW_EXPR);
ast_node!(ArrayInitExpr, ARRAY_INIT_EXPR);
ast_node!(CastExpr, CAST_EXPR);
ast_node!(InstanceofExpr, INSTANCEOF_EXPR);
ast_node!(PatternInstanceofExpr, PATTERN_INSTANCEOF_EXPR);
ast_node!(LambdaExpr, LAMBDA_EXPR);
ast_node!(MethodRefExpr, METHOD_REF_EXPR);
ast_node!(ConstructorRefExpr, CONSTRUCTOR_REF_EXPR);
ast_node!(ParenExpr, PAREN_EXPR);
ast_node!(ThisExpr, THIS_EXPR);
ast_node!(SuperExpr, SUPER_EXPR);
ast_node!(ClassLiteralExpr, CLASS_LITERAL_EXPR);
ast_node!(LiteralExpr, LITERAL_EXPR);
ast_node!(NameExpr, NAME_EXPR);
ast_node!(SwitchExpr, SWITCH_EXPR);

// ===== Types =====

/// A type reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Primitive(PrimitiveType),
    Array(ArrayType),
    Class(ClassType),
    Wildcard(WildcardType),
    Var(VarType),
    Qualified(QualifiedType),
    Union(UnionType),
    Intersection(IntersectionType),
}

impl Type {
    #[must_use]
    pub fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            SyntaxKind::PRIMITIVE_TYPE => Some(Self::Primitive(PrimitiveType::new(node))),
            SyntaxKind::ARRAY_TYPE => Some(Self::Array(ArrayType::new(node))),
            SyntaxKind::CLASS_TYPE => Some(Self::Class(ClassType::new(node))),
            SyntaxKind::WILDCARD_TYPE => Some(Self::Wildcard(WildcardType::new(node))),
            SyntaxKind::VAR_TYPE => Some(Self::Var(VarType::new(node))),
            SyntaxKind::QUALIFIED_TYPE => Some(Self::Qualified(QualifiedType::new(node))),
            SyntaxKind::UNION_TYPE => Some(Self::Union(UnionType::new(node))),
            SyntaxKind::INTERSECTION_TYPE => Some(Self::Intersection(IntersectionType::new(node))),
            _ => None,
        }
    }

    #[must_use]
    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Primitive(it) => it.syntax(),
            Self::Array(it) => it.syntax(),
            Self::Class(it) => it.syntax(),
            Self::Wildcard(it) => it.syntax(),
            Self::Var(it) => it.syntax(),
            Self::Qualified(it) => it.syntax(),
            Self::Union(it) => it.syntax(),
            Self::Intersection(it) => it.syntax(),
        }
    }
}

ast_node!(PrimitiveType, PRIMITIVE_TYPE);
ast_node!(ArrayType, ARRAY_TYPE);
ast_node!(ClassType, CLASS_TYPE);

impl ClassType {
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|it| it.kind().is_type_identifier())
    }

    pub fn type_args(&self) -> Option<TypeArgs> {
        self.syntax.children().find_map(TypeArgs::cast)
    }
}

ast_node!(TypeArgs, TYPE_ARGS);

impl TypeArgs {
    pub fn args(&self) -> impl Iterator<Item = Type> + '_ {
        self.syntax.children().filter_map(Type::cast)
    }
}

ast_node!(WildcardType, WILDCARD_TYPE);
ast_node!(VarType, VAR_TYPE);
ast_node!(QualifiedType, QUALIFIED_TYPE);
ast_node!(UnionType, UNION_TYPE);
ast_node!(IntersectionType, INTERSECTION_TYPE);

// ===== Annotations =====

ast_node!(Annotation, ANNOTATION);

impl Annotation {
    pub fn name(&self) -> Option<QualifiedName> {
        self.syntax.children().find_map(QualifiedName::cast)
    }

    pub fn args(&self) -> Option<AnnotationArgList> {
        self.syntax.children().find_map(AnnotationArgList::cast)
    }
}

ast_node!(AnnotationArgList, ANNOTATION_ARG_LIST);
ast_node!(AnnotationArg, ANNOTATION_ARG);

// ===== Modifiers =====

ast_node!(ModifierList, MODIFIER_LIST);

impl ModifierList {
    pub fn modifiers(&self) -> impl Iterator<Item = Modifier> + '_ {
        self.syntax.children().filter_map(Modifier::cast)
    }

    /// Returns true if the specified keyword modifier is present.
    ///
    /// The parser wraps each keyword in a `MODIFIER` node, so the keyword is looked for
    /// inside those as well as directly in the list.
    #[must_use]
    pub fn has_modifier(&self, kind: SyntaxKind) -> bool {
        self.syntax
            .children_with_tokens()
            .any(|it| it.kind() == kind)
            || self.syntax.children().any(|child| {
                child.kind() == SyntaxKind::MODIFIER
                    && child.children_with_tokens().any(|it| it.kind() == kind)
            })
    }

    #[must_use]
    pub fn is_public(&self) -> bool {
        self.has_modifier(SyntaxKind::PUBLIC_KW)
    }

    #[must_use]
    pub fn is_private(&self) -> bool {
        self.has_modifier(SyntaxKind::PRIVATE_KW)
    }

    #[must_use]
    pub fn is_protected(&self) -> bool {
        self.has_modifier(SyntaxKind::PROTECTED_KW)
    }

    #[must_use]
    pub fn is_static(&self) -> bool {
        self.has_modifier(SyntaxKind::STATIC_KW)
    }

    #[must_use]
    pub fn is_final(&self) -> bool {
        self.has_modifier(SyntaxKind::FINAL_KW)
    }

    #[must_use]
    pub fn is_abstract(&self) -> bool {
        self.has_modifier(SyntaxKind::ABSTRACT_KW)
    }
}

ast_node!(Modifier, MODIFIER);

// ===== Names =====

ast_node!(SimpleName, SIMPLE_NAME);
ast_node!(QualifiedName, QUALIFIED_NAME);

impl QualifiedName {
    /// Returns the parts of the qualified name.
    /// For "java.util.List", returns ["java", "util", "List"].
    pub fn parts(&self) -> impl Iterator<Item = SyntaxToken> + '_ {
        self.syntax
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|it| it.kind().is_identifier())
    }

    /// Returns the fully qualified name as a string.
    #[must_use]
    pub fn as_string(&self) -> String {
        self.parts()
            .map(|t| t.text().to_string())
            .collect::<Vec<_>>()
            .join(".")
    }
}

// ===== Argument Lists =====

ast_node!(ArgList, ARG_LIST);

impl ArgList {
    pub fn args(&self) -> impl Iterator<Item = Expr> + '_ {
        self.syntax.children().filter_map(Expr::cast)
    }
}

// ===== Module System =====

ast_node!(ModuleDecl, MODULE_DECL);

impl ModuleDecl {
    pub fn name(&self) -> Option<QualifiedName> {
        self.syntax.children().find_map(QualifiedName::cast)
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.syntax
            .children_with_tokens()
            .any(|it| it.kind() == SyntaxKind::OPEN_KW)
    }
}

ast_node!(RequiresDirective, REQUIRES_DIRECTIVE);
ast_node!(ExportsDirective, EXPORTS_DIRECTIVE);
ast_node!(OpensDirective, OPENS_DIRECTIVE);
ast_node!(UsesDirective, USES_DIRECTIVE);
ast_node!(ProvidesDirective, PROVIDES_DIRECTIVE);

// ===== Traversal Helpers =====
// Note: Rowan's SyntaxNode already provides rich traversal methods including:
// - ancestors(), parent(), children(), descendants()
// - preorder(), preorder_with_tokens()
// - children_with_tokens(), first_child(), last_child()
// See rowan documentation for the full API.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syntax_kind_properties() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(SyntaxKind::LINE_COMMENT.is_trivia());
        assert!(SyntaxKind::CLASS_KW.is_keyword());
        assert!(SyntaxKind::INT_LITERAL.is_literal());
        assert!(SyntaxKind::PLUS.is_operator());
        assert!(SyntaxKind::L_PAREN.is_punctuation());
        assert!(SyntaxKind::CLASS_DECL.is_node());
    }

    #[test]
    fn test_syntax_kind_display() {
        assert_eq!(SyntaxKind::CLASS_KW.to_string(), "class");
        assert_eq!(SyntaxKind::PLUS.to_string(), "+");
        assert_eq!(SyntaxKind::CLASS_DECL.to_string(), "CLASS_DECL");
    }

    #[test]
    fn test_contextual_keywords_are_keywords() {
        // Ensure all contextual keywords are also marked as keywords
        assert!(SyntaxKind::RECORD_KW.is_keyword());
        assert!(SyntaxKind::VAR_KW.is_keyword());
        assert!(SyntaxKind::WHEN_KW.is_keyword());
        assert!(SyntaxKind::YIELD_KW.is_keyword());
        assert!(SyntaxKind::SEALED_KW.is_keyword());
        assert!(SyntaxKind::PERMITS_KW.is_keyword());
        assert!(SyntaxKind::NON_SEALED_KW.is_keyword());
    }

    #[test]
    fn test_contextual_keywords_classification() {
        // Contextual keywords should be identifiers in most contexts
        assert!(SyntaxKind::RECORD_KW.is_identifier());
        assert!(SyntaxKind::VAR_KW.is_identifier());
        assert!(SyntaxKind::WHEN_KW.is_identifier());
        assert!(SyntaxKind::YIELD_KW.is_identifier());

        // These restricted identifiers are not type identifiers.
        assert!(!SyntaxKind::RECORD_KW.is_type_identifier());
        assert!(!SyntaxKind::VAR_KW.is_type_identifier());
        assert!(!SyntaxKind::YIELD_KW.is_type_identifier());
        assert!(!SyntaxKind::SEALED_KW.is_type_identifier());
        assert!(!SyntaxKind::PERMITS_KW.is_type_identifier());

        // Other contextual keywords remain legal TypeIdentifiers (JLS 3.8).
        assert!(SyntaxKind::WHEN_KW.is_type_identifier());
        assert!(SyntaxKind::OPEN_KW.is_type_identifier());
        assert!(SyntaxKind::MODULE_KW.is_type_identifier());
        assert!(SyntaxKind::WITH_KW.is_type_identifier());

        // The hyphenated contextual keyword is never one identifier.
        assert!(!SyntaxKind::NON_SEALED_KW.is_identifier());
        assert!(!SyntaxKind::NON_SEALED_KW.is_type_identifier());

        // Regular identifiers can be both
        assert!(SyntaxKind::IDENT.is_identifier());
        assert!(SyntaxKind::IDENT.is_type_identifier());

        // Hard keywords are neither
        assert!(!SyntaxKind::CLASS_KW.is_identifier());
        assert!(!SyntaxKind::CLASS_KW.is_type_identifier());
    }

    #[test]
    fn test_is_identifier_consistency() {
        // All type identifiers should also be identifiers
        for i in 0..=(SyntaxKind::EOF as u16) {
            let kind = SyntaxKind::from(i);
            if kind.is_type_identifier() {
                assert!(
                    kind.is_identifier(),
                    "{kind:?} is a type_identifier but not an identifier"
                );
            }
        }
    }
}
