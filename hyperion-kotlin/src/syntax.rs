// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! # Concrete Syntax Tree (CST) for Kotlin
//!
//! This module provides a lossless representation of Kotlin source code using the `rowan` library.
//! Following the same architecture as the Java parser, this CST preserves all information from
//! the source, including whitespace, comments, and exact source positions.
//!
//! ## Architecture
//!
//! The CST is built using a layered architecture:
//!
//! ### 1. SyntaxKind - Token and Node Types
//! Defines all possible token types (keywords, operators, literals) and node types
//! (expressions, statements, declarations) in the Kotlin grammar.
//!
//! ### 2. Green Tree (Immutable, Interned)
//! - The underlying storage layer provided by `rowan`
//! - Immutable and structural-shared
//! - Memory efficient through interning
//!
//! ### 3. Red Tree (Lazy, Positional)
//! - SyntaxNode and SyntaxToken provide a view over the green tree
//! - Lazily computed absolute positions
//! - Parent pointers via runtime computation
//!
//! ## Design Principles
//!
//! 1. **Lossless**: Every byte of source is represented
//! 2. **Immutable**: Trees are immutable, modifications create new trees
//! 3. **Efficient**: Structural sharing minimizes memory usage
//! 4. **Error-Resilient**: Can represent partially parsed or malformed code

use rowan::Language;
use std::fmt;

/// The Kotlin language type for rowan.
/// This is a zero-sized type that implements the `Language` trait,
/// parameterizing rowan's generic syntax tree structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KotlinLanguage {}

impl Language for KotlinLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        SyntaxKind::from(raw.0)
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.into())
    }
}

/// All possible syntax kinds in the Kotlin language.
///
/// This enum represents every type of token and node in a Kotlin program.
/// It's divided into several categories:
///
/// - **Keywords**: Kotlin reserved words (fun, val, var, class, etc.)
/// - **Literals**: Numeric, string, boolean, null literals
/// - **Operators**: Arithmetic, logical, bitwise, Kotlin-specific operators
/// - **Punctuation**: Braces, brackets, semicolons, etc.
/// - **Trivia**: Whitespace and comments (preserved for losslessness)
/// - **Node Types**: Composite syntax constructs (expressions, statements, declarations)
/// - **Special**: Error nodes, end-of-file markers
///
/// # Safety Invariants
///
/// This enum uses `#[repr(u16)]` to enable efficient storage and conversion to/from u16.
/// The following invariants MUST be maintained for memory safety:
///
/// 1. **Contiguous Discriminants**: All variants must have contiguous discriminants starting from 0.
///    Do NOT add explicit discriminants (except WHITESPACE = 0 for clarity) as this breaks
///    the From<u16> conversion.
///
/// 2. **Last Variant**: TOMBSTONE must remain the last variant in the enum. The From<u16>
///    implementation validates that incoming u16 values are <= TOMBSTONE's discriminant.
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
    // ========== Trivia (Whitespace and Comments) ==========
    /// Whitespace: spaces, tabs, newlines
    WHITESPACE = 0,
    /// Single-line comment (`//...`)
    LINE_COMMENT,
    /// Block comment (`/* ... */`)
    BLOCK_COMMENT,

    // ========== Keywords ==========
    /// `fun` keyword - function declaration
    FUN_KW,
    /// `val` keyword - immutable variable
    VAL_KW,
    /// `var` keyword - mutable variable
    VAR_KW,
    /// `class` keyword
    CLASS_KW,
    /// `enum` keyword - enum class modifier
    ENUM_KW,
    /// `annotation` keyword - annotation class modifier
    ANNOTATION_KW,
    /// `object` keyword - singleton declaration
    OBJECT_KW,
    /// `interface` keyword
    INTERFACE_KW,
    /// `if` keyword
    IF_KW,
    /// `else` keyword
    ELSE_KW,
    /// `when` keyword - Kotlin's switch expression
    WHEN_KW,
    /// `for` keyword
    FOR_KW,
    /// `while` keyword
    WHILE_KW,
    /// `return` keyword
    RETURN_KW,
    /// `package` keyword
    PACKAGE_KW,
    /// `import` keyword
    IMPORT_KW,
    /// `as` keyword - type aliasing and casting
    AS_KW,
    /// `is` keyword - type checking
    IS_KW,
    /// `in` keyword - containment and iteration
    IN_KW,
    /// `out` keyword - declaration-site variance
    OUT_KW,
    /// `override` keyword
    OVERRIDE_KW,
    /// `open` keyword - allows inheritance
    OPEN_KW,
    /// `abstract` keyword
    ABSTRACT_KW,
    /// `data` keyword - data class
    DATA_KW,
    /// `sealed` keyword - sealed class
    SEALED_KW,
    /// `internal` keyword - module visibility
    INTERNAL_KW,
    /// `private` keyword
    PRIVATE_KW,
    /// `public` keyword
    PUBLIC_KW,
    /// `protected` keyword
    PROTECTED_KW,
    /// `suspend` keyword - coroutine function
    SUSPEND_KW,
    /// `inline` keyword
    INLINE_KW,
    /// `noinline` keyword
    NOINLINE_KW,
    /// `crossinline` keyword
    CROSSINLINE_KW,
    /// `vararg` keyword
    VARARG_KW,
    /// `operator` keyword
    OPERATOR_KW,
    /// `infix` keyword
    INFIX_KW,
    /// `companion` keyword
    COMPANION_KW,
    /// `init` keyword - initializer block
    INIT_KW,
    /// `constructor` keyword
    CONSTRUCTOR_KW,
    /// `this` keyword
    THIS_KW,
    /// `super` keyword
    SUPER_KW,
    /// `true` literal
    TRUE_KW,
    /// `false` literal
    FALSE_KW,
    /// `null` literal
    NULL_KW,
    /// `try` keyword
    TRY_KW,
    /// `catch` keyword
    CATCH_KW,
    /// `finally` keyword
    FINALLY_KW,
    /// `throw` keyword
    THROW_KW,
    /// `break` keyword
    BREAK_KW,
    /// `continue` keyword
    CONTINUE_KW,
    /// `lateinit` keyword - late initialization
    LATEINIT_KW,
    /// `by` keyword - delegation
    BY_KW,
    /// `typealias` keyword - type alias
    TYPEALIAS_KW,
    /// `expect` keyword - multiplatform expect
    EXPECT_KW,
    /// `actual` keyword - multiplatform actual
    ACTUAL_KW,
    /// `do` keyword - do-while loop
    DO_KW,
    /// `where` keyword - generic type constraints
    WHERE_KW,
    /// `reified` keyword - reified generic type parameters (soft keyword)
    REIFIED_KW,

    // ========== Operators ==========
    /// `+` operator
    PLUS,
    /// `-` operator
    MINUS,
    /// `*` operator
    STAR,
    /// `/` operator
    SLASH,
    /// `%` operator (modulo)
    PERCENT,
    /// `=` operator (assignment)
    EQ,
    /// `==` operator (equality)
    EQEQ,
    /// `!=` operator (inequality)
    BANGEQ,
    /// `===` operator (referential equality)
    EQEQEQ,
    /// `!==` operator (referential inequality)
    BANGEQEQ,
    /// `<` operator
    LT,
    /// `>` operator
    GT,
    /// `<=` operator
    LTEQ,
    /// `>=` operator
    GTEQ,
    /// `&&` operator (logical AND)
    AMPAMP,
    /// `||` operator (logical OR)
    PIPEPIPE,
    /// `!` operator (logical NOT)
    BANG,
    /// `->` arrow - lambda and when
    ARROW,
    /// `=>` double arrow
    DOUBLE_ARROW,
    /// `?:` Elvis operator - null coalescing
    ELVIS,
    /// `?.` safe call operator
    SAFE_CALL,
    /// `!!` not-null assertion
    NOT_NULL,
    /// `..` range operator
    RANGE,
    /// `..<` range until operator
    RANGE_UNTIL,
    /// `++` increment
    PLUSPLUS,
    /// `--` decrement
    MINUSMINUS,
    /// `+=` compound assignment
    PLUSEQ,
    /// `-=` compound assignment
    MINUSEQ,
    /// `*=` compound assignment
    STAREQ,
    /// `/=` compound assignment
    SLASHEQ,
    /// `%=` compound assignment
    PERCENTEQ,
    /// `&` bitwise AND
    AMP,
    /// `|` bitwise OR
    PIPE,
    /// `^` bitwise XOR
    CARET,
    /// `~` bitwise NOT
    TILDE,
    /// `::` member reference
    COLONCOLON,
    /// `!in` operator - negated containment check
    NOT_IN,
    /// `!is` operator - negated type check
    NOT_IS,
    /// `as?` operator - safe cast
    SAFE_CAST,

    // ========== Delimiters ==========
    /// `(` left parenthesis
    L_PAREN,
    /// `)` right parenthesis
    R_PAREN,
    /// `{` left brace
    L_BRACE,
    /// `}` right brace
    R_BRACE,
    /// `[` left bracket
    L_BRACKET,
    /// `]` right bracket
    R_BRACKET,
    /// `;` semicolon
    SEMICOLON,
    /// `:` colon
    COLON,
    /// `,` comma
    COMMA,
    /// `.` dot
    DOT,
    /// `?` question mark - nullable type
    QUESTION,
    /// `<` angle bracket (for generics)
    L_ANGLE,
    /// `>` angle bracket (for generics)
    R_ANGLE,
    /// `@` at sign - annotations
    AT,

    // ========== Literals ==========
    /// Integer literal (e.g., `42`, `0x2A`, `0b101010`)
    INT_LITERAL,
    /// Long literal (e.g., `42L`)
    LONG_LITERAL,
    /// Float literal (e.g., `3.14f`)
    FLOAT_LITERAL,
    /// Double literal (e.g., `3.14`)
    DOUBLE_LITERAL,
    /// String literal (e.g., `"hello"`)
    STRING_LITERAL,
    /// Character literal (e.g., `'a'`)
    CHAR_LITERAL,
    /// Raw string literal (e.g., `"""triple quoted"""`)
    RAW_STRING_LITERAL,

    // ========== Identifiers ==========
    /// Identifier (variable, function, class names)
    IDENT,
    /// Backtick-quoted identifier (e.g., `` `fun` ``)
    BACKTICK_IDENT,

    // ========== Composite Nodes ==========
    /// Root node of the syntax tree
    SOURCE_FILE,
    /// Function declaration (`fun name() { }`)
    FUNCTION_DECL,
    /// Property declaration (`val/var name: Type = value`)
    PROPERTY_DECL,
    /// Class declaration (`class Name { }`)
    CLASS_DECL,
    /// Object declaration (`object Name { }`)
    OBJECT_DECL,
    /// Interface declaration (`interface Name { }`)
    INTERFACE_DECL,
    /// Parameter list in functions
    PARAMETER_LIST,
    /// Single parameter
    PARAMETER,
    /// Type reference
    TYPE_REF,
    /// Nullable type (e.g., `String?`)
    NULLABLE_TYPE,
    /// Code block (`{ ... }`)
    BLOCK,
    /// If expression (`if (cond) { } else { }`)
    IF_EXPR,
    /// When expression (`when (x) { ... }`)
    WHEN_EXPR,
    /// When entry (`value -> result`)
    WHEN_ENTRY,
    /// For expression (`for (x in items) { }`)
    FOR_EXPR,
    /// For loop variable binding (`x in items` or `(a, b) in pairs`)
    FOR_LOOP_VAR,
    /// While expression (`while (cond) { }`)
    WHILE_EXPR,
    /// Lambda expression (`{ x -> x + 1 }`)
    LAMBDA_EXPR,
    /// Lambda parameter list
    LAMBDA_PARAMETERS,
    /// Function call (`foo(args)`)
    CALL_EXPR,
    /// Literal expression (string, int, boolean, null, etc.)
    LITERAL_EXPR,
    /// Binary expression (`a + b`)
    BINARY_EXPR,
    /// Unary expression (`!a`, `-a`)
    UNARY_EXPR,
    /// Parenthesized expression (`(expr)`)
    PAREN_EXPR,
    /// Array access (`arr[index]`)
    ARRAY_ACCESS_EXPR,
    /// Member access (`obj.member`)
    MEMBER_ACCESS_EXPR,
    /// Safe member access (`obj?.member`)
    SAFE_ACCESS_EXPR,
    /// Return expression (`return value`)
    RETURN_EXPR,
    /// Break expression
    BREAK_EXPR,
    /// Continue expression
    CONTINUE_EXPR,
    /// Throw expression
    THROW_EXPR,
    /// Try expression
    TRY_EXPR,
    /// Catch clause
    CATCH_CLAUSE,
    /// Finally clause
    FINALLY_CLAUSE,
    /// Annotation (`@Annotation`)
    ANNOTATION,
    /// Annotation argument list (`(arg1, arg2)`)
    ANNOTATION_ARG_LIST,
    /// Annotation argument (`value = "foo"` or just `"foo"`)
    ANNOTATION_ARG,
    /// Annotation array value (`[val1, val2]`)
    ANNOTATION_ARRAY_VALUE,
    /// Package declaration
    PACKAGE_DECL,
    /// Import declaration
    IMPORT_DECL,
    /// Modifiers list
    MODIFIERS,
    /// Type parameter (`<T>`)
    TYPE_PARAMETER,
    /// Type parameter list (`<T, U>`)
    TYPE_PARAMETER_LIST,
    /// Type argument list (`<String, Int>`)
    TYPE_ARGUMENT_LIST,
    /// Class body
    CLASS_BODY,
    /// Property accessor (getter/setter)
    PROPERTY_ACCESSOR,
    /// Companion object
    COMPANION_OBJECT,
    /// Initializer block (`init { }`)
    INIT_BLOCK,
    /// An entry of an enum body: `A`, `B(1)`, `C { override fun f() = 2 }`
    ENUM_ENTRY,
    /// Constructor declaration
    CONSTRUCTOR_DECL,
    /// Function type (e.g., `(String) -> Int` or `() -> Unit`)
    FUNCTION_TYPE,
    /// Expression statement
    EXPR_STMT,
    /// Type cast expression (`x as String`, `x as? String`)
    AS_EXPR,
    /// Destructuring declaration (`val (a, b) = pair`)
    DESTRUCTURING_DECL,
    /// Where clause for generic type constraints
    WHERE_CLAUSE,
    /// Individual type constraint in a where clause
    TYPE_CONSTRAINT,
    /// Do-while expression (`do { } while (cond)`)
    DO_WHILE_EXPR,
    /// Type alias declaration (`typealias Name = Type`)
    TYPEALIAS_DECL,

    // ========== Special ==========
    /// Error token/node for error recovery
    ERROR,
    /// End of file marker
    EOF,
    /// Tombstone marker for deleted nodes
    TOMBSTONE,
}

impl From<u16> for SyntaxKind {
    fn from(value: u16) -> Self {
        // Validate the discriminant is within the valid range
        // TOMBSTONE is the last variant in the enum
        assert!(
            value <= SyntaxKind::TOMBSTONE as u16,
            "Invalid SyntaxKind discriminant: {}. Max valid value is {}",
            value,
            SyntaxKind::TOMBSTONE as u16
        );

        // Safety: We've validated that the discriminant is within the valid range
        // of the enum. This is a repr(u16) enum with contiguous discriminants
        // starting from 0, so any value <= TOMBSTONE is a valid SyntaxKind variant.
        unsafe { std::mem::transmute(value) }
    }
}

impl From<SyntaxKind> for u16 {
    fn from(kind: SyntaxKind) -> Self {
        kind as u16
    }
}

impl fmt::Display for SyntaxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl SyntaxKind {
    /// Returns true if this is a trivia token (whitespace or comment)
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        )
    }

    /// Returns true if this is a comment token (line or block comment).
    #[inline]
    pub const fn is_comment(self) -> bool {
        matches!(self, SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT)
    }

    /// Returns true if this is a string literal token (regular or raw).
    #[inline]
    pub const fn is_string_literal(self) -> bool {
        matches!(
            self,
            SyntaxKind::STRING_LITERAL | SyntaxKind::RAW_STRING_LITERAL
        )
    }

    /// Returns true if this token is non-code (comment or string literal).
    /// Useful for skipping LSP operations on tokens that aren't navigable code.
    /// Does NOT include whitespace — use `is_trivia()` for that.
    #[inline]
    pub const fn is_non_code(self) -> bool {
        self.is_comment() || self.is_string_literal()
    }

    /// Returns true if this is a keyword
    pub const fn is_keyword(self) -> bool {
        matches!(
            self,
            SyntaxKind::FUN_KW
                | SyntaxKind::VAL_KW
                | SyntaxKind::VAR_KW
                | SyntaxKind::CLASS_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::ANNOTATION_KW
                | SyntaxKind::OBJECT_KW
                | SyntaxKind::INTERFACE_KW
                | SyntaxKind::IF_KW
                | SyntaxKind::ELSE_KW
                | SyntaxKind::WHEN_KW
                | SyntaxKind::FOR_KW
                | SyntaxKind::WHILE_KW
                | SyntaxKind::RETURN_KW
                | SyntaxKind::PACKAGE_KW
                | SyntaxKind::IMPORT_KW
                | SyntaxKind::AS_KW
                | SyntaxKind::IS_KW
                | SyntaxKind::IN_KW
                | SyntaxKind::OUT_KW
                | SyntaxKind::OVERRIDE_KW
                | SyntaxKind::OPEN_KW
                | SyntaxKind::ABSTRACT_KW
                | SyntaxKind::DATA_KW
                | SyntaxKind::SEALED_KW
                | SyntaxKind::INTERNAL_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PUBLIC_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::SUSPEND_KW
                | SyntaxKind::INLINE_KW
                | SyntaxKind::OPERATOR_KW
                | SyntaxKind::INFIX_KW
                | SyntaxKind::COMPANION_KW
                | SyntaxKind::INIT_KW
                | SyntaxKind::CONSTRUCTOR_KW
                | SyntaxKind::THIS_KW
                | SyntaxKind::SUPER_KW
                | SyntaxKind::TRUE_KW
                | SyntaxKind::FALSE_KW
                | SyntaxKind::NULL_KW
                | SyntaxKind::TRY_KW
                | SyntaxKind::CATCH_KW
                | SyntaxKind::FINALLY_KW
                | SyntaxKind::THROW_KW
                | SyntaxKind::BREAK_KW
                | SyntaxKind::CONTINUE_KW
                | SyntaxKind::LATEINIT_KW
                | SyntaxKind::BY_KW
                | SyntaxKind::TYPEALIAS_KW
                | SyntaxKind::EXPECT_KW
                | SyntaxKind::ACTUAL_KW
                | SyntaxKind::DO_KW
                | SyntaxKind::WHERE_KW
                | SyntaxKind::REIFIED_KW
        )
    }

    /// Returns true if this is a literal
    pub const fn is_literal(self) -> bool {
        matches!(
            self,
            SyntaxKind::INT_LITERAL
                | SyntaxKind::LONG_LITERAL
                | SyntaxKind::FLOAT_LITERAL
                | SyntaxKind::DOUBLE_LITERAL
                | SyntaxKind::STRING_LITERAL
                | SyntaxKind::CHAR_LITERAL
                | SyntaxKind::RAW_STRING_LITERAL
                | SyntaxKind::TRUE_KW
                | SyntaxKind::FALSE_KW
                | SyntaxKind::NULL_KW
        )
    }
}

/// Type alias for Kotlin syntax nodes
pub type SyntaxNode = rowan::SyntaxNode<KotlinLanguage>;

/// Type alias for Kotlin syntax tokens
pub type SyntaxToken = rowan::SyntaxToken<KotlinLanguage>;

/// Type alias for Kotlin syntax elements (node or token)
pub type SyntaxElement = rowan::SyntaxElement<KotlinLanguage>;
