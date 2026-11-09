// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Reserved extract-refactoring API.
//!
//! The public types preserve the intended API shape, but every analysis and
//! edit operation currently returns [`ExtractError::NotImplemented`]. LSP
//! extract actions must remain disabled until these methods compute and
//! validate real edits.

use hyperion_semantics::db::FileId;
use rowan::TextRange;

/// Reserved API for extracting a selection into a method.
pub struct ExtractMethod;

impl ExtractMethod {
    /// Create a new extract method refactoring
    pub fn new() -> Self {
        Self
    }

    /// Analyze a selection for extract method
    ///
    /// Returns information about:
    /// - Required parameters (variables used from outer scope)
    /// - Return values (variables assigned that are used after)
    /// - Suggested method name
    /// - Conflicts and issues
    pub fn analyze_selection(
        _file_id: FileId,
        _range: TextRange,
    ) -> Result<ExtractMethodAnalysis, ExtractError> {
        Err(ExtractError::NotImplemented)
    }

    /// Compute the edits needed to extract a method
    pub fn compute_extraction(
        _file_id: FileId,
        _range: TextRange,
        _method_name: &str,
    ) -> Result<Vec<ExtractEdit>, ExtractError> {
        Err(ExtractError::NotImplemented)
    }
}

impl Default for ExtractMethod {
    fn default() -> Self {
        Self::new()
    }
}

/// Reserved API for extracting an expression into a local variable.
pub struct ExtractVariable;

impl ExtractVariable {
    /// Create a new extract variable refactoring
    pub fn new() -> Self {
        Self
    }

    /// Analyze an expression for extraction
    pub fn analyze_expression(
        _file_id: FileId,
        _range: TextRange,
    ) -> Result<ExtractVariableAnalysis, ExtractError> {
        Err(ExtractError::NotImplemented)
    }

    /// Compute the edits needed to extract a variable
    pub fn compute_extraction(
        _file_id: FileId,
        _range: TextRange,
        _var_name: &str,
    ) -> Result<Vec<ExtractEdit>, ExtractError> {
        Err(ExtractError::NotImplemented)
    }
}

impl Default for ExtractVariable {
    fn default() -> Self {
        Self::new()
    }
}

/// Reserved API for extracting a literal into a named constant.
pub struct ExtractConstant;

impl ExtractConstant {
    /// Create a new extract constant refactoring
    pub fn new() -> Self {
        Self
    }

    /// Analyze a literal for extraction
    pub fn analyze_literal(
        _file_id: FileId,
        _range: TextRange,
    ) -> Result<ExtractConstantAnalysis, ExtractError> {
        Err(ExtractError::NotImplemented)
    }

    /// Compute the edits needed to extract a constant
    pub fn compute_extraction(
        _file_id: FileId,
        _range: TextRange,
        _constant_name: &str,
    ) -> Result<Vec<ExtractEdit>, ExtractError> {
        Err(ExtractError::NotImplemented)
    }
}

impl Default for ExtractConstant {
    fn default() -> Self {
        Self::new()
    }
}

/// Analysis result for extract method
#[derive(Debug, Clone)]
pub struct ExtractMethodAnalysis {
    /// Variables that need to be passed as parameters
    pub required_parameters: Vec<String>,
    /// Variables that need to be returned
    pub return_values: Vec<String>,
    /// Suggested method name
    pub suggested_name: String,
    /// Whether the extraction is valid
    pub is_valid: bool,
    /// Validation errors or warnings
    pub messages: Vec<String>,
}

/// Analysis result for extract variable
#[derive(Debug, Clone)]
pub struct ExtractVariableAnalysis {
    /// Type of the expression
    pub expression_type: String,
    /// Suggested variable name
    pub suggested_name: String,
    /// Number of occurrences that can be replaced
    pub occurrence_count: usize,
    /// Whether the extraction is valid
    pub is_valid: bool,
    /// Validation errors or warnings
    pub messages: Vec<String>,
}

/// Analysis result for extract constant
#[derive(Debug, Clone)]
pub struct ExtractConstantAnalysis {
    /// Type of the literal
    pub literal_type: String,
    /// Suggested constant name (UPPER_SNAKE_CASE)
    pub suggested_name: String,
    /// Where to place the constant (class, interface)
    pub placement: ConstantPlacement,
    /// Number of occurrences that can be replaced
    pub occurrence_count: usize,
    /// Whether the extraction is valid
    pub is_valid: bool,
    /// Validation errors or warnings
    pub messages: Vec<String>,
}

/// Where to place an extracted constant
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstantPlacement {
    /// Place in the current class
    CurrentClass,
    /// Place in an interface
    Interface,
    /// Place in a separate constants class
    ConstantsClass,
}

/// Edit for an extract refactoring
#[derive(Debug, Clone)]
pub struct ExtractEdit {
    /// File to edit
    pub file_id: FileId,
    /// Range to replace
    pub range: TextRange,
    /// New text
    pub new_text: String,
}

/// Errors that can occur during extract refactorings
#[derive(Debug, Clone, thiserror::Error)]
pub enum ExtractError {
    /// Feature not yet implemented
    #[error("Extract refactoring not yet implemented")]
    NotImplemented,

    /// Invalid selection
    #[error("Invalid selection: {0}")]
    InvalidSelection(String),

    /// Cannot extract this code
    #[error("Cannot extract: {0}")]
    CannotExtract(String),

    /// Name conflict
    #[error("Name conflict: {0}")]
    NameConflict(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_method_placeholder() {
        let _method = ExtractMethod::new();
        assert!(
            ExtractMethod::analyze_selection(FileId::new(0), TextRange::empty(0.into())).is_err()
        );
    }

    #[test]
    fn test_extract_variable_placeholder() {
        let _var = ExtractVariable::new();
        assert!(
            ExtractVariable::analyze_expression(FileId::new(0), TextRange::empty(0.into()))
                .is_err()
        );
    }

    #[test]
    fn test_extract_constant_placeholder() {
        let _constant = ExtractConstant::new();
        assert!(
            ExtractConstant::analyze_literal(FileId::new(0), TextRange::empty(0.into())).is_err()
        );
    }
}
