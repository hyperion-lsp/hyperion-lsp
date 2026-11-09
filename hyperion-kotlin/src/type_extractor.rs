// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::SyntaxKind;
/// Type extraction from Kotlin AST
///
/// This module provides utilities to extract type information from Kotlin syntax nodes.
/// It supports simple types, generic types, nullable types, and function types.
use crate::SyntaxNode;

/// Represents a Kotlin type
///
/// **DEPRECATED**: This enum is being replaced by `crate::types::KotlinType` which provides
/// a more comprehensive type representation with better support for generics, variance,
/// and nullability. Use `KotlinType` for new code. The `From<Type> for KotlinType`
/// conversion is available for interop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Simple type: "String", "Int", "User"
    Simple(String),

    /// Generic type: "List<String>" = Generic("List", [Simple("String")])
    Generic(String, Vec<Type>),

    /// Nullable type: "String?" = Nullable(Simple("String"))
    Nullable(Box<Type>),

    /// Function type: "(Int, String) -> Boolean"
    Function(Vec<Type>, Box<Type>),

    /// Unknown/inferred type
    Unknown,
}

impl Type {
    /// Format the type as a Kotlin type string
    pub fn format(&self) -> String {
        match self {
            Type::Simple(name) => name.clone(),
            Type::Generic(name, args) => {
                let args_str = args.iter().map(Type::format).collect::<Vec<_>>().join(", ");
                format!("{name}<{args_str}>")
            }
            Type::Nullable(inner) => format!("{}?", inner.format()),
            Type::Function(params, ret) => {
                let params_str = params
                    .iter()
                    .map(Type::format)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({}) -> {}", params_str, ret.format())
            }
            Type::Unknown => "?".to_string(),
        }
    }

    /// Get the simple name of the type (without generics or nullability)
    ///
    /// For generic types like `List<String>`, returns `"List"`.
    /// For nullable types like `String?`, returns `"String"`.
    /// For function types, returns `"Function"`.
    pub fn simple_name(&self) -> String {
        match self {
            Type::Simple(name) | Type::Generic(name, _) => name.clone(),
            Type::Nullable(inner) => inner.simple_name(),
            Type::Function(_, _) => "Function".to_string(),
            Type::Unknown => "Unknown".to_string(),
        }
    }
}

/// Extracts type information from Kotlin AST nodes
pub struct TypeExtractor;

impl TypeExtractor {
    /// Extract type from a TYPE_REF or NULLABLE_TYPE node
    pub fn extract(node: &SyntaxNode) -> Type {
        match node.kind() {
            SyntaxKind::NULLABLE_TYPE => Self::extract_nullable(node),
            SyntaxKind::TYPE_REF => Self::extract_type_ref(node),
            _ => {
                // Try to extract from children
                for child in node.children() {
                    let child_type = Self::extract(&child);
                    if child_type != Type::Unknown {
                        return child_type;
                    }
                }
                Type::Unknown
            }
        }
    }

    /// Infer type from a literal token
    ///
    /// Maps literal tokens to their corresponding Kotlin types:
    /// - INT_LITERAL -> Int
    /// - LONG_LITERAL -> Long
    /// - FLOAT_LITERAL -> Float
    /// - DOUBLE_LITERAL -> Double
    /// - STRING_LITERAL, RAW_STRING_LITERAL -> String
    /// - CHAR_LITERAL -> Char
    /// - TRUE_KW, FALSE_KW -> Boolean
    /// - NULL_KW -> Nothing? (nullable Nothing)
    pub fn infer_literal_type(token_kind: SyntaxKind, _text: &str) -> Option<Type> {
        match token_kind {
            SyntaxKind::INT_LITERAL => Some(Type::Simple("Int".to_string())),
            SyntaxKind::LONG_LITERAL => Some(Type::Simple("Long".to_string())),
            SyntaxKind::FLOAT_LITERAL => Some(Type::Simple("Float".to_string())),
            SyntaxKind::DOUBLE_LITERAL => Some(Type::Simple("Double".to_string())),
            SyntaxKind::STRING_LITERAL | SyntaxKind::RAW_STRING_LITERAL => {
                Some(Type::Simple("String".to_string()))
            }
            SyntaxKind::CHAR_LITERAL => Some(Type::Simple("Char".to_string())),
            SyntaxKind::TRUE_KW | SyntaxKind::FALSE_KW => Some(Type::Simple("Boolean".to_string())),
            SyntaxKind::NULL_KW => {
                // null is of type Nothing? (nullable Nothing)
                Some(Type::Nullable(Box::new(Type::Simple(
                    "Nothing".to_string(),
                ))))
            }
            _ => None,
        }
    }

    /// Infer type from a literal expression node
    ///
    /// Handles both single literals and literals with prefix operators (e.g., -5 still -> Int)
    /// Only looks at direct children tokens, not nested expressions.
    pub fn infer_from_literal_expr(node: &SyntaxNode) -> Option<Type> {
        // Look for literal tokens in the node's direct children only
        // Don't recurse into child nodes to avoid picking up literals from constructor args
        for elem in node.children_with_tokens() {
            if let Some(token) = elem.as_token() {
                // Skip whitespace and operators
                if token.kind().is_trivia() {
                    continue;
                }
                // If we find a literal, return its type
                if let Some(ty) = Self::infer_literal_type(token.kind(), token.text()) {
                    return Some(ty);
                }
                // If we find a non-literal, non-trivia token (like IDENT), this is not a literal expr
                if matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::L_PAREN) {
                    return None;
                }
            }
        }
        None
    }

    /// Infer type from an initializer expression
    ///
    /// This is a simplified version that handles common patterns:
    /// - Constructor calls (User(), ArrayList<String>()) - checked first
    /// - Binary expressions that contain generic constructor patterns
    /// - Literals (5, "hello", true, etc.) - fallback
    ///
    /// More complex patterns (method calls, etc.) are handled by other functions.
    pub fn infer_from_initializer(node: &SyntaxNode) -> Option<Type> {
        // Try constructor call inference first (more specific than literals)
        if let Some(ty) = Self::infer_from_constructor_call(node) {
            return Some(ty);
        }

        // Handle BINARY_EXPR that might be a generic constructor call (ArrayList<String>())
        // The parser treats < as a binary operator, so ArrayList<String>() becomes a BINARY_EXPR
        if node.kind() == SyntaxKind::BINARY_EXPR {
            if let Some(ty) = Self::infer_generic_constructor_from_binary_expr(node) {
                return Some(ty);
            }
        }

        // Try literal inference as fallback
        if let Some(ty) = Self::infer_from_literal_expr(node) {
            return Some(ty);
        }

        // TODO: Add more inference patterns (method calls, etc.)
        None
    }

    /// Infer type from a method call expression
    ///
    /// This function attempts to extract the method name and receiver from a method call.
    /// The actual type resolution requires workspace lookup, which is done in the scope builder.
    ///
    /// Returns: (receiver_name, method_name) or None if not a method call
    ///
    /// Patterns:
    /// - user.getName() -> Some(("user", "getName"))
    /// - service.fetch() -> Some(("service", "fetch"))
    /// - getUserById(123) -> Some(("", "getUserById")) // no receiver = function call
    pub fn extract_method_call_info(node: &SyntaxNode) -> Option<(String, String)> {
        // Look for pattern: [IDENT DOT] IDENT L_PAREN
        let mut receiver = String::new();
        let mut method_name = String::new();
        let mut last_ident = String::new();
        let mut seen_dot = false;

        for elem in node.children_with_tokens() {
            if let Some(token) = elem.as_token() {
                match token.kind() {
                    SyntaxKind::IDENT => {
                        if seen_dot {
                            // This is the method name after the dot
                            method_name = token.text().to_string();
                        } else {
                            // This might be the receiver, or just a function call
                            last_ident = token.text().to_string();
                        }
                    }
                    SyntaxKind::DOT if !last_ident.is_empty() => {
                        // The last ident we saw is the receiver
                        receiver.clone_from(&last_ident);
                        seen_dot = true;
                    }
                    SyntaxKind::L_PAREN => {
                        // End of method call pattern
                        if !method_name.is_empty() {
                            // We have receiver.method() pattern
                            return Some((receiver, method_name));
                        } else if !last_ident.is_empty() {
                            // We have method() pattern (no receiver)
                            return Some((String::new(), last_ident));
                        }
                        break;
                    }
                    _ => {}
                }
            }
        }

        None
    }

    /// Infer type from a constructor call
    ///
    /// Patterns:
    /// - User() -> User
    /// - ArrayList<String>() -> ArrayList<String>
    /// - HashMap<String, Int>() -> HashMap<String, Int>
    /// - OuterClass.InnerClass() -> OuterClass.InnerClass
    pub fn infer_from_constructor_call(node: &SyntaxNode) -> Option<Type> {
        // Look for pattern: IDENT [TYPE_ARGUMENT_LIST] L_PAREN or IDENT < TYPE > ( )
        let mut type_name_parts = Vec::new();
        let mut type_args: Vec<Type> = Vec::new();
        let mut found_paren = false;
        let mut in_generics = false;
        let mut type_args_from_node: Option<Vec<Type>> = None;

        for elem in node.children_with_tokens() {
            match elem {
                rowan::NodeOrToken::Token(token) => {
                    match token.kind() {
                        SyntaxKind::IDENT => {
                            // Only collect IDENTs before we see any < operator (generics)
                            if !in_generics && type_args_from_node.is_none() {
                                type_name_parts.push(token.text().to_string());
                            } else if in_generics {
                                // Collect type arg IDENTs (e.g., "String" in ArrayList<String>)
                                type_args.push(Type::Simple(token.text().to_string()));
                            }
                        }
                        SyntaxKind::LT => {
                            // Mark that we've entered generic parameters
                            in_generics = true;
                        }
                        SyntaxKind::GT => {
                            // Mark that we've exited generic parameters
                            in_generics = false;
                        }
                        SyntaxKind::L_PAREN => {
                            found_paren = true;
                            break;
                        }
                        SyntaxKind::DOT if !in_generics => {
                            // Allow qualified names: OuterClass.InnerClass()
                        }
                        _ => {}
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::TYPE_ARGUMENT_LIST {
                        type_args_from_node = Some(Self::extract_type_args(&n));
                    }
                }
            }
        }

        // Must have found a type name and a left paren to be a constructor call
        if !found_paren || type_name_parts.is_empty() {
            return None;
        }

        let type_name = type_name_parts.join(".");

        // Heuristic: In Kotlin, constructor calls typically start with uppercase
        // (e.g., User(), ArrayList<String>()) while function calls start with lowercase
        // (e.g., listOf(), println()). Without semantic analysis, we use this convention
        // to distinguish constructor calls from function calls.
        if let Some(first_char) = type_name.chars().next() {
            if !first_char.is_uppercase() {
                return None;
            }
        }

        // Build the type - prefer type_args_from_node if available, otherwise use collected tokens
        if let Some(args) = type_args_from_node {
            if !args.is_empty() {
                return Some(Type::Generic(type_name, args));
            }
        }

        if type_args.is_empty() {
            Some(Type::Simple(type_name))
        } else {
            Some(Type::Generic(type_name, type_args))
        }
    }

    /// Infer generic constructor call from a BINARY_EXPR node
    ///
    /// Handles the pattern: IDENT LT TYPE_IDENT GT L_PAREN R_PAREN
    /// This occurs because the parser treats < as a binary operator.
    ///
    /// Example AST for `ArrayList<String>()`:
    /// BINARY_EXPR
    ///   IDENT = "ArrayList"
    ///   LT = "<"
    ///   IDENT = "String"
    ///   GT = ">"
    ///   L_PAREN = "("
    ///   R_PAREN = ")"
    fn infer_generic_constructor_from_binary_expr(node: &SyntaxNode) -> Option<Type> {
        let mut type_name = String::new();
        let mut type_args = Vec::new();
        let mut in_type_args = false;
        let mut found_lparen = false;

        // Helper to collect tokens from nested structure
        fn collect_tokens(
            node: &SyntaxNode,
            type_name: &mut String,
            type_args: &mut Vec<Type>,
            in_type_args: &mut bool,
            found_lparen: &mut bool,
        ) {
            for elem in node.children_with_tokens() {
                if *found_lparen {
                    break;
                }
                match elem {
                    rowan::NodeOrToken::Token(token) => match token.kind() {
                        SyntaxKind::IDENT => {
                            if type_name.is_empty() {
                                // First IDENT is the class name
                                *type_name = token.text().to_string();
                            } else if *in_type_args {
                                // IDENT inside < > is a type argument
                                type_args.push(Type::Simple(token.text().to_string()));
                            }
                        }
                        SyntaxKind::LT => {
                            *in_type_args = true;
                        }
                        SyntaxKind::GT => {
                            *in_type_args = false;
                        }
                        SyntaxKind::L_PAREN => {
                            *found_lparen = true;
                        }
                        _ => {}
                    },
                    rowan::NodeOrToken::Node(n) => {
                        // Recurse into nested BINARY_EXPR and PAREN_EXPR
                        if matches!(n.kind(), SyntaxKind::BINARY_EXPR | SyntaxKind::PAREN_EXPR) {
                            collect_tokens(&n, type_name, type_args, in_type_args, found_lparen);
                        }
                    }
                }
            }
        }

        collect_tokens(
            node,
            &mut type_name,
            &mut type_args,
            &mut in_type_args,
            &mut found_lparen,
        );

        // Verify we found a valid generic constructor pattern
        if type_name.is_empty() || type_args.is_empty() || !found_lparen {
            return None;
        }

        Some(Type::Generic(type_name, type_args))
    }

    /// Infer constructor type from a sequence of tokens
    ///
    /// Used when the AST doesn't provide a proper expression node (e.g., in local variable declarations)
    /// Pattern: IDENT [< TYPE_ARGS >] (
    pub fn infer_from_token_sequence(
        tokens: &[rowan::SyntaxElement<crate::KotlinLanguage>],
    ) -> Option<Type> {
        let mut type_name_parts = Vec::new();
        let mut generic_depth = 0;
        let mut generic_arg_text = String::new();
        let mut found_paren = false;

        for elem in tokens {
            if let Some(token) = elem.as_token() {
                match token.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::DOT if generic_depth == 0 => {
                        // Skip whitespace and dots (qualified names)
                    }
                    SyntaxKind::IDENT if generic_depth == 0 => {
                        type_name_parts.push(token.text().to_string());
                    }
                    SyntaxKind::L_PAREN if generic_depth == 0 => {
                        found_paren = true;
                        break;
                    }
                    SyntaxKind::LT => {
                        generic_depth += 1;
                        generic_arg_text.push('<');
                    }
                    SyntaxKind::GT => {
                        generic_depth -= 1;
                        generic_arg_text.push('>');
                    }
                    _ if generic_depth > 0 => {
                        // Collect everything inside < >
                        generic_arg_text.push_str(token.text());
                    }
                    _ => {}
                }
            }
        }

        // Must have found a type name and a left paren to be a constructor call
        if !found_paren || type_name_parts.is_empty() {
            return None;
        }

        let type_name = type_name_parts.join(".");

        // Parse generic arguments if we collected any
        if generic_arg_text.is_empty() {
            Some(Type::Simple(type_name))
        } else {
            // For now, store the full generic string
            // A more sophisticated parser would extract individual type arguments
            Some(Type::Simple(format!("{type_name}{generic_arg_text}")))
        }
    }

    /// Extract type from a parameter node (PARAMETER)
    pub fn extract_from_parameter(param_node: &SyntaxNode) -> Option<Type> {
        // Look for TYPE_REF or NULLABLE_TYPE child
        for child in param_node.children() {
            match child.kind() {
                SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE => {
                    return Some(Self::extract(&child));
                }
                _ => {}
            }
        }
        None
    }

    /// Extract return type from a function declaration
    pub fn extract_return_type(fun_decl: &SyntaxNode) -> Type {
        let mut past_param_list = false;

        // Look for type after PARAMETER_LIST
        for child in fun_decl.children() {
            if child.kind() == SyntaxKind::PARAMETER_LIST {
                past_param_list = true;
                continue;
            }

            if past_param_list {
                match child.kind() {
                    SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE => {
                        return Self::extract(&child);
                    }
                    _ => {}
                }
            }
        }

        // No return type specified - defaults to Unit
        Type::Simple("Unit".to_string())
    }

    /// Extract type from a property declaration (PROPERTY_DECL)
    pub fn extract_from_property(prop_decl: &SyntaxNode) -> Option<Type> {
        // Look for TYPE_REF or NULLABLE_TYPE child
        for child in prop_decl.children() {
            match child.kind() {
                SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE => {
                    return Some(Self::extract(&child));
                }
                _ => {}
            }
        }
        None
    }

    /// Infer type from a property/val declaration without explicit type annotation
    ///
    /// Handles patterns like:
    /// - val x = 5                      -> Int
    /// - val s = "hello"                -> String
    /// - val user = User()              -> User
    /// - val list = ArrayList<String>() -> ArrayList<String>
    /// - val empty = null               -> Nothing?
    ///
    /// This looks for the initializer expression after the `=` token
    /// and infers its type using available inference methods.
    pub fn infer_val_type(prop_decl: &SyntaxNode) -> Option<Type> {
        // Check if there's an explicit type annotation first
        // If there is, use that instead of inferring
        if let Some(explicit_type) = Self::extract_from_property(prop_decl) {
            return Some(explicit_type);
        }

        // Scan the initializer as a flat token stream.
        //
        // The initializer reaches us in two shapes. Local `val`s used to be
        // parsed as a bare run of tokens directly under `PROPERTY_DECL`, while
        // top-level and class properties went through the expression parser and
        // came out as structured nodes (`CALL_EXPR`, `LITERAL_EXPR`,
        // `MEMBER_ACCESS_EXPR`, …). Local `val`s now use the expression parser
        // too, so this scan flattens nodes away and works on the token stream
        // that both shapes share.
        let flat = Self::flat_initializer_tokens(prop_decl);

        let mut prev_was_dot = false;
        for (i, token) in flat.iter().enumerate() {
            // Skip whitespace
            if token.kind().is_trivia() {
                prev_was_dot = false;
                continue;
            }

            // Track if previous token was DOT (for method chain detection)
            if token.kind() == SyntaxKind::DOT {
                prev_was_dot = true;
                continue;
            }

            let rest = &flat[i + 1..];

            // Check for constructor/function call pattern: IDENT followed IMMEDIATELY by L_PAREN
            // This needs to be checked BEFORE literal inference to avoid picking up
            // string literals inside constructor arguments
            // IMPORTANT: For method calls like numbers.sorted(), we should NOT match
            // sorted as a free function call - it's a method call that needs scope resolution.
            if token.kind() == SyntaxKind::IDENT && !prev_was_dot {
                let ident_text = token.text().to_string();
                let is_uppercase = ident_text.chars().next().is_some_and(char::is_uppercase);

                // Check if FIRST non-trivia token indicates a call
                // - L_PAREN: direct call like `listOf()` or `ArrayList()`
                // - LT: generic constructor like `ArrayList<String>()`
                // DOT means this is a receiver in a method chain
                let first_non_trivia = rest.iter().find(|t| !t.kind().is_trivia());

                let is_direct_call =
                    first_non_trivia.is_some_and(|t| t.kind() == SyntaxKind::L_PAREN);
                let is_generic_constructor =
                    is_uppercase && first_non_trivia.is_some_and(|t| t.kind() == SyntaxKind::LT);

                if is_direct_call || is_generic_constructor {
                    let name = ident_text.clone();
                    // If starts with uppercase, it's a constructor - type is the name itself
                    if is_uppercase {
                        // For generic constructors like ArrayList<String>(), extract type arguments
                        if is_generic_constructor {
                            // Extract type arguments from <...> section
                            let type_args: Vec<Type> = rest
                                .iter()
                                .skip_while(|t| t.kind() != SyntaxKind::LT)
                                .skip(1) // Skip LT
                                .take_while(|t| t.kind() != SyntaxKind::GT)
                                .filter(|t| t.kind() == SyntaxKind::IDENT)
                                .map(|t| Type::Simple(t.text().to_string()))
                                .collect();

                            if !type_args.is_empty() {
                                return Some(Type::Generic(name, type_args));
                            }
                        }
                        return Some(Type::Simple(name));
                    }
                    // If lowercase, it's a function call - try to resolve return type
                    // Extract argument tokens from L_PAREN to R_PAREN
                    let arg_tokens: Vec<_> = rest
                        .iter()
                        .skip_while(|t| t.kind() != SyntaxKind::L_PAREN)
                        .skip(1) // Skip L_PAREN
                        .take_while(|t| t.kind() != SyntaxKind::R_PAREN)
                        .map(|t| (t.kind(), t.text().to_string()))
                        .collect();

                    // Try to resolve return type with arguments
                    if let Some(ty) =
                        resolve_free_function_return_type_with_args(&name, &arg_tokens)
                    {
                        return Some(ty);
                    }
                }
            }
            prev_was_dot = false;

            // A literal decides the type only when it is the whole initializer. An
            // operator or call after it can change the type (`2 * radius`, `1 + 2.0`),
            // and a method chain is left to scope-based resolution
            // (`"hello".uppercase().length`).
            if let Some(ty) = Self::infer_literal_type(token.kind(), token.text()) {
                let has_chain = rest.iter().any(|t| t.kind() == SyntaxKind::DOT);
                let is_whole_initializer = rest.iter().all(|t| t.kind().is_trivia());
                let is_first_significant = flat[..i].iter().all(|t| t.kind().is_trivia());

                if !has_chain && is_whole_initializer && is_first_significant {
                    return Some(ty);
                }
            }
        }

        // Structured fallback: a few shapes are only recognised at the node
        // level — qualified constructors (`Outer.Inner()`) and the
        // `BINARY_EXPR` the parser builds for `ArrayList<String>()`.
        let mut found_eq = false;
        for elem in prop_decl.children_with_tokens() {
            if !found_eq {
                found_eq = elem.as_token().is_some_and(|t| t.kind() == SyntaxKind::EQ);
                continue;
            }
            if let Some(node) = elem.as_node() {
                // Try to infer type from the initializer node
                if let Some(ty) = Self::infer_from_initializer(node) {
                    return Some(ty);
                }
                // If infer_from_initializer didn't work, try other patterns
                // Check if it's a single literal
                if let Some(ty) = Self::infer_from_literal_expr(node) {
                    return Some(ty);
                }
            }
        }

        None
    }

    /// Flatten a property's initializer (everything after the top-level `=`)
    /// into a token stream.
    ///
    /// Lambda and block bodies are *not* descended into: their contents belong
    /// to an inner scope, and letting their `DOT`s and literals leak into this
    /// stream would corrupt the receiver-chain analysis in `infer_val_type`
    /// (e.g. `val m = list.map { it.name }` must not look like a chain ending
    /// in `.name`). This mirrors the pre-expression-parser tree, where a
    /// lambda was an opaque `LAMBDA_EXPR` sibling that the scan stepped over.
    fn flat_initializer_tokens(prop_decl: &SyntaxNode) -> Vec<crate::SyntaxToken> {
        fn push_tokens(node: &SyntaxNode, out: &mut Vec<crate::SyntaxToken>) {
            for elem in node.children_with_tokens() {
                match elem {
                    rowan::NodeOrToken::Token(t) => out.push(t),
                    rowan::NodeOrToken::Node(n) => {
                        if !matches!(n.kind(), SyntaxKind::LAMBDA_EXPR | SyntaxKind::BLOCK) {
                            push_tokens(&n, out);
                        }
                    }
                }
            }
        }

        let mut out = Vec::new();
        let mut found_eq = false;
        for elem in prop_decl.children_with_tokens() {
            if !found_eq {
                found_eq = elem.as_token().is_some_and(|t| t.kind() == SyntaxKind::EQ);
                continue;
            }
            match elem {
                rowan::NodeOrToken::Token(t) => out.push(t),
                rowan::NodeOrToken::Node(n) => {
                    if !matches!(n.kind(), SyntaxKind::LAMBDA_EXPR | SyntaxKind::BLOCK) {
                        push_tokens(&n, &mut out);
                    }
                }
            }
        }
        out
    }

    /// Extract nullable type
    fn extract_nullable(node: &SyntaxNode) -> Type {
        // Find the base type (TYPE_REF child)
        for child in node.children() {
            if child.kind() == SyntaxKind::TYPE_REF {
                let base_type = Self::extract_type_ref(&child);
                return Type::Nullable(Box::new(base_type));
            }
        }

        // Fallback: collect text and parse
        let text = node.text().to_string();
        let base_text = text.trim_end_matches('?').trim();
        Type::Nullable(Box::new(Type::Simple(base_text.to_string())))
    }

    /// Extract type reference (can be simple or generic)
    fn extract_type_ref(node: &SyntaxNode) -> Type {
        // Check for generic type (TYPE_ARGUMENT_LIST child)
        let mut base_name = None;
        let mut type_args = None;
        let mut is_nullable = false;

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::IDENT && base_name.is_none() {
                        base_name = Some(token.text().to_string());
                    } else if token.kind() == SyntaxKind::QUESTION {
                        is_nullable = true;
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::TYPE_ARGUMENT_LIST {
                        type_args = Some(Self::extract_type_args(&n));
                    }
                }
            }
        }

        let base_type = match (base_name, type_args) {
            (Some(name), Some(args)) => Type::Generic(name, args),
            (Some(name), None) => Type::Simple(name),
            _ => {
                // Fallback: collect all IDENT tokens
                let idents = Self::collect_idents(node);
                if idents.is_empty() {
                    Type::Unknown
                } else {
                    Type::Simple(idents.join("."))
                }
            }
        };

        // Wrap in Nullable if ? was present
        if is_nullable {
            Type::Nullable(Box::new(base_type))
        } else {
            base_type
        }
    }

    /// Extract type arguments from a TYPE_ARGUMENT_LIST node
    fn extract_type_args(node: &SyntaxNode) -> Vec<Type> {
        let mut args = Vec::new();

        for child in node.children() {
            match child.kind() {
                SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE => {
                    args.push(Self::extract(&child));
                }
                _ => {}
            }
        }

        args
    }

    /// Collect all IDENT tokens from a node recursively
    fn collect_idents(node: &SyntaxNode) -> Vec<String> {
        let mut idents = Vec::new();

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::IDENT {
                        idents.push(token.text().to_string());
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    idents.extend(Self::collect_idents(&n));
                }
            }
        }

        idents
    }
}

/// Resolve the return type of a method call
///
/// Given a MethodCallInfo and the current scope, determines what type the method returns.
/// This enables hover to show inferred types for variables assigned from method calls.
///
/// # Arguments
///
/// * `method_info` - Information about the method call (receiver, method name, etc.)
/// * `scope` - The scope graph for resolving receiver types
/// * `current_offset` - The offset in the file where we're resolving (for scope lookup)
///
/// # Returns
///
/// The return type of the method, or None if it cannot be determined
pub fn resolve_method_return_type(
    method_info: &crate::scope::MethodCallInfo,
    scope: &crate::scope::ScopeGraph,
    current_offset: usize,
) -> Option<Type> {
    // Step 1: Determine receiver type
    let receiver_type = if let Some(receiver_name) = &method_info.receiver_name {
        // Instance method: resolve receiver from scope
        resolve_receiver_type(receiver_name, scope, current_offset)?
    } else if let Some(ref resolved_receiver_type) = method_info.receiver_type {
        // Chained call with pre-resolved receiver type (convert from KotlinType)
        resolved_receiver_type.to_simple_type()
    } else {
        // Free function: try argument-aware inference if we have arg tokens
        return method_info.arg_tokens.as_ref().map_or_else(
            || resolve_free_function_return_type(&method_info.method_name),
            |arg_tokens| {
                resolve_free_function_return_type_with_args(&method_info.method_name, arg_tokens)
            },
        );
    };

    // Step 2: Look up method return type from receiver type
    find_method_return_type(&receiver_type, &method_info.method_name)
}

/// Resolve the type of a receiver variable from scope
fn resolve_receiver_type(
    receiver_name: &str,
    scope: &crate::scope::ScopeGraph,
    offset: usize,
) -> Option<Type> {
    // Look up receiver in scope from the given offset
    let symbol = scope.resolve_symbol(receiver_name, offset)?;

    // Prefer explicit type annotation (convert from KotlinType to Type)
    if let Some(explicit_type) = &symbol.type_annotation {
        return Some(explicit_type.to_simple_type());
    }

    // If receiver is itself a method call, recursively resolve its return type
    if let Some(call_info) = &symbol.method_call_info {
        return resolve_method_return_type(call_info, scope, offset);
    }

    None
}

/// Find the return type of a method given its receiver type
pub fn find_method_return_type(receiver_type: &Type, method_name: &str) -> Option<Type> {
    match receiver_type {
        Type::Simple(class_name) => {
            // Builder pattern support
            // Most builder methods return the builder itself for chaining
            if class_name.ends_with("Builder") {
                // Methods that a builder shares with its text or with Object do not return
                // the builder, whatever the chaining convention.
                let shared = match method_name {
                    "toString" | "substring" => Some("String"),
                    "length" | "indexOf" | "lastIndexOf" | "capacity" | "hashCode" => Some("Int"),
                    "charAt" => Some("Char"),
                    "isEmpty" => Some("Boolean"),
                    _ => None,
                };
                if let Some(shared) = shared {
                    return Some(Type::Simple(shared.to_string()));
                }
                match method_name {
                    // build() returns the built type (remove "Builder" suffix)
                    "build" => {
                        if let Some(base_name) = class_name.strip_suffix("Builder") {
                            return Some(Type::Simple(base_name.to_string()));
                        }
                        // If not a Builder suffix, treat like other builder methods
                        return Some(receiver_type.clone());
                    }
                    // Common builder setter methods and default case - return the builder itself
                    _ => {
                        // These fluent methods return the builder itself (common pattern)
                        return Some(receiver_type.clone());
                    }
                }
            }

            // Static builder() method - returns XyzBuilder for class Xyz
            if method_name == "builder" {
                return Some(Type::Simple(format!("{class_name}Builder")));
            }

            // Consolidate by return type to avoid duplicate arm warnings
            let result_type = match (class_name.as_str(), method_name) {
                // String methods
                (
                    "String",
                    "lowercase" | "uppercase" | "trim" | "trimStart" | "trimEnd" | "substring"
                    | "replace" | "substringBefore" | "substringAfter" | "format",
                )
                | ("Int" | "Long" | "Float" | "Double" | "Boolean", "toString") => "String",
                // Boolean methods
                ("String" | "Boolean", "toBoolean" | "isEmpty" | "isNotEmpty")
                | (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet" | "Map"
                    | "HashMap" | "MutableMap",
                    "isEmpty" | "isNotEmpty",
                ) => "Boolean",
                // Int methods
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet" | "Map"
                    | "HashMap" | "MutableMap",
                    "size",
                )
                | ("String" | "Int" | "Long" | "Float" | "Double", "length" | "toInt") => "Int",
                // Other numeric conversions
                ("String" | "Int" | "Long" | "Float" | "Double", "toLong") => "Long",
                ("String" | "Int" | "Long" | "Float" | "Double", "toFloat") => "Float",
                ("String" | "Int" | "Long" | "Float" | "Double", "toDouble") => "Double",
                // Fallback to Unknown for everything else (including collection element access)
                _ => return Some(Type::Unknown),
            };
            Some(Type::Simple(result_type.to_string()))
        }
        Type::Generic(class_name, type_args) => {
            // Handle generic types
            match (class_name.as_str(), method_name) {
                // Collection methods - Int returns
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet" | "Map"
                    | "HashMap" | "MutableMap",
                    "size",
                )
                | (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet",
                    "count",
                ) => Some(Type::Simple("Int".to_string())),
                // Collection methods - Boolean returns
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet" | "Map"
                    | "HashMap" | "MutableMap",
                    "isEmpty" | "isNotEmpty",
                )
                | ("Optional", "isEmpty" | "isPresent") => {
                    Some(Type::Simple("Boolean".to_string()))
                }
                // List/Set element access - returns T
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet",
                    "get" | "first" | "last",
                ) => {
                    // For List<T>/Set<T>, get() returns T (type_args[0])
                    type_args.first().cloned()
                }
                // List/Set nullable element access - returns T?
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet",
                    "firstOrNull" | "lastOrNull",
                ) => {
                    // For List<T>/Set<T>, firstOrNull() returns T? (nullable type_args[0])
                    type_args
                        .first()
                        .map(|t| Type::Nullable(Box::new(t.clone())))
                }
                // Map value access - returns V?
                ("Map" | "HashMap" | "MutableMap", "get") => {
                    // For Map<K, V>, get() returns V? (nullable type_args[1])
                    type_args
                        .get(1)
                        .map(|v| Type::Nullable(Box::new(v.clone())))
                }
                ("Optional", "get" | "orElse" | "orElseGet") => {
                    // Optional<T>.get() returns T
                    type_args.first().cloned()
                }

                // Higher-order functions (lambdas)
                // Note: For now, we return Unknown for map() since we'd need to analyze the lambda
                // In the future, we could infer the return type from the lambda body
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet",
                    "map",
                ) => {
                    // Iterable.map { ... } returns List<R> for every collection, sets included.
                    // R is the lambda's return type, which is not inferred yet.
                    Some(Type::Generic("List".to_string(), vec![Type::Unknown]))
                }
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet",
                    "filter" | "filterNot",
                ) => {
                    // Iterable.filter/filterNot { ... } returns List<T> for every collection.
                    Some(Type::Generic("List".to_string(), type_args.clone()))
                }
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet",
                    "forEach",
                ) => {
                    // forEach returns Unit
                    Some(Type::Simple("Unit".to_string()))
                }
                (
                    "List" | "ArrayList" | "MutableList" | "Set" | "HashSet" | "MutableSet",
                    "any" | "all" | "none",
                ) => {
                    // Predicates return Boolean
                    Some(Type::Simple("Boolean".to_string()))
                }
                ("List" | "ArrayList" | "MutableList", "find") => {
                    // find returns T? (nullable element type)
                    type_args
                        .first()
                        .map(|t| Type::Nullable(Box::new(t.clone())))
                }
                (
                    "List" | "ArrayList" | "MutableList",
                    "sorted" | "sortedBy" | "sortedDescending",
                ) => {
                    // Sorted returns same collection type
                    Some(Type::Generic(class_name.clone(), type_args.clone()))
                }

                _ => Some(Type::Unknown),
            }
        }
        Type::Nullable(inner) => {
            // For nullable types, unwrap and recurse
            // Most nullable methods return the same as non-nullable, except for ?.method() which stays nullable
            find_method_return_type(inner, method_name)
        }
        _ => Some(Type::Unknown),
    }
}

/// Resolve return type for free functions (no receiver)
pub fn resolve_free_function_return_type(function_name: &str) -> Option<Type> {
    match function_name {
        // List constructors
        "listOf" | "emptyList" => Some(Type::Generic("List".to_string(), vec![Type::Unknown])),
        "mutableListOf" => Some(Type::Generic(
            "MutableList".to_string(),
            vec![Type::Unknown],
        )),
        "arrayListOf" => Some(Type::Generic("ArrayList".to_string(), vec![Type::Unknown])),

        // Set constructors
        "setOf" | "emptySet" => Some(Type::Generic("Set".to_string(), vec![Type::Unknown])),
        "mutableSetOf" => Some(Type::Generic("MutableSet".to_string(), vec![Type::Unknown])),
        "hashSetOf" => Some(Type::Generic("HashSet".to_string(), vec![Type::Unknown])),

        // Map constructors
        "mapOf" | "emptyMap" => Some(Type::Generic(
            "Map".to_string(),
            vec![Type::Unknown, Type::Unknown],
        )),
        "mutableMapOf" => Some(Type::Generic(
            "MutableMap".to_string(),
            vec![Type::Unknown, Type::Unknown],
        )),
        "hashMapOf" => Some(Type::Generic(
            "HashMap".to_string(),
            vec![Type::Unknown, Type::Unknown],
        )),

        // Array constructor
        "arrayOf" => Some(Type::Generic("Array".to_string(), vec![Type::Unknown])),

        // Printing functions
        "println" | "print" => Some(Type::Simple("Unit".to_string())),

        // String conversion
        "toString" => Some(Type::Simple("String".to_string())),

        // Not found
        _ => None,
    }
}

/// Resolve return type for free functions with argument type inference
///
/// This function enhances the basic free function return type by inferring generic
/// type parameters from the actual arguments passed to collection constructors.
///
/// # Arguments
///
/// * `function_name` - The name of the function being called
/// * `arg_tokens` - Tokens representing the function arguments (between parentheses)
///
/// # Returns
///
/// The inferred return type with concrete type parameters if possible
///
/// # Examples
///
/// - `listOf(1, 2, 3)` → `List<Int>` (infers Int from literal arguments)
/// - `listOf("a", "b")` → `List<String>` (infers String from literal arguments)
/// - `listOf()` → `List<?>` (no arguments = Unknown type param)
/// - `setOf(true, false)` → `Set<Boolean>`
/// - `mapOf()` → `Map<?, ?>` (not enough info for map inference yet)
pub fn resolve_free_function_return_type_with_args(
    function_name: &str,
    arg_tokens: &[(crate::SyntaxKind, String)],
) -> Option<Type> {
    // Try the new inference engine first
    if let Some(result) = infer_generic_function_return_type(function_name, arg_tokens) {
        return Some(result);
    }

    // Fallback to the basic inference
    resolve_free_function_return_type(function_name)
}

/// Infer return type using the new generic type inference engine
///
/// This leverages the stdlib registry and type unifier for accurate generic inference.
fn infer_generic_function_return_type(
    function_name: &str,
    arg_tokens: &[(crate::SyntaxKind, String)],
) -> Option<Type> {
    use crate::inference::infer_generic_call;
    use crate::types::KotlinType;

    // Convert argument tokens to KotlinTypes
    let arg_types = infer_argument_types(arg_tokens);
    let kotlin_arg_types: Vec<KotlinType> = arg_types.into_iter().map(KotlinType::from).collect();

    // Use the new inference engine
    let result = infer_generic_call(function_name, &kotlin_arg_types)?;

    // Convert back to old Type format
    Some(result.to_simple_type())
}

/// Infer types from argument tokens
///
/// Analyzes a sequence of tokens representing function arguments and infers
/// the type of each argument.
///
/// # Arguments
///
/// * `tokens` - Token sequence between the parentheses of a function call
///
/// # Returns
///
/// Vector of inferred types for each argument
///
/// # Examples
///
/// - `[INT_LITERAL("1"), COMMA, INT_LITERAL("2")]` → `[Int, Int]`
/// - `[STRING_LITERAL("hello"), COMMA, STRING_LITERAL("world")]` → `[String, String]`
fn infer_argument_types(tokens: &[(crate::SyntaxKind, String)]) -> Vec<Type> {
    let mut arg_types = Vec::new();
    let mut current_arg_tokens = Vec::new();

    for (kind, text) in tokens {
        match kind {
            crate::SyntaxKind::COMMA => {
                // End of current argument
                if !current_arg_tokens.is_empty() {
                    arg_types.push(infer_single_argument_type(&current_arg_tokens));
                    current_arg_tokens.clear();
                }
            }
            crate::SyntaxKind::WHITESPACE => {
                // Ignore whitespace
            }
            _ => {
                current_arg_tokens.push((*kind, text.clone()));
            }
        }
    }

    // Don't forget the last argument (no trailing comma)
    if !current_arg_tokens.is_empty() {
        arg_types.push(infer_single_argument_type(&current_arg_tokens));
    }

    arg_types
}

/// Infer the type of a single argument from its tokens. An argument that is not a
/// lone literal is `Unknown`: its first token says nothing about the whole expression
/// (`"a" to 1` is not a String), and it still takes a position, so the arity holds.
fn infer_single_argument_type(tokens: &[(crate::SyntaxKind, String)]) -> Type {
    use crate::SyntaxKind;

    // Skip leading whitespace
    let tokens: Vec<_> = tokens
        .iter()
        .filter(|(k, _)| *k != SyntaxKind::WHITESPACE)
        .collect();

    if let [(kind, text)] = tokens.as_slice() {
        if let Some(lit_type) = TypeExtractor::infer_literal_type(*kind, text) {
            return lit_type;
        }
    }

    // Constructor calls and other expressions are not typed here yet.
    Type::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    #[test]
    fn test_simple_type() {
        let source = "val x: String = \"\"";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Find TYPE_REF node
        fn find_type_ref(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::TYPE_REF {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_ref(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_ref = find_type_ref(&root).expect("TYPE_REF not found");
        let extracted_type = TypeExtractor::extract(&type_ref);

        assert_eq!(extracted_type, Type::Simple("String".to_string()));
        assert_eq!(extracted_type.format(), "String");
    }

    #[test]
    fn test_nullable_type() {
        let source = "val x: String? = null";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Find TYPE_REF node (nullable marker is inside TYPE_REF)
        fn find_type_ref(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::TYPE_REF {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_ref(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_ref = find_type_ref(&root).expect("TYPE_REF not found");
        let extracted_type = TypeExtractor::extract(&type_ref);

        assert_eq!(
            extracted_type,
            Type::Nullable(Box::new(Type::Simple("String".to_string())))
        );
        assert_eq!(extracted_type.format(), "String?");
    }

    #[test]
    fn test_generic_type() {
        let source = "val list: List<String> = emptyList()";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Find TYPE_REF node
        fn find_type_ref(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::TYPE_REF {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_ref(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_ref = find_type_ref(&root).expect("TYPE_REF not found");
        let extracted_type = TypeExtractor::extract(&type_ref);

        assert_eq!(
            extracted_type,
            Type::Generic("List".to_string(), vec![Type::Simple("String".to_string())])
        );
        assert_eq!(extracted_type.format(), "List<String>");
    }

    #[test]
    fn test_nested_generic_type() {
        let source = "val map: Map<String, List<Int>> = emptyMap()";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // Find TYPE_REF node
        fn find_type_ref(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::TYPE_REF {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_ref(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_ref = find_type_ref(&root).expect("TYPE_REF not found");
        let extracted_type = TypeExtractor::extract(&type_ref);

        assert_eq!(
            extracted_type,
            Type::Generic(
                "Map".to_string(),
                vec![
                    Type::Simple("String".to_string()),
                    Type::Generic("List".to_string(), vec![Type::Simple("Int".to_string())])
                ]
            )
        );
        assert_eq!(extracted_type.format(), "Map<String, List<Int>>");
    }

    // ===== Literal Type Inference Tests =====

    #[test]
    fn test_infer_int_literal() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::INT_LITERAL, "42"),
            Some(Type::Simple("Int".to_string()))
        );
    }

    #[test]
    fn test_infer_long_literal() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::LONG_LITERAL, "42L"),
            Some(Type::Simple("Long".to_string()))
        );
    }

    #[test]
    fn test_infer_float_literal() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::FLOAT_LITERAL, "3.14f"),
            Some(Type::Simple("Float".to_string()))
        );
    }

    #[test]
    fn test_infer_double_literal() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::DOUBLE_LITERAL, "3.14"),
            Some(Type::Simple("Double".to_string()))
        );
    }

    #[test]
    fn test_infer_string_literal() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::STRING_LITERAL, "\"hello\""),
            Some(Type::Simple("String".to_string()))
        );
    }

    #[test]
    fn test_infer_raw_string_literal() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::RAW_STRING_LITERAL, "\"\"\"hello\"\"\""),
            Some(Type::Simple("String".to_string()))
        );
    }

    #[test]
    fn test_infer_char_literal() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::CHAR_LITERAL, "'a'"),
            Some(Type::Simple("Char".to_string()))
        );
    }

    #[test]
    fn test_infer_boolean_true() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::TRUE_KW, "true"),
            Some(Type::Simple("Boolean".to_string()))
        );
    }

    #[test]
    fn test_infer_boolean_false() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::FALSE_KW, "false"),
            Some(Type::Simple("Boolean".to_string()))
        );
    }

    #[test]
    fn test_infer_null() {
        assert_eq!(
            TypeExtractor::infer_literal_type(SyntaxKind::NULL_KW, "null"),
            Some(Type::Nullable(Box::new(Type::Simple(
                "Nothing".to_string()
            ))))
        );
    }

    #[test]
    fn test_infer_from_literal_expr_int() {
        let source = "val x = 5";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        // The literal should be inferrable from the tree
        fn find_literal(node: &SyntaxNode) -> Option<Type> {
            TypeExtractor::infer_from_literal_expr(node).or_else(|| {
                for child in node.children() {
                    if let Some(ty) = find_literal(&child) {
                        return Some(ty);
                    }
                }
                None
            })
        }

        let inferred = find_literal(&root).expect("Should infer int type");
        assert_eq!(inferred, Type::Simple("Int".to_string()));
    }

    #[test]
    fn test_infer_from_literal_expr_string() {
        let source = "val message = \"hello\"";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_literal(node: &SyntaxNode) -> Option<Type> {
            TypeExtractor::infer_from_literal_expr(node).or_else(|| {
                for child in node.children() {
                    if let Some(ty) = find_literal(&child) {
                        return Some(ty);
                    }
                }
                None
            })
        }

        let inferred = find_literal(&root).expect("Should infer String type");
        assert_eq!(inferred, Type::Simple("String".to_string()));
    }

    #[test]
    fn test_infer_from_constructor_call() {
        let source = "val user = User(\"John\")";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn print_tree(node: &SyntaxNode, indent: usize) {
            for elem in node.children_with_tokens() {
                let prefix = "  ".repeat(indent);
                match elem {
                    rowan::NodeOrToken::Node(n) => {
                        eprintln!("{}Node: {:?}", prefix, n.kind());
                        print_tree(&n, indent + 1);
                    }
                    rowan::NodeOrToken::Token(t) => {
                        eprintln!("{}Token: {:?} '{}'", prefix, t.kind(), t.text());
                    }
                }
            }
        }

        eprintln!("=== AST STRUCTURE ===");
        print_tree(&root, 0);

        // Try to find constructor call and infer type
        fn find_constructor(node: &SyntaxNode) -> Option<Type> {
            // Try to infer from this node
            if let Some(ty) = TypeExtractor::infer_from_constructor_call(node) {
                eprintln!(
                    "Found constructor type: {:?} at node kind: {:?}",
                    ty,
                    node.kind()
                );
                return Some(ty);
            }

            // Recurse
            for child in node.children() {
                if let Some(ty) = find_constructor(&child) {
                    return Some(ty);
                }
            }
            None
        }

        let inferred = find_constructor(&root);
        eprintln!("Final result: {inferred:?}");
        assert_eq!(inferred, Some(Type::Simple("User".to_string())));
    }

    // === Tests for infer_val_type() ===

    #[test]
    fn test_infer_val_from_literal_int() {
        let source = "val x = 5";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        assert_eq!(inferred, Some(Type::Simple("Int".to_string())));
    }

    #[test]
    fn list_of_a_null_argument_is_nullable_in_its_element_type() {
        let source = "val xs = listOf(null, 1)";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            node.children().find_map(|child| find_property(&child))
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        assert_eq!(
            inferred,
            Some(Type::Generic(
                "List".to_string(),
                vec![Type::Nullable(Box::new(Type::Simple("Int".to_string())))],
            ))
        );
    }

    #[test]
    fn test_builder_text_methods_do_not_return_the_builder() {
        let builder = Type::Simple("StringBuilder".to_string());

        assert_eq!(
            find_method_return_type(&builder, "toString"),
            Some(Type::Simple("String".to_string()))
        );
        assert_eq!(
            find_method_return_type(&builder, "length"),
            Some(Type::Simple("Int".to_string()))
        );
        assert_eq!(
            find_method_return_type(&builder, "append"),
            Some(builder.clone())
        );
    }

    #[test]
    fn test_set_map_and_filter_return_lists() {
        let set_of_int = Type::Generic("Set".to_string(), vec![Type::Simple("Int".to_string())]);

        assert_eq!(
            find_method_return_type(&set_of_int, "map"),
            Some(Type::Generic("List".to_string(), vec![Type::Unknown]))
        );
        assert_eq!(
            find_method_return_type(&set_of_int, "filter"),
            Some(Type::Generic(
                "List".to_string(),
                vec![Type::Simple("Int".to_string())]
            ))
        );
    }

    #[test]
    fn test_infer_val_literal_inside_an_expression_does_not_set_the_type() {
        fn infer(source: &str) -> Option<Type> {
            let parse_tree = parse(source);
            let root = SyntaxNode::new_root(parse_tree.green_node);
            fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
                if node.kind() == SyntaxKind::PROPERTY_DECL {
                    return Some(node.clone());
                }
                node.children().find_map(|child| find_property(&child))
            }
            let prop = find_property(&root).expect("PROPERTY_DECL not found");
            TypeExtractor::infer_val_type(&prop)
        }

        // The `2` is not the type of `2 * radius`, and `1` is not the type of `1 + 2.0`.
        assert_ne!(
            infer("val area = 2 * radius"),
            Some(Type::Simple("Int".to_string()))
        );
        assert_ne!(
            infer("val sum = 1 + 2.0"),
            Some(Type::Simple("Int".to_string()))
        );
        assert_eq!(infer("val n = 7"), Some(Type::Simple("Int".to_string())));
    }

    #[test]
    fn test_infer_val_from_literal_string() {
        let source = "val msg = \"hello\"";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        assert_eq!(inferred, Some(Type::Simple("String".to_string())));
    }

    #[test]
    fn test_infer_val_from_literal_boolean() {
        let source = "val flag = true";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        assert_eq!(inferred, Some(Type::Simple("Boolean".to_string())));
    }

    #[test]
    fn test_infer_val_from_constructor() {
        let source = "val user = User()";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        assert_eq!(inferred, Some(Type::Simple("User".to_string())));
    }

    #[test]
    fn test_infer_val_from_generic_constructor() {
        let source = "val list = ArrayList<String>()";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        assert_eq!(
            inferred,
            Some(Type::Generic(
                "ArrayList".to_string(),
                vec![Type::Simple("String".to_string())]
            ))
        );
    }

    #[test]
    fn test_infer_val_with_explicit_type() {
        // When there's an explicit type annotation, it should be used instead of inferring
        let source = "val user: Person = User()";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        // Should return the explicit type annotation (Person), not the constructor type (User)
        assert_eq!(inferred, Some(Type::Simple("Person".to_string())));
    }

    #[test]
    fn test_infer_val_from_null() {
        let source = "val empty = null";
        let parse_tree = parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::PROPERTY_DECL {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        let inferred = TypeExtractor::infer_val_type(&prop);

        // null has type Nothing?
        assert_eq!(
            inferred,
            Some(Type::Nullable(Box::new(Type::Simple(
                "Nothing".to_string()
            ))))
        );
    }

    #[test]
    fn test_infer_user_generic_constructor() {
        // Test: val repo = Repository<User>()
        let code = r"
            fun main() {
                val repo = Repository<User>()
            }
        ";

        let parse_tree = crate::parse(code);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_property(node: &SyntaxNode) -> Option<SyntaxNode> {
            for child in node.children() {
                if child.kind() == SyntaxKind::PROPERTY_DECL {
                    return Some(child);
                }
                if let Some(found) = find_property(&child) {
                    return Some(found);
                }
            }
            None
        }

        let prop = find_property(&root).expect("PROPERTY_DECL not found");
        eprintln!("Property node text: {}", prop.text());
        let inferred = TypeExtractor::infer_val_type(&prop);
        eprintln!("Inferred type: {inferred:?}");

        assert_eq!(
            inferred,
            Some(Type::Generic(
                "Repository".to_string(),
                vec![Type::Simple("User".to_string())]
            ))
        );
    }

    #[test]
    fn test_infer_val_from_multi_argument_generic_constructor() {
        let parse_tree = crate::parse(
            r#"
            fun main() {
                val pair = Pair<String, Int>("hello", 42)
            }
            "#,
        );
        let root = SyntaxNode::new_root(parse_tree.green_node);
        let prop = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::PROPERTY_DECL)
            .expect("PROPERTY_DECL not found");

        assert_eq!(
            TypeExtractor::infer_val_type(&prop),
            Some(Type::Generic(
                "Pair".to_string(),
                vec![
                    Type::Simple("String".to_string()),
                    Type::Simple("Int".to_string())
                ]
            ))
        );
    }
}
