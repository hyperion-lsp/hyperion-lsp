// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Change Signature Refactoring
//!
//! Implements method signature modifications including:
//! - Rename method
//! - Add/remove/reorder parameters
//! - Change parameter types
//! - Update all call sites automatically
//!
//! # Safety Guarantees
//!
//! - Parameter validation (valid types and names)
//! - Call site discovery before changes
//! - Preview mode (compute edits without applying)
//! - Conflict detection (name collisions)
//!
//! # Example
//!
//! ```ignore
//! use hyperion_refactor::signature::{ChangeSignatureRefactoring, ParameterChange};
//!
//! // Find method signature
//! let signature = ChangeSignatureRefactoring::find_method_signature(
//!     &db, file_id, offset, &workspace_index
//! )?;
//!
//! // Apply parameter changes
//! let changes = ParameterChange::add_parameter(Parameter::new("newParam", "String"));
//! let edits = ChangeSignatureRefactoring::compute_signature_change(
//!     &db, &signature, changes, &workspace_index
//! )?;
//! ```

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::FileId;
use hyperion_workspace::{SymbolLocation, WorkspaceIndex};
use rowan::{TextRange, TextSize};
use std::sync::Arc;

/// A method parameter with its type and name
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    /// Parameter name
    pub name: String,
    /// Parameter type (as string, e.g., "String", "List<User>")
    pub type_name: String,
    /// Annotations on the parameter (e.g., "@NotNull")
    pub annotations: Vec<String>,
    /// Whether this is a varargs parameter
    pub is_varargs: bool,
    /// Text range of the parameter in the source
    pub range: Option<TextRange>,
}

impl Parameter {
    /// Create a new parameter with name and type
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            annotations: Vec::new(),
            is_varargs: false,
            range: None,
        }
    }

    /// Create a varargs parameter
    pub fn varargs(name: impl Into<String>, element_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            type_name: element_type.into(),
            annotations: Vec::new(),
            is_varargs: true,
            range: None,
        }
    }

    /// Add an annotation to the parameter
    pub fn with_annotation(mut self, annotation: impl Into<String>) -> Self {
        self.annotations.push(annotation.into());
        self
    }

    /// Format parameter for display (e.g., "@NotNull String name")
    pub fn format(&self) -> String {
        let mut parts = Vec::new();
        for ann in &self.annotations {
            parts.push(ann.clone());
        }
        if self.is_varargs {
            parts.push(format!("{}...", self.type_name));
        } else {
            parts.push(self.type_name.clone());
        }
        parts.push(self.name.clone());
        parts.join(" ")
    }
}

/// Represents a method signature extracted from source code
#[derive(Debug, Clone)]
pub struct MethodSignature {
    /// File containing the method
    pub file_id: FileId,
    /// Fully qualified name of the method (e.g., "com.example.MyClass.myMethod")
    pub fqn: String,
    /// Simple method name
    pub name: String,
    /// Return type (e.g., "void", "String", "List<User>")
    pub return_type: String,
    /// Text range of the return type, or of `void`
    pub return_type_range: Option<TextRange>,
    /// Method parameters in order
    pub parameters: Vec<Parameter>,
    /// Exception types thrown (e.g., ["IOException", "SQLException"])
    pub throws: Vec<String>,
    /// Modifiers (e.g., ["public", "static"])
    pub modifiers: Vec<String>,
    /// Generic type parameters (e.g., ["T", "U extends Comparable"])
    pub type_parameters: Vec<String>,
    /// Text range of the entire method declaration
    pub declaration_range: TextRange,
    /// Text range of just the parameter list (including parentheses)
    pub parameter_list_range: Option<TextRange>,
    /// Text range of just the method name
    pub name_range: Option<TextRange>,
}

impl MethodSignature {
    /// Format the full signature for display
    pub fn format(&self) -> String {
        let params = self
            .parameters
            .iter()
            .map(|p| p.format())
            .collect::<Vec<_>>()
            .join(", ");

        let type_params = if self.type_parameters.is_empty() {
            String::new()
        } else {
            format!("<{}> ", self.type_parameters.join(", "))
        };

        let throws = if self.throws.is_empty() {
            String::new()
        } else {
            format!(" throws {}", self.throws.join(", "))
        };

        format!(
            "{}{} {}({}){}",
            type_params, self.return_type, self.name, params, throws
        )
    }

    /// Get just the simple method name with parameters for display
    pub fn format_short(&self) -> String {
        let params = self
            .parameters
            .iter()
            .map(|p| p.format())
            .collect::<Vec<_>>()
            .join(", ");
        format!("{}({})", self.name, params)
    }
}

/// Represents a call site where a method is invoked
#[derive(Debug, Clone)]
pub struct CallSite {
    /// File containing the call
    pub file_id: FileId,
    /// Text range of the entire method invocation
    pub invocation_range: TextRange,
    /// Text range of just the argument list (including parentheses)
    pub argument_list_range: TextRange,
    /// Arguments passed at this call site (as source text)
    pub arguments: Vec<String>,
    /// Whether this is a constructor call (new Foo())
    pub is_constructor: bool,
    /// Whether this is a super() or this() call
    pub is_chained_constructor: bool,
}

/// Describes changes to be made to a method signature
#[derive(Debug, Clone, Default)]
pub struct SignatureChange {
    /// New method name (None = keep current)
    pub new_name: Option<String>,
    /// New return type (None = keep current)
    pub new_return_type: Option<String>,
    /// Parameters to add (index, parameter)
    pub add_parameters: Vec<(usize, Parameter)>,
    /// Indices of parameters to remove
    pub remove_parameters: Vec<usize>,
    /// New parameter order (maps old index -> new index)
    pub reorder_parameters: Option<Vec<usize>>,
    /// Parameters with changed types (index, new_type)
    pub change_types: Vec<(usize, String)>,
    /// Parameters with changed names (index, new_name)
    pub rename_parameters: Vec<(usize, String)>,
    /// Default values for new parameters at call sites
    pub default_values: Vec<String>,
}

impl SignatureChange {
    /// Create an empty change (no modifications)
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a new method name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.new_name = Some(name.into());
        self
    }

    /// Add a parameter at the specified index
    pub fn add_parameter(
        mut self,
        index: usize,
        param: Parameter,
        default_value: impl Into<String>,
    ) -> Self {
        self.add_parameters.push((index, param));
        self.default_values.push(default_value.into());
        self
    }

    /// Remove the parameter at the specified index
    pub fn remove_parameter(mut self, index: usize) -> Self {
        self.remove_parameters.push(index);
        self
    }

    /// Rename a parameter
    pub fn rename_parameter(mut self, index: usize, new_name: impl Into<String>) -> Self {
        self.rename_parameters.push((index, new_name.into()));
        self
    }

    /// Change a parameter's type
    pub fn change_parameter_type(mut self, index: usize, new_type: impl Into<String>) -> Self {
        self.change_types.push((index, new_type.into()));
        self
    }

    /// Check if this change is empty (no modifications)
    pub fn is_empty(&self) -> bool {
        self.new_name.is_none()
            && self.new_return_type.is_none()
            && self.add_parameters.is_empty()
            && self.remove_parameters.is_empty()
            && self.reorder_parameters.is_none()
            && self.change_types.is_empty()
            && self.rename_parameters.is_empty()
    }
}

/// Represents an edit to apply for signature change
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureEdit {
    /// File to edit
    pub file_id: FileId,
    /// Range to replace
    pub range: TextRange,
    /// New text to insert
    pub new_text: String,
    /// Description of this edit
    pub description: String,
}

/// Errors that can occur during signature changes
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    /// No method found at the given position
    #[error("No method found at position")]
    MethodNotFound,

    /// Invalid new name for method or parameter
    #[error("Invalid identifier: {0}")]
    InvalidName(String),

    /// Parameter index out of bounds
    #[error("Parameter index {0} out of bounds (method has {1} parameters)")]
    ParameterIndexOutOfBounds(usize, usize),

    /// Duplicate parameter name
    #[error("Duplicate parameter name: {0}")]
    DuplicateParameterName(String),

    /// Cannot change signature of this kind of method
    #[error("Cannot change signature: {0}")]
    CannotChange(String),

    /// Missing default value for new parameter
    #[error("Missing default value for new parameter '{0}'")]
    MissingDefaultValue(String),

    /// Parse error extracting signature
    #[error("Failed to parse method signature")]
    ParseError,
}

/// Change Signature refactoring engine
pub struct ChangeSignatureRefactoring;

impl ChangeSignatureRefactoring {
    /// Extract method signature from AST at the given position
    ///
    /// Returns the method signature if cursor is on a method declaration.
    pub fn extract_signature_from_node(
        file_id: FileId,
        method_node: &SyntaxNode,
        containing_class: Option<&str>,
        package: Option<&str>,
    ) -> Result<MethodSignature, SignatureError> {
        if method_node.kind() != SyntaxKind::METHOD_DECL {
            return Err(SignatureError::MethodNotFound);
        }

        let mut name = String::new();
        let mut return_type = String::new();
        let mut parameters = Vec::new();
        let mut modifiers = Vec::new();
        let mut throws = Vec::new();
        let mut type_parameters = Vec::new();
        let mut return_type_range = None;
        let mut parameter_list_range = None;
        let mut name_range = None;

        for child in method_node.children_with_tokens() {
            match child.kind() {
                SyntaxKind::IDENT if name.is_empty() => {
                    if let Some(token) = child.as_token() {
                        name = token.text().to_string();
                        name_range = Some(token.text_range());
                    }
                }
                SyntaxKind::PRIMITIVE_TYPE
                | SyntaxKind::CLASS_TYPE
                | SyntaxKind::ARRAY_TYPE
                | SyntaxKind::VAR_TYPE
                | SyntaxKind::QUALIFIED_TYPE
                    if return_type.is_empty() =>
                {
                    return_type = child
                        .as_node()
                        .map(|n| n.text().to_string())
                        .unwrap_or_default();
                    return_type_range = child.as_node().map(|n| n.text_range());
                }
                SyntaxKind::VOID_KW => {
                    return_type = "void".to_string();
                    return_type_range = child.as_token().map(|t| t.text_range());
                }
                SyntaxKind::PARAM_LIST => {
                    if let Some(node) = child.as_node() {
                        parameter_list_range = Some(node.text_range());
                        parameters = Self::extract_parameters(node);
                    }
                }
                SyntaxKind::THROWS_CLAUSE => {
                    if let Some(node) = child.as_node() {
                        throws = Self::extract_throws(node);
                    }
                }
                SyntaxKind::TYPE_PARAM_LIST => {
                    if let Some(node) = child.as_node() {
                        type_parameters = Self::extract_type_parameters(node);
                    }
                }
                SyntaxKind::PUBLIC_KW => modifiers.push("public".to_string()),
                SyntaxKind::PRIVATE_KW => modifiers.push("private".to_string()),
                SyntaxKind::PROTECTED_KW => modifiers.push("protected".to_string()),
                SyntaxKind::STATIC_KW => modifiers.push("static".to_string()),
                SyntaxKind::FINAL_KW => modifiers.push("final".to_string()),
                SyntaxKind::ABSTRACT_KW => modifiers.push("abstract".to_string()),
                SyntaxKind::SYNCHRONIZED_KW => modifiers.push("synchronized".to_string()),
                SyntaxKind::NATIVE_KW => modifiers.push("native".to_string()),
                _ => {}
            }
        }

        if name.is_empty() {
            return Err(SignatureError::ParseError);
        }

        // Build FQN
        let fqn = match (package, containing_class) {
            (Some(pkg), Some(cls)) => format!("{}.{}.{}", pkg, cls, name),
            (None, Some(cls)) => format!("{}.{}", cls, name),
            (Some(pkg), None) => format!("{}.{}", pkg, name),
            (None, None) => name.clone(),
        };

        Ok(MethodSignature {
            file_id,
            fqn,
            name,
            return_type,
            return_type_range,
            parameters,
            throws,
            modifiers,
            type_parameters,
            declaration_range: method_node.text_range(),
            parameter_list_range,
            name_range,
        })
    }

    /// Extract parameters from a PARAM_LIST node
    fn extract_parameters(param_list: &SyntaxNode) -> Vec<Parameter> {
        let mut params = Vec::new();

        for child in param_list.children() {
            if child.kind() == SyntaxKind::PARAM {
                let mut name = String::new();
                let mut type_name = String::new();
                let mut annotations = Vec::new();
                let mut is_varargs = false;

                for param_child in child.children_with_tokens() {
                    match param_child.kind() {
                        SyntaxKind::IDENT => {
                            if let Some(token) = param_child.as_token() {
                                name = token.text().to_string();
                            }
                        }
                        SyntaxKind::PRIMITIVE_TYPE
                        | SyntaxKind::CLASS_TYPE
                        | SyntaxKind::ARRAY_TYPE
                        | SyntaxKind::VAR_TYPE
                        | SyntaxKind::QUALIFIED_TYPE => {
                            if let Some(node) = param_child.as_node() {
                                type_name = node.text().to_string();
                            }
                        }
                        SyntaxKind::ELLIPSIS => {
                            is_varargs = true;
                        }
                        SyntaxKind::ANNOTATION => {
                            if let Some(node) = param_child.as_node() {
                                annotations.push(node.text().to_string());
                            }
                        }
                        _ => {}
                    }
                }

                if !name.is_empty() {
                    params.push(Parameter {
                        name,
                        type_name,
                        annotations,
                        is_varargs,
                        range: Some(child.text_range()),
                    });
                }
            }
        }

        params
    }

    /// Extract exception types from a THROWS_CLAUSE node
    fn extract_throws(throws_node: &SyntaxNode) -> Vec<String> {
        let mut throws = Vec::new();
        for child in throws_node.children() {
            if child.kind() == SyntaxKind::CLASS_TYPE || child.kind() == SyntaxKind::QUALIFIED_TYPE
            {
                throws.push(child.text().to_string());
            }
        }
        throws
    }

    /// Extract type parameters from a TYPE_PARAM_LIST node
    fn extract_type_parameters(type_params: &SyntaxNode) -> Vec<String> {
        let mut params = Vec::new();
        for child in type_params.children() {
            if child.kind() == SyntaxKind::TYPE_PARAM {
                params.push(child.text().to_string());
            }
        }
        params
    }

    /// Validate a signature change
    pub fn validate_change(
        signature: &MethodSignature,
        change: &SignatureChange,
    ) -> Result<(), SignatureError> {
        // Validate new method name
        if let Some(ref new_name) = change.new_name {
            if !is_valid_java_identifier(new_name) {
                return Err(SignatureError::InvalidName(new_name.clone()));
            }
        }

        // A change is edited at its source range. A missing range is refused here, so no
        // edit is silently skipped.
        if change.new_name.is_some() && signature.name_range.is_none() {
            return Err(SignatureError::CannotChange(
                "the method name's source range is unknown".to_string(),
            ));
        }
        if Self::changes_parameters(change) && signature.parameter_list_range.is_none() {
            return Err(SignatureError::CannotChange(
                "the parameter list's source range is unknown".to_string(),
            ));
        }
        if change
            .new_return_type
            .as_ref()
            .is_some_and(|ty| *ty != signature.return_type)
            && signature.return_type_range.is_none()
        {
            return Err(SignatureError::CannotChange(
                "the return type's source range is unknown".to_string(),
            ));
        }

        // Validate parameter indices
        let param_count = signature.parameters.len();
        for idx in &change.remove_parameters {
            if *idx >= param_count {
                return Err(SignatureError::ParameterIndexOutOfBounds(*idx, param_count));
            }
        }

        for (idx, _) in &change.change_types {
            if *idx >= param_count {
                return Err(SignatureError::ParameterIndexOutOfBounds(*idx, param_count));
            }
        }

        for (idx, _) in &change.rename_parameters {
            if *idx >= param_count {
                return Err(SignatureError::ParameterIndexOutOfBounds(*idx, param_count));
            }
        }

        // Check for duplicate parameter names in final result
        let mut final_names: Vec<String> = signature
            .parameters
            .iter()
            .enumerate()
            .filter(|(i, _)| !change.remove_parameters.contains(i))
            .map(|(i, p)| {
                change
                    .rename_parameters
                    .iter()
                    .find(|(idx, _)| *idx == i)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| p.name.clone())
            })
            .collect();

        for (_, param) in &change.add_parameters {
            final_names.push(param.name.clone());
        }

        // Check for duplicates
        let mut seen = std::collections::HashSet::new();
        for name in &final_names {
            if !seen.insert(name.clone()) {
                return Err(SignatureError::DuplicateParameterName(name.clone()));
            }
        }

        Ok(())
    }

    /// Compute the new parameter list text after applying changes
    pub fn compute_new_parameter_list(
        signature: &MethodSignature,
        change: &SignatureChange,
    ) -> String {
        // Start with existing parameters
        let mut params: Vec<Parameter> = signature.parameters.clone();

        // Apply renames
        for (idx, new_name) in &change.rename_parameters {
            if let Some(param) = params.get_mut(*idx) {
                param.name = new_name.clone();
            }
        }

        // Apply type changes
        for (idx, new_type) in &change.change_types {
            if let Some(param) = params.get_mut(*idx) {
                param.type_name = new_type.clone();
            }
        }

        // Remove parameters (in reverse order to preserve indices)
        let mut remove_indices: Vec<_> = change.remove_parameters.clone();
        remove_indices.sort_by(|a, b| b.cmp(a));
        for idx in remove_indices {
            if idx < params.len() {
                params.remove(idx);
            }
        }

        // Add new parameters
        for (idx, param) in &change.add_parameters {
            let insert_idx = (*idx).min(params.len());
            params.insert(insert_idx, param.clone());
        }

        // Apply reordering if specified
        if let Some(ref new_order) = change.reorder_parameters {
            let old_params = params.clone();
            params.clear();
            for &idx in new_order {
                if idx < old_params.len() {
                    params.push(old_params[idx].clone());
                }
            }
        }

        // Format the parameter list
        let param_strs: Vec<String> = params.iter().map(|p| p.format()).collect();
        format!("({})", param_strs.join(", "))
    }

    /// Whether the change adds, removes, reorders, retypes or renames any parameter.
    fn changes_parameters(change: &SignatureChange) -> bool {
        !change.add_parameters.is_empty()
            || !change.remove_parameters.is_empty()
            || !change.change_types.is_empty()
            || !change.rename_parameters.is_empty()
            || change.reorder_parameters.is_some()
    }

    /// Compute edits for signature change at declaration site
    pub fn compute_declaration_edits(
        signature: &MethodSignature,
        change: &SignatureChange,
    ) -> Vec<SignatureEdit> {
        let mut edits = Vec::new();

        // Edit the return type if it changes
        if let Some(ref new_return_type) = change.new_return_type {
            if *new_return_type != signature.return_type {
                if let Some(range) = signature.return_type_range {
                    edits.push(SignatureEdit {
                        file_id: signature.file_id,
                        range,
                        new_text: new_return_type.clone(),
                        description: format!("Change return type to '{}'", new_return_type),
                    });
                }
            }
        }

        // Edit method name if changed
        if let Some(ref new_name) = change.new_name {
            if let Some(name_range) = signature.name_range {
                edits.push(SignatureEdit {
                    file_id: signature.file_id,
                    range: name_range,
                    new_text: new_name.clone(),
                    description: format!("Rename method to '{}'", new_name),
                });
            }
        }

        // Edit parameter list if any parameter changes
        if Self::changes_parameters(change) {
            if let Some(param_range) = signature.parameter_list_range {
                let new_params = Self::compute_new_parameter_list(signature, change);
                edits.push(SignatureEdit {
                    file_id: signature.file_id,
                    range: param_range,
                    new_text: new_params,
                    description: "Update parameter list".to_string(),
                });
            }
        }

        edits
    }

    /// Compute edits for a single call site
    pub fn compute_call_site_edit(
        call_site: &CallSite,
        signature: &MethodSignature,
        change: &SignatureChange,
    ) -> Option<SignatureEdit> {
        // Build new argument list
        let mut args = call_site.arguments.clone();

        // Apply removals (in reverse order)
        let mut remove_indices: Vec<_> = change.remove_parameters.clone();
        remove_indices.sort_by(|a, b| b.cmp(a));
        for idx in remove_indices {
            if idx < args.len() {
                args.remove(idx);
            }
        }

        // Apply reordering if specified
        if let Some(ref new_order) = change.reorder_parameters {
            let old_args = args.clone();
            args.clear();
            for &idx in new_order {
                if idx < old_args.len() {
                    args.push(old_args[idx].clone());
                }
            }
        }

        // Add new parameters with default values
        for (i, (idx, _param)) in change.add_parameters.iter().enumerate() {
            let default_value = change
                .default_values
                .get(i)
                .cloned()
                .unwrap_or_else(|| "null".to_string());
            let insert_idx = (*idx).min(args.len());
            args.insert(insert_idx, default_value);
        }

        // Check if anything changed
        if args == call_site.arguments && change.new_name.is_none() {
            return None;
        }

        let new_args = format!("({})", args.join(", "));
        let (range, new_text) = if let Some(new_name) = &change.new_name {
            (call_site.invocation_range, format!("{new_name}{new_args}"))
        } else {
            (call_site.argument_list_range, new_args)
        };

        Some(SignatureEdit {
            file_id: call_site.file_id,
            range,
            new_text,
            description: format!("Update call to {}", signature.name),
        })
    }

    /// Find call sites using the legacy best-effort usage snapshot.
    ///
    /// Returns the call sites and the number of usages that could not be read as one,
    /// such as a method reference or a usage whose file has no source.
    pub fn find_call_sites<F>(
        signature: &MethodSignature,
        index: &WorkspaceIndex,
        get_source: F,
    ) -> (Vec<CallSite>, usize)
    where
        F: Fn(FileId) -> Option<Arc<String>>,
    {
        let mut unresolved = 0;
        let sites = index
            .find_usages(&signature.fqn)
            .into_iter()
            .filter_map(|usage| {
                let site = get_source(usage.file_id)
                    .and_then(|source| Self::extract_call_site_from_usage(&usage, &source));
                if site.is_none() {
                    unresolved += 1;
                }
                site
            })
            .collect();
        (sites, unresolved)
    }

    /// Extract a CallSite from a SymbolLocation usage
    ///
    /// The usage range points to the method name. We need to find the
    /// argument list that follows it.
    fn extract_call_site_from_usage(usage: &SymbolLocation, source: &str) -> Option<CallSite> {
        let name_end: usize = usage.range.end as usize;
        let remaining = source.get(name_end..)?;
        let paren_offset = next_argument_list_start(remaining)?;
        let arg_start = name_end + paren_offset;

        // Find matching closing paren (handling nested parens)
        let (arg_end, arguments) = Self::parse_argument_list(&remaining[paren_offset..])?;
        let arg_end = arg_start + arg_end;

        // Detect constructor vs normal call
        let is_constructor = {
            let before_name: usize = usage.range.start as usize;
            let prefix = source.get(..before_name).unwrap_or("");
            let trimmed = prefix.trim_end();
            trimmed.ends_with("new")
        };

        // Detect super() or this() call
        let is_chained_constructor = {
            let name_str = source
                .get(usage.range.start as usize..usage.range.end as usize)
                .unwrap_or("");
            name_str == "super" || name_str == "this"
        };

        Some(CallSite {
            file_id: usage.file_id,
            invocation_range: TextRange::new(
                TextSize::new(usage.range.start),
                TextSize::new(arg_end as u32),
            ),
            argument_list_range: TextRange::new(
                TextSize::new(arg_start as u32),
                TextSize::new(arg_end as u32),
            ),
            arguments,
            is_constructor,
            is_chained_constructor,
        })
    }

    /// Parse an argument list starting with '(' and return (end_offset, arguments)
    ///
    /// Handles nested parentheses, strings, and char literals.
    fn parse_argument_list(s: &str) -> Option<(usize, Vec<String>)> {
        if !s.starts_with('(') {
            return None;
        }

        let bytes = s.as_bytes();
        let mut depth = 0;
        let mut args = Vec::new();
        let mut current_arg_start = 1; // Skip opening paren
        let mut i = 0;
        let mut in_string = false;
        let mut in_char = false;

        while i < bytes.len() {
            let c = bytes[i] as char;

            // Handle escape sequences in strings/chars
            if (in_string || in_char) && c == '\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }

            // Handle string literals
            if c == '"' && !in_char {
                in_string = !in_string;
                i += 1;
                continue;
            }

            // Handle char literals
            if c == '\'' && !in_string {
                in_char = !in_char;
                i += 1;
                continue;
            }

            // Skip content inside strings/chars
            if in_string || in_char {
                i += 1;
                continue;
            }

            match c {
                '(' => {
                    depth += 1;
                }
                ')' => {
                    if depth == 1 {
                        // End of argument list
                        let arg = s[current_arg_start..i].trim();
                        if !arg.is_empty() {
                            args.push(arg.to_string());
                        }
                        return Some((i + 1, args));
                    }
                    depth -= 1;
                }
                ',' if depth == 1 => {
                    // Argument separator at top level
                    let arg = s[current_arg_start..i].trim();
                    args.push(arg.to_string());
                    current_arg_start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }

        None // Unclosed parentheses
    }

    /// Assemble declaration and call-site edits using the historical
    /// best-effort usage snapshot.
    pub fn compute_all_edits<F>(
        signature: &MethodSignature,
        change: &SignatureChange,
        index: &WorkspaceIndex,
        get_source: F,
    ) -> Result<Vec<SignatureEdit>, SignatureError>
    where
        F: Fn(FileId) -> Option<Arc<String>>,
    {
        Self::validate_change(signature, change)?;

        let (call_sites, unresolved) = Self::find_call_sites(signature, index, get_source);
        // A usage that cannot be rewritten would keep calling the old signature. A change
        // that renames or reshapes the callee is refused rather than applied in part.
        let reshapes_callers = change.new_name.is_some() || Self::changes_parameters(change);
        if unresolved > 0 && reshapes_callers {
            return Err(SignatureError::CannotChange(format!(
                "{unresolved} usage(s) cannot be rewritten, such as a method reference"
            )));
        }

        let mut edits = Self::compute_declaration_edits(signature, change);
        for call_site in &call_sites {
            if let Some(edit) = Self::compute_call_site_edit(call_site, signature, change) {
                edits.push(edit);
            }
        }
        Ok(edits)
    }
}

/// Return the opening parenthesis only when it is the next Java token after a
/// method name. The historical `find('(')` implementation could steal the
/// argument list from a later statement when the usage was a method reference.
fn next_argument_list_start(source_after_name: &str) -> Option<usize> {
    let bytes = source_after_name.as_bytes();
    let mut offset = 0;
    loop {
        while bytes
            .get(offset)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0c))
        {
            offset += 1;
        }
        match bytes.get(offset..offset.saturating_add(2)) {
            Some(b"//") => {
                offset += 2;
                offset += bytes
                    .get(offset..)?
                    .iter()
                    .position(|byte| *byte == b'\n')?
                    + 1;
            }
            Some(b"/*") => {
                offset += 2;
                let close = bytes
                    .get(offset..)?
                    .windows(2)
                    .position(|window| window == b"*/")?;
                offset += close + 2;
            }
            _ => break,
        }
    }
    (bytes.get(offset) == Some(&b'(')).then_some(offset)
}

fn is_valid_java_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if is_java_keyword(name) {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn is_java_keyword(name: &str) -> bool {
    matches!(
        name,
        "abstract"
            | "assert"
            | "boolean"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extends"
            | "final"
            | "finally"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "implements"
            | "import"
            | "instanceof"
            | "int"
            | "interface"
            | "long"
            | "native"
            | "new"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "short"
            | "static"
            | "strictfp"
            | "super"
            | "switch"
            | "synchronized"
            | "this"
            | "throw"
            | "throws"
            | "transient"
            | "try"
            | "void"
            | "volatile"
            | "while"
            | "true"
            | "false"
            | "null"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_format() {
        let p = Parameter::new("name", "String");
        assert_eq!(p.format(), "String name");

        let p = Parameter::new("args", "String").with_annotation("@NotNull");
        assert_eq!(p.format(), "@NotNull String args");

        let p = Parameter::varargs("items", "Object");
        assert_eq!(p.format(), "Object... items");
    }

    #[test]
    fn test_signature_change_builder() {
        let change = SignatureChange::new()
            .with_name("newMethod")
            .add_parameter(0, Parameter::new("ctx", "Context"), "null")
            .remove_parameter(1);

        assert_eq!(change.new_name, Some("newMethod".to_string()));
        assert_eq!(change.add_parameters.len(), 1);
        assert_eq!(change.remove_parameters, vec![1]);
    }

    #[test]
    fn test_compute_new_parameter_list() {
        let sig = MethodSignature {
            file_id: FileId::new(0),
            fqn: "Test.method".to_string(),
            name: "method".to_string(),
            return_type: "void".to_string(),
            return_type_range: None,
            parameters: vec![Parameter::new("a", "int"), Parameter::new("b", "String")],
            throws: vec![],
            modifiers: vec![],
            type_parameters: vec![],
            declaration_range: TextRange::default(),
            parameter_list_range: None,
            name_range: None,
        };

        // Test rename
        let change = SignatureChange::new().rename_parameter(0, "newA");
        let result = ChangeSignatureRefactoring::compute_new_parameter_list(&sig, &change);
        assert_eq!(result, "(int newA, String b)");

        // Test remove
        let change = SignatureChange::new().remove_parameter(0);
        let result = ChangeSignatureRefactoring::compute_new_parameter_list(&sig, &change);
        assert_eq!(result, "(String b)");

        // Test add
        let change =
            SignatureChange::new().add_parameter(0, Parameter::new("ctx", "Context"), "null");
        let result = ChangeSignatureRefactoring::compute_new_parameter_list(&sig, &change);
        assert_eq!(result, "(Context ctx, int a, String b)");
    }

    #[test]
    fn test_validate_change() {
        let sig = MethodSignature {
            file_id: FileId::new(0),
            fqn: "Test.method".to_string(),
            name: "method".to_string(),
            return_type: "void".to_string(),
            return_type_range: None,
            parameters: vec![Parameter::new("a", "int"), Parameter::new("b", "String")],
            throws: vec![],
            modifiers: vec![],
            type_parameters: vec![],
            declaration_range: TextRange::default(),
            parameter_list_range: Some(TextRange::default()),
            name_range: None,
        };

        // Valid change
        let change = SignatureChange::new().rename_parameter(0, "newA");
        assert!(ChangeSignatureRefactoring::validate_change(&sig, &change).is_ok());

        // Invalid: out of bounds
        let change = SignatureChange::new().remove_parameter(5);
        assert!(matches!(
            ChangeSignatureRefactoring::validate_change(&sig, &change),
            Err(SignatureError::ParameterIndexOutOfBounds(5, 2))
        ));

        // Invalid: duplicate name
        let change = SignatureChange::new().rename_parameter(0, "b");
        assert!(matches!(
            ChangeSignatureRefactoring::validate_change(&sig, &change),
            Err(SignatureError::DuplicateParameterName(_))
        ));

        // Invalid: bad identifier
        let change = SignatureChange::new().with_name("123invalid");
        assert!(matches!(
            ChangeSignatureRefactoring::validate_change(&sig, &change),
            Err(SignatureError::InvalidName(_))
        ));
    }

    #[test]
    fn test_parse_argument_list_simple() {
        // Empty args
        let (end, args) = ChangeSignatureRefactoring::parse_argument_list("()").unwrap();
        assert_eq!(end, 2);
        assert!(args.is_empty());

        // Single arg
        let (end, args) = ChangeSignatureRefactoring::parse_argument_list("(x)").unwrap();
        assert_eq!(end, 3);
        assert_eq!(args, vec!["x"]);

        // Multiple args
        let (end, args) = ChangeSignatureRefactoring::parse_argument_list("(a, b, c)").unwrap();
        assert_eq!(end, 9);
        assert_eq!(args, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_argument_list_nested() {
        // Nested method call
        let (_, args) =
            ChangeSignatureRefactoring::parse_argument_list("(foo(1, 2), bar)").unwrap();
        assert_eq!(args, vec!["foo(1, 2)", "bar"]);

        // Deeply nested
        let (_, args) =
            ChangeSignatureRefactoring::parse_argument_list("(outer(inner(x)))").unwrap();
        assert_eq!(args, vec!["outer(inner(x))"]);
    }

    #[test]
    fn test_parse_argument_list_strings() {
        // String with comma
        let (_, args) = ChangeSignatureRefactoring::parse_argument_list(r#"("a,b", c)"#).unwrap();
        assert_eq!(args, vec![r#""a,b""#, "c"]);

        // String with paren
        let (_, args) = ChangeSignatureRefactoring::parse_argument_list(r#"("()", x)"#).unwrap();
        assert_eq!(args, vec![r#""()""#, "x"]);

        // Escaped quote in string
        let (_, args) =
            ChangeSignatureRefactoring::parse_argument_list(r#"("say \"hi\"", y)"#).unwrap();
        assert_eq!(args, vec![r#""say \"hi\"""#, "y"]);
    }

    #[test]
    fn test_parse_argument_list_char_literals() {
        // Char with comma-like content
        let (_, args) = ChangeSignatureRefactoring::parse_argument_list("(',', x)").unwrap();
        assert_eq!(args, vec!["','", "x"]);

        // Char with paren
        let (_, args) = ChangeSignatureRefactoring::parse_argument_list("(')', z)").unwrap();
        assert_eq!(args, vec!["')'", "z"]);
    }

    #[test]
    fn call_site_rename_replaces_the_callee_and_arguments_as_one_edit() {
        let signature = MethodSignature {
            file_id: FileId::new(7),
            fqn: "p.Subject.oldName(int)".to_owned(),
            name: "oldName".to_owned(),
            return_type: "void".to_owned(),
            return_type_range: None,
            parameters: vec![Parameter::new("value", "int")],
            throws: Vec::new(),
            modifiers: Vec::new(),
            type_parameters: Vec::new(),
            declaration_range: TextRange::default(),
            parameter_list_range: None,
            name_range: None,
        };
        let call_site = CallSite {
            file_id: FileId::new(8),
            invocation_range: TextRange::new(TextSize::new(20), TextSize::new(30)),
            argument_list_range: TextRange::new(TextSize::new(27), TextSize::new(30)),
            arguments: vec!["1".to_owned()],
            is_constructor: false,
            is_chained_constructor: false,
        };

        let edit = ChangeSignatureRefactoring::compute_call_site_edit(
            &call_site,
            &signature,
            &SignatureChange::new().with_name("newName"),
        )
        .expect("rename produces a call-site edit");
        assert_eq!(edit.range, call_site.invocation_range);
        assert_eq!(edit.new_text, "newName(1)");
    }

    #[test]
    fn argument_list_detection_does_not_steal_parentheses_from_later_code() {
        assert_eq!(next_argument_list_start(" /* comment */ (value)"), Some(15));
        assert_eq!(next_argument_list_start("::method; later()"), None);
        assert_eq!(next_argument_list_start("; later()"), None);
        assert_eq!(next_argument_list_start(" // comment\n (value)"), Some(13));
    }
}

#[cfg(test)]
mod return_type_and_refusal_tests {
    use super::*;

    fn signature(
        return_type_range: Option<TextRange>,
        name_range: Option<TextRange>,
    ) -> MethodSignature {
        MethodSignature {
            file_id: FileId::new(0),
            fqn: "Test.method".to_string(),
            name: "method".to_string(),
            return_type: "void".to_string(),
            return_type_range,
            parameters: vec![Parameter::new("a", "int")],
            throws: vec![],
            modifiers: vec![],
            type_parameters: vec![],
            declaration_range: TextRange::default(),
            parameter_list_range: Some(TextRange::new(TextSize::new(20), TextSize::new(26))),
            name_range,
        }
    }

    #[test]
    fn a_return_type_change_is_applied_at_the_return_type() {
        let sig = signature(
            Some(TextRange::new(TextSize::new(4), TextSize::new(8))),
            None,
        );
        let change = SignatureChange {
            new_return_type: Some("int".to_string()),
            ..SignatureChange::new()
        };
        let edits = ChangeSignatureRefactoring::compute_declaration_edits(&sig, &change);
        assert_eq!(
            edits.len(),
            1,
            "a return-type-only change must produce an edit"
        );
        assert_eq!(edits[0].new_text, "int");
        assert_eq!(
            edits[0].range,
            TextRange::new(TextSize::new(4), TextSize::new(8))
        );
    }

    #[test]
    fn a_return_type_change_without_its_range_is_refused() {
        let sig = signature(None, None);
        let change = SignatureChange {
            new_return_type: Some("int".to_string()),
            ..SignatureChange::new()
        };
        assert!(matches!(
            ChangeSignatureRefactoring::validate_change(&sig, &change),
            Err(SignatureError::CannotChange(_))
        ));
    }

    #[test]
    fn a_rename_without_the_name_range_is_refused() {
        let sig = signature(None, None);
        let change = SignatureChange::new().with_name("renamed");
        assert!(matches!(
            ChangeSignatureRefactoring::validate_change(&sig, &change),
            Err(SignatureError::CannotChange(_))
        ));
    }

    #[test]
    fn contextual_keywords_are_valid_parameter_names() {
        for name in ["record", "var", "to", "with", "module", "yield", "sealed"] {
            assert!(
                is_valid_java_identifier(name),
                "{name} is a legal identifier"
            );
        }
        assert!(!is_valid_java_identifier("class"));
    }
}
