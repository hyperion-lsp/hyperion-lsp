// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy Closure Semantics
//!
//! Implements Groovy closure representation and semantics, including:
//! - Closure scope (this, owner, delegate)
//! - Parameter and return type tracking
//! - ResolveStrategy for method dispatch
//! - Closure coercion to Single Abstract Method (SAM) types

use std::collections::HashMap;

/// How a closure resolves method calls and property access
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ResolveStrategy {
    /// Check owner first, then delegate (Groovy default)
    #[default]
    OwnerFirst,
    /// Check delegate first, then owner (Gradle DSL default)
    DelegateFirst,
    /// Only check owner
    OwnerOnly,
    /// Only check delegate
    DelegateOnly,
    /// Check 'this' object first
    ToSelf,
}

impl ResolveStrategy {
    /// Get the default strategy for this level
    pub fn default_for_context(context: &str) -> Self {
        match context {
            "gradle_dependencies" | "gradle_repositories" | "gradle_tasks" | "gradle_block" => {
                ResolveStrategy::DelegateFirst
            }
            _ => ResolveStrategy::OwnerFirst,
        }
    }
}

/// Parameter definition for a closure
#[derive(Clone, Debug)]
pub struct ClosureParameter {
    /// Parameter name (e.g., "it", "value", "k")
    pub name: String,
    /// Inferred or declared type
    pub param_type: String,
}

/// A Groovy closure with full semantic information
#[derive(Clone, Debug)]
pub struct GroovyClosure {
    /// Parameters the closure accepts (empty for implicit 'it')
    pub parameters: Vec<ClosureParameter>,

    /// Return type of the closure (inferred from body or expected type)
    pub return_type: String,

    /// The `this` reference - lexically enclosing class or object
    pub this_object: String,

    /// The `owner` reference - lexically enclosing closure or class
    pub owner: String,

    /// The `delegate` reference - where unresolved calls go (mutable)
    pub delegate: Option<String>,

    /// How method/property resolution works
    pub resolve_strategy: ResolveStrategy,

    /// Source code location (for error reporting)
    pub location: Option<SourceLocation>,

    /// Metadata from the closure
    pub metadata: HashMap<String, String>,
}

/// Source location in the file
#[derive(Clone, Debug)]
pub struct SourceLocation {
    pub line: u32,
    pub column: u32,
    pub file: Option<String>,
}

impl GroovyClosure {
    /// Create a new closure with the given delegate
    pub fn new(
        delegate: String,
        owner: String,
        this_object: String,
        resolve_strategy: ResolveStrategy,
    ) -> Self {
        GroovyClosure {
            parameters: Vec::new(),
            return_type: "void".to_string(),
            this_object,
            owner,
            delegate: Some(delegate),
            resolve_strategy,
            location: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a closure for a Gradle DSL block
    pub fn gradle_block(delegate_type: String, owner: String) -> Self {
        GroovyClosure {
            parameters: Vec::new(),
            return_type: "void".to_string(),
            this_object: owner.clone(),
            owner,
            delegate: Some(delegate_type),
            resolve_strategy: ResolveStrategy::DelegateFirst,
            location: None,
            metadata: HashMap::new(),
        }
    }

    /// Add a parameter to this closure
    pub fn add_parameter(&mut self, name: String, param_type: String) {
        self.parameters.push(ClosureParameter { name, param_type });
    }

    /// Get the implicit 'it' parameter if the closure has a single parameter
    pub fn implicit_it_type(&self) -> Option<&str> {
        if self.parameters.len() == 1 {
            Some(&self.parameters[0].param_type)
        } else {
            None
        }
    }

    /// Set the return type
    pub fn set_return_type(&mut self, return_type: String) {
        self.return_type = return_type;
    }

    /// Set the source location
    pub fn set_location(&mut self, line: u32, column: u32, file: Option<String>) {
        self.location = Some(SourceLocation { line, column, file });
    }

    /// Get the effective receiver for method resolution (delegate or owner)
    pub fn get_method_receiver(&self) -> String {
        match self.resolve_strategy {
            ResolveStrategy::DelegateFirst => {
                self.delegate.clone().unwrap_or_else(|| self.owner.clone())
            }
            ResolveStrategy::OwnerFirst => {
                // Try owner first, fall back to delegate if owner is empty/Object/Script
                // Script is treated like Object - a generic base type with no specific implementation
                if !self.owner.is_empty() && self.owner != "Object" && self.owner != "Script" {
                    self.owner.clone()
                } else {
                    self.delegate.clone().unwrap_or_else(|| self.owner.clone())
                }
            }
            ResolveStrategy::DelegateOnly => self
                .delegate
                .clone()
                .unwrap_or_else(|| "Object".to_string()),
            ResolveStrategy::OwnerOnly => self.owner.clone(),
            ResolveStrategy::ToSelf => self.this_object.clone(),
        }
    }

    /// Check if this closure can be coerced to a SAM type
    pub fn can_coerce_to_sam(&self, sam_interface: &str) -> bool {
        // Simple check: single parameter SAM types
        // More complex coercion would check interface method signature
        !self.parameters.is_empty() || sam_interface == "Callable" || sam_interface == "Runnable"
    }
}

/// Closure context for tracking nested closures
#[derive(Clone, Debug)]
pub struct ClosureContext {
    pub current_closure: GroovyClosure,
    pub parent_closure: Option<Box<ClosureContext>>,
}

impl ClosureContext {
    /// Create a new closure context
    pub fn new(closure: GroovyClosure) -> Self {
        ClosureContext {
            current_closure: closure,
            parent_closure: None,
        }
    }

    /// Create a nested closure context
    pub fn nested(closure: GroovyClosure, parent: ClosureContext) -> Self {
        ClosureContext {
            current_closure: closure,
            parent_closure: Some(Box::new(parent)),
        }
    }

    /// Get the parameter type by name
    pub fn get_parameter_type(&self, name: &str) -> Option<String> {
        if name == "it" {
            self.current_closure.implicit_it_type().map(String::from)
        } else {
            self.current_closure
                .parameters
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.param_type.clone())
        }
    }

    /// Walk up the closure chain to find a variable
    pub fn find_variable_type(&self, name: &str) -> Option<String> {
        // First check current closure parameters
        if let Some(ty) = self.get_parameter_type(name) {
            return Some(ty);
        }

        // Then check parent closure
        if let Some(ref parent) = self.parent_closure {
            parent.find_variable_type(name)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_closure() {
        let closure = GroovyClosure::new(
            "DependencyHandler".to_string(),
            "Project".to_string(),
            "Project".to_string(),
            ResolveStrategy::DelegateFirst,
        );

        assert_eq!(closure.delegate, Some("DependencyHandler".to_string()));
        assert_eq!(closure.owner, "Project");
        assert_eq!(closure.resolve_strategy, ResolveStrategy::DelegateFirst);
    }

    #[test]
    fn test_gradle_block_closure() {
        let closure =
            GroovyClosure::gradle_block("DependencyHandler".to_string(), "Project".to_string());

        assert_eq!(closure.resolve_strategy, ResolveStrategy::DelegateFirst);
        assert_eq!(closure.return_type, "void");
    }

    #[test]
    fn test_add_parameter() {
        let mut closure =
            GroovyClosure::gradle_block("Configuration".to_string(), "Project".to_string());

        closure.add_parameter("config".to_string(), "Configuration".to_string());
        assert_eq!(closure.parameters.len(), 1);
        assert_eq!(closure.parameters[0].name, "config");
    }

    #[test]
    fn test_implicit_it_type() {
        let mut closure = GroovyClosure::gradle_block("Handler".to_string(), "Project".to_string());

        closure.add_parameter("it".to_string(), "String".to_string());
        assert_eq!(closure.implicit_it_type(), Some("String"));
    }

    #[test]
    fn test_implicit_it_multiple_params() {
        let mut closure = GroovyClosure::gradle_block("Handler".to_string(), "Project".to_string());

        closure.add_parameter("a".to_string(), "String".to_string());
        closure.add_parameter("b".to_string(), "Integer".to_string());
        assert_eq!(closure.implicit_it_type(), None);
    }

    #[test]
    fn test_method_receiver_delegate_first() {
        let closure = GroovyClosure::new(
            "Handler".to_string(),
            "Script".to_string(),
            "Script".to_string(),
            ResolveStrategy::DelegateFirst,
        );

        assert_eq!(closure.get_method_receiver(), "Handler");
    }

    #[test]
    fn test_method_receiver_owner_first() {
        let closure = GroovyClosure::new(
            "Handler".to_string(),
            "Script".to_string(),
            "Script".to_string(),
            ResolveStrategy::OwnerFirst,
        );

        // With no owner implementation, defaults to delegate
        assert_eq!(closure.get_method_receiver(), "Handler");
    }

    #[test]
    fn test_method_receiver_owner_only() {
        let closure = GroovyClosure::new(
            "Handler".to_string(),
            "Script".to_string(),
            "Script".to_string(),
            ResolveStrategy::OwnerOnly,
        );

        assert_eq!(closure.get_method_receiver(), "Script");
    }

    #[test]
    fn test_resolve_strategy_default_gradle() {
        assert_eq!(
            ResolveStrategy::default_for_context("gradle_dependencies"),
            ResolveStrategy::DelegateFirst
        );
    }

    #[test]
    fn test_resolve_strategy_default_other() {
        assert_eq!(
            ResolveStrategy::default_for_context("custom"),
            ResolveStrategy::OwnerFirst
        );
    }

    #[test]
    fn test_closure_context_parameter() {
        let mut closure = GroovyClosure::gradle_block("Handler".to_string(), "Project".to_string());
        closure.add_parameter("item".to_string(), "File".to_string());

        let ctx = ClosureContext::new(closure);
        assert_eq!(ctx.get_parameter_type("item"), Some("File".to_string()));
    }

    #[test]
    fn test_closure_context_implicit_it() {
        let mut closure = GroovyClosure::gradle_block("Handler".to_string(), "Project".to_string());
        closure.add_parameter("it".to_string(), "String".to_string());

        let ctx = ClosureContext::new(closure);
        assert_eq!(ctx.get_parameter_type("it"), Some("String".to_string()));
    }

    #[test]
    fn test_nested_closure_context() {
        let mut outer = GroovyClosure::gradle_block("Handler".to_string(), "Project".to_string());
        outer.add_parameter("x".to_string(), "Integer".to_string());

        let mut inner =
            GroovyClosure::gradle_block("NestedHandler".to_string(), "Project".to_string());
        inner.add_parameter("y".to_string(), "String".to_string());

        let outer_ctx = ClosureContext::new(outer);
        let inner_ctx = ClosureContext::nested(inner, outer_ctx);

        // Inner context should find its own parameters
        assert_eq!(
            inner_ctx.get_parameter_type("y"),
            Some("String".to_string())
        );

        // And walk up to find outer parameters
        assert_eq!(
            inner_ctx.find_variable_type("x"),
            Some("Integer".to_string())
        );
    }

    #[test]
    fn test_sam_coercion() {
        let mut closure = GroovyClosure::gradle_block("Handler".to_string(), "Project".to_string());
        closure.add_parameter("it".to_string(), "String".to_string());

        assert!(closure.can_coerce_to_sam("Consumer"));
        assert!(closure.can_coerce_to_sam("Callable"));
    }
}
