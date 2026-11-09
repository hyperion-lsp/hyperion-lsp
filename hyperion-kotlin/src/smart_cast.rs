// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Smart Cast Tracking for Kotlin Type Narrowing
//!
//! This module tracks type narrowing from `is` checks and null checks in Kotlin code.
//!
//! # Overview
//!
//! Kotlin smart casts narrow types after type checks:
//!
//! ```kotlin
//! fun process(obj: Any) {
//!     if (obj is String) {
//!         // obj is now smart-cast to String!
//!         println(obj.length)  // No cast needed
//!     }
//! }
//! ```
//!
//! # Supported Patterns
//!
//! - `if (x is Type)` - x is Type in true branch
//! - `if (x !is Type)` - x is Type in false/else branch
//! - `when (x) { is Type -> }` - x is Type in that branch
//! - `if (x != null)` - x is non-nullable in true branch
//! - `if (x == null)` - x is null in true branch (non-nullable in else)
//! - `x && y is Type` - y is Type after the &&
//!
//! # Architecture
//!
//! The `SmartCastContext` maintains a stack of scopes, each with its own set of
//! active smart casts. When entering a block (if body, when branch), a new scope
//! is pushed. When exiting, it's popped.
//!
//! The context is built during AST traversal and consulted during type resolution.

use crate::types::{KotlinType, Nullability};
use crate::{SyntaxKind, SyntaxNode};
use rowan::TextRange;
use std::collections::HashMap;

/// Represents a single smart cast (type narrowing)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartCastInfo {
    /// The variable being narrowed
    pub variable_name: String,

    /// The original type before narrowing (if known)
    pub original_type: Option<KotlinType>,

    /// The narrowed type
    pub narrowed_type: KotlinType,

    /// The source of this smart cast
    pub source: SmartCastSource,
}

/// How the smart cast was triggered
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartCastSource {
    /// `x is Type` check
    IsCheck,

    /// `x !is Type` in else branch
    NotIsCheck,

    /// `x != null` check
    NullCheck,

    /// `x == null` in else branch
    NullEqualsCheck,

    /// `when (x) { is Type -> }` branch
    WhenBranch,

    /// `x?.let { it }` - it is non-nullable
    SafeCallLet,
}

/// A scope-aware context for tracking active smart casts
///
/// This tracks smart casts across nested scopes, properly handling
/// scope entry/exit for if blocks, when branches, etc.
#[derive(Debug, Clone, Default)]
pub struct SmartCastContext {
    /// Stack of scopes, each with its own smart cast mappings
    /// Inner scopes inherit from outer scopes
    scopes: Vec<SmartCastScope>,
}

/// A single scope level with its smart casts
#[derive(Debug, Clone, Default)]
struct SmartCastScope {
    /// Variable name -> smart cast info
    casts: HashMap<String, SmartCastInfo>,
}

impl SmartCastContext {
    /// Create a new empty smart cast context
    pub fn new() -> Self {
        Self {
            scopes: vec![SmartCastScope::default()],
        }
    }

    /// Enter a new scope (e.g., if block, when branch)
    pub fn enter_scope(&mut self) {
        self.scopes.push(SmartCastScope {
            casts: HashMap::new(),
        });
    }

    /// Exit the current scope, discarding its smart casts
    pub fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Add a smart cast to the current scope
    pub fn add_smart_cast(&mut self, info: SmartCastInfo) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.casts.insert(info.variable_name.clone(), info);
        }
    }

    /// Add a type narrowing from an `is` check
    pub fn add_is_check(&mut self, variable: impl Into<String>, narrowed_type: KotlinType) {
        self.add_smart_cast(SmartCastInfo {
            variable_name: variable.into(),
            original_type: None,
            narrowed_type,
            source: SmartCastSource::IsCheck,
        });
    }

    /// Add a type narrowing from a `!is` check (for else branch)
    pub fn add_not_is_check(&mut self, variable: impl Into<String>, narrowed_type: KotlinType) {
        self.add_smart_cast(SmartCastInfo {
            variable_name: variable.into(),
            original_type: None,
            narrowed_type,
            source: SmartCastSource::NotIsCheck,
        });
    }

    /// Add a null check narrowing (`x != null`)
    pub fn add_null_check(&mut self, variable: impl Into<String>, original_type: KotlinType) {
        // Make the type non-nullable
        let narrowed = original_type.to_non_nullable();
        self.add_smart_cast(SmartCastInfo {
            variable_name: variable.into(),
            original_type: Some(original_type),
            narrowed_type: narrowed,
            source: SmartCastSource::NullCheck,
        });
    }

    /// Add a when branch smart cast
    pub fn add_when_branch(&mut self, variable: impl Into<String>, narrowed_type: KotlinType) {
        self.add_smart_cast(SmartCastInfo {
            variable_name: variable.into(),
            original_type: None,
            narrowed_type,
            source: SmartCastSource::WhenBranch,
        });
    }

    /// Get the smart cast for a variable, if any
    ///
    /// Searches from innermost to outermost scope
    pub fn get_smart_cast(&self, variable: &str) -> Option<&SmartCastInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(cast) = scope.casts.get(variable) {
                return Some(cast);
            }
        }
        None
    }

    /// Get the narrowed type for a variable, if any
    pub fn get_narrowed_type(&self, variable: &str) -> Option<&KotlinType> {
        self.get_smart_cast(variable).map(|c| &c.narrowed_type)
    }

    /// Check if a variable has an active smart cast
    pub fn has_smart_cast(&self, variable: &str) -> bool {
        self.get_smart_cast(variable).is_some()
    }

    /// Get all active smart casts (for debugging)
    pub fn all_active_casts(&self) -> Vec<&SmartCastInfo> {
        let mut result = HashMap::new();
        // Process outer to inner, so inner overrides outer
        for scope in &self.scopes {
            for (name, cast) in &scope.casts {
                result.insert(name.as_str(), cast);
            }
        }
        result.into_values().collect()
    }

    /// Current scope depth (for debugging)
    pub const fn depth(&self) -> usize {
        self.scopes.len()
    }
}

/// Condition analysis result for smart cast extraction
#[derive(Debug, Clone)]
pub struct ConditionAnalysis {
    /// Smart casts for the true branch
    pub true_branch_casts: Vec<SmartCastInfo>,

    /// Smart casts for the false/else branch
    pub false_branch_casts: Vec<SmartCastInfo>,
}

impl Default for ConditionAnalysis {
    fn default() -> Self {
        Self::new()
    }
}

impl ConditionAnalysis {
    pub const fn new() -> Self {
        Self {
            true_branch_casts: Vec::new(),
            false_branch_casts: Vec::new(),
        }
    }

    /// Merge with another analysis (for && conditions)
    pub fn merge(&mut self, other: ConditionAnalysis) {
        self.true_branch_casts.extend(other.true_branch_casts);
        self.false_branch_casts.extend(other.false_branch_casts);
    }
}

/// Analyze a condition expression for smart casts
///
/// This extracts smart cast information from conditions like:
/// - `x is String`
/// - `x !is String`
/// - `x != null`
/// - `x == null`
/// - `x is String && x.length > 0`
pub fn analyze_condition(condition: &SyntaxNode) -> ConditionAnalysis {
    let mut analysis = ConditionAnalysis::new();

    // Collect all tokens from the condition
    let tokens = collect_condition_tokens(condition);

    // Try to extract smart casts from the tokens
    analyze_condition_tokens(&tokens, &mut analysis);

    analysis
}

/// Collect tokens from a condition node, flattening nested expressions
fn collect_condition_tokens(node: &SyntaxNode) -> Vec<(SyntaxKind, String)> {
    let mut tokens = Vec::new();

    fn collect(node: &SyntaxNode, tokens: &mut Vec<(SyntaxKind, String)>) {
        for elem in node.children_with_tokens() {
            match elem {
                rowan::NodeOrToken::Token(t) => {
                    tokens.push((t.kind(), t.text().to_string()));
                }
                rowan::NodeOrToken::Node(n) => {
                    collect(&n, tokens);
                }
            }
        }
    }

    collect(node, &mut tokens);
    tokens
}

/// Analyze collected tokens for smart cast patterns
fn analyze_condition_tokens(tokens: &[(SyntaxKind, String)], analysis: &mut ConditionAnalysis) {
    let mut i = 0;

    while i < tokens.len() {
        // Skip whitespace
        if tokens[i].0 == SyntaxKind::WHITESPACE {
            i += 1;
            continue;
        }

        // Look for && to split compound conditions
        if tokens[i].0 == SyntaxKind::AMPAMP {
            // Process left side has already been done
            // Continue to process right side
            i += 1;
            continue;
        }

        // Pattern: IDENT IS_KW TYPE
        // or: IDENT NOT_IS TYPE (for !is)
        if tokens[i].0 == SyntaxKind::IDENT {
            let var_name = tokens[i].1.clone();

            // Look ahead for IS_KW or NOT_IS
            let mut j = i + 1;
            while j < tokens.len() && tokens[j].0 == SyntaxKind::WHITESPACE {
                j += 1;
            }

            if j < tokens.len() {
                match tokens[j].0 {
                    SyntaxKind::IS_KW => {
                        // x is Type
                        if let Some((type_name, end_idx)) = extract_type_name(tokens, j + 1) {
                            let narrowed = KotlinType::simple(&type_name);
                            analysis.true_branch_casts.push(SmartCastInfo {
                                variable_name: var_name.clone(),
                                original_type: None,
                                narrowed_type: narrowed.clone(),
                                source: SmartCastSource::IsCheck,
                            });
                            // For else branch, the variable is NOT that type
                            // We don't add a cast for !is in true branch
                            i = end_idx;
                            continue;
                        }
                    }
                    SyntaxKind::NOT_IS => {
                        // x !is Type
                        if let Some((type_name, end_idx)) = extract_type_name(tokens, j + 1) {
                            let narrowed = KotlinType::simple(&type_name);
                            // For !is, the smart cast applies to the ELSE branch
                            analysis.false_branch_casts.push(SmartCastInfo {
                                variable_name: var_name.clone(),
                                original_type: None,
                                narrowed_type: narrowed,
                                source: SmartCastSource::NotIsCheck,
                            });
                            i = end_idx;
                            continue;
                        }
                    }
                    SyntaxKind::BANGEQ => {
                        // x != ...
                        // Check if right side is null
                        let mut k = j + 1;
                        while k < tokens.len() && tokens[k].0 == SyntaxKind::WHITESPACE {
                            k += 1;
                        }
                        if k < tokens.len() && tokens[k].0 == SyntaxKind::NULL_KW {
                            // x != null -> true branch has non-null x
                            analysis.true_branch_casts.push(SmartCastInfo {
                                variable_name: var_name.clone(),
                                original_type: None,
                                // Non-nullable Any as placeholder
                                narrowed_type: KotlinType::Any,
                                source: SmartCastSource::NullCheck,
                            });
                            i = k + 1;
                            continue;
                        }
                    }
                    SyntaxKind::EQEQ => {
                        // x == ...
                        let mut k = j + 1;
                        while k < tokens.len() && tokens[k].0 == SyntaxKind::WHITESPACE {
                            k += 1;
                        }
                        if k < tokens.len() && tokens[k].0 == SyntaxKind::NULL_KW {
                            // x == null -> else branch has non-null x
                            analysis.false_branch_casts.push(SmartCastInfo {
                                variable_name: var_name.clone(),
                                original_type: None,
                                narrowed_type: KotlinType::Any,
                                source: SmartCastSource::NullEqualsCheck,
                            });
                            i = k + 1;
                            continue;
                        }
                    }
                    _ => {}
                }
            }
        }

        i += 1;
    }
}

/// Extract a type name from tokens starting at given index
/// Returns the type name and the index after the type
fn extract_type_name(tokens: &[(SyntaxKind, String)], start: usize) -> Option<(String, usize)> {
    let mut i = start;

    // Skip whitespace
    while i < tokens.len() && tokens[i].0 == SyntaxKind::WHITESPACE {
        i += 1;
    }

    if i >= tokens.len() {
        return None;
    }

    // Expect IDENT for the type name
    if tokens[i].0 != SyntaxKind::IDENT {
        return None;
    }

    let mut type_parts = vec![tokens[i].1.clone()];
    i += 1;

    // Check for qualified name (Foo.Bar)
    while i + 1 < tokens.len() {
        // Skip whitespace
        if tokens[i].0 == SyntaxKind::WHITESPACE {
            i += 1;
            continue;
        }

        if tokens[i].0 == SyntaxKind::DOT {
            i += 1;
            // Skip whitespace after dot
            while i < tokens.len() && tokens[i].0 == SyntaxKind::WHITESPACE {
                i += 1;
            }
            if i < tokens.len() && tokens[i].0 == SyntaxKind::IDENT {
                type_parts.push(tokens[i].1.clone());
                i += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Some((type_parts.join("."), i))
}

/// Analyze a when expression for smart casts in each branch
///
/// For each `is Type` branch, creates a smart cast for the subject variable.
pub fn analyze_when_branches(
    when_node: &SyntaxNode,
    subject_name: &str,
) -> Vec<(TextRange, SmartCastInfo)> {
    let mut results = Vec::new();

    if when_node.kind() != SyntaxKind::WHEN_EXPR {
        return results;
    }

    // Find all WHEN_ENTRY nodes
    for child in when_node.children() {
        if child.kind() == SyntaxKind::WHEN_ENTRY {
            if let Some((branch_range, type_name)) = extract_when_entry_is_type(&child) {
                results.push((
                    branch_range,
                    SmartCastInfo {
                        variable_name: subject_name.to_string(),
                        original_type: None,
                        narrowed_type: KotlinType::simple(&type_name),
                        source: SmartCastSource::WhenBranch,
                    },
                ));
            }
        }
    }

    results
}

/// Extract the is-type from a when entry condition
fn extract_when_entry_is_type(entry: &SyntaxNode) -> Option<(TextRange, String)> {
    let mut found_is = false;
    let mut type_parts = Vec::new();
    let mut body_range = None;

    for elem in entry.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            match token.kind() {
                SyntaxKind::IS_KW => {
                    found_is = true;
                }
                SyntaxKind::IDENT if found_is => {
                    type_parts.push(token.text().to_string());
                }
                SyntaxKind::DOT if found_is => {
                    // Continue collecting qualified name
                }
                _ => {}
            }
        } else if let Some(node) = elem.as_node() {
            // The body block
            if matches!(node.kind(), SyntaxKind::BLOCK | SyntaxKind::EXPR_STMT) {
                body_range = Some(node.text_range());
            }
        }
    }

    if found_is && !type_parts.is_empty() {
        let type_name = type_parts.join(".");
        let range = body_range.unwrap_or_else(|| entry.text_range());
        Some((range, type_name))
    } else {
        None
    }
}

// Extension to KotlinType for smart cast operations
impl KotlinType {
    /// Convert a nullable type to non-nullable
    pub fn to_non_nullable(&self) -> KotlinType {
        match self {
            KotlinType::Class {
                fqn,
                type_arguments,
                nullability: _,
            } => KotlinType::Class {
                fqn: fqn.clone(),
                type_arguments: type_arguments.clone(),
                nullability: Nullability::NonNull,
            },
            KotlinType::Function {
                receiver,
                parameters,
                return_type,
                nullability: _,
            } => KotlinType::Function {
                receiver: receiver.clone(),
                parameters: parameters.clone(),
                return_type: return_type.clone(),
                nullability: Nullability::NonNull,
            },
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_cast_context_basic() {
        let mut ctx = SmartCastContext::new();

        ctx.add_is_check("x", KotlinType::simple("String"));

        assert!(ctx.has_smart_cast("x"));
        assert!(!ctx.has_smart_cast("y"));

        let cast = ctx.get_smart_cast("x").unwrap();
        assert_eq!(cast.variable_name, "x");
        assert_eq!(cast.narrowed_type, KotlinType::simple("String"));
    }

    #[test]
    fn test_smart_cast_context_scoping() {
        let mut ctx = SmartCastContext::new();

        // Outer scope
        ctx.add_is_check("x", KotlinType::simple("Number"));

        // Enter inner scope
        ctx.enter_scope();
        ctx.add_is_check("x", KotlinType::simple("Int"));
        ctx.add_is_check("y", KotlinType::simple("String"));

        // Inner scope should override
        assert_eq!(
            ctx.get_narrowed_type("x").unwrap(),
            &KotlinType::simple("Int")
        );
        assert_eq!(
            ctx.get_narrowed_type("y").unwrap(),
            &KotlinType::simple("String")
        );

        // Exit inner scope
        ctx.exit_scope();

        // Back to outer scope
        assert_eq!(
            ctx.get_narrowed_type("x").unwrap(),
            &KotlinType::simple("Number")
        );
        assert!(!ctx.has_smart_cast("y"));
    }

    #[test]
    fn test_null_check_smart_cast() {
        let mut ctx = SmartCastContext::new();

        let nullable_string = KotlinType::simple("String").nullable();
        ctx.add_null_check("x", nullable_string);

        let cast = ctx.get_smart_cast("x").unwrap();
        assert_eq!(cast.source, SmartCastSource::NullCheck);
        assert!(!cast.narrowed_type.is_nullable());
    }

    #[test]
    fn test_condition_analysis_is_check() {
        // Simulate tokens for: x is String
        let tokens = vec![
            (SyntaxKind::IDENT, "x".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::IS_KW, "is".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::IDENT, "String".to_string()),
        ];

        let mut analysis = ConditionAnalysis::new();
        analyze_condition_tokens(&tokens, &mut analysis);

        assert_eq!(analysis.true_branch_casts.len(), 1);
        assert_eq!(analysis.true_branch_casts[0].variable_name, "x");
        assert_eq!(
            analysis.true_branch_casts[0].narrowed_type,
            KotlinType::simple("String")
        );
    }

    #[test]
    fn test_condition_analysis_not_is_check() {
        // Simulate tokens for: x !is String
        let tokens = vec![
            (SyntaxKind::IDENT, "x".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::NOT_IS, "!is".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::IDENT, "String".to_string()),
        ];

        let mut analysis = ConditionAnalysis::new();
        analyze_condition_tokens(&tokens, &mut analysis);

        // !is puts the smart cast in the FALSE branch
        assert_eq!(analysis.true_branch_casts.len(), 0);
        assert_eq!(analysis.false_branch_casts.len(), 1);
        assert_eq!(analysis.false_branch_casts[0].variable_name, "x");
    }

    #[test]
    fn test_condition_analysis_null_check() {
        // Simulate tokens for: x != null
        let tokens = vec![
            (SyntaxKind::IDENT, "x".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::BANGEQ, "!=".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::NULL_KW, "null".to_string()),
        ];

        let mut analysis = ConditionAnalysis::new();
        analyze_condition_tokens(&tokens, &mut analysis);

        assert_eq!(analysis.true_branch_casts.len(), 1);
        assert_eq!(
            analysis.true_branch_casts[0].source,
            SmartCastSource::NullCheck
        );
    }

    #[test]
    fn test_condition_analysis_null_equals() {
        // Simulate tokens for: x == null
        let tokens = vec![
            (SyntaxKind::IDENT, "x".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::EQEQ, "==".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::NULL_KW, "null".to_string()),
        ];

        let mut analysis = ConditionAnalysis::new();
        analyze_condition_tokens(&tokens, &mut analysis);

        // == null puts the non-null cast in the FALSE (else) branch
        assert_eq!(analysis.false_branch_casts.len(), 1);
        assert_eq!(
            analysis.false_branch_casts[0].source,
            SmartCastSource::NullEqualsCheck
        );
    }

    #[test]
    fn test_qualified_type_name() {
        // Simulate tokens for: x is Result.Success
        let tokens = vec![
            (SyntaxKind::IDENT, "x".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::IS_KW, "is".to_string()),
            (SyntaxKind::WHITESPACE, " ".to_string()),
            (SyntaxKind::IDENT, "Result".to_string()),
            (SyntaxKind::DOT, ".".to_string()),
            (SyntaxKind::IDENT, "Success".to_string()),
        ];

        let mut analysis = ConditionAnalysis::new();
        analyze_condition_tokens(&tokens, &mut analysis);

        assert_eq!(analysis.true_branch_casts.len(), 1);
        assert_eq!(
            analysis.true_branch_casts[0].narrowed_type,
            KotlinType::simple("Result.Success")
        );
    }
}
