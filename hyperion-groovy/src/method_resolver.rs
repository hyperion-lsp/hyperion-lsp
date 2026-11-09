// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy Method Resolution using the Meta-Object Protocol
//!
//! Implements method call resolution following Groovy's MOP (Meta-Object Protocol).
//! This enables dynamic method dispatch, method missing handling, and proper
//! resolution order for declared, inherited, and extension methods.

use crate::extension_registry::ExtensionRegistry;
use crate::meta_class::{MetaClassRegistry, MetaMethod};

/// How specific a method match is (for overload resolution)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchQuality {
    /// Exact parameter type match
    Exact = 4,
    /// Parameter types are assignable (subtype relationship)
    Assignable = 3,
    /// Parameter types can be coerced
    Coercible = 2,
    /// Via methodMissing fallback
    MethodMissing = 1,
}

/// A candidate method that could handle a method call
#[derive(Clone, Debug)]
pub struct MethodCandidate {
    /// The method metadata
    pub method: MetaMethod,
    /// How specific this match is
    pub quality: MatchQuality,
    /// Source category (declared, inherited, extension, missing)
    pub source: MethodSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodSource {
    /// Method declared directly on this class
    Declared,
    /// Method inherited from superclass
    Inherited,
    /// Method added via Category or @Mixin
    Extension,
    /// Fallback via methodMissing
    Missing,
}

/// Result of method resolution
#[derive(Clone, Debug)]
pub enum MethodResolution {
    /// Found matching method(s)
    Resolved(Vec<MethodCandidate>),
    /// No method found - methodMissing not available
    NotFound { method_name: String },
    /// Method missing is available as fallback
    UsedMethodMissing { method_name: String },
}

impl MethodResolution {
    /// Check if resolution found at least one method
    pub fn is_resolved(&self) -> bool {
        matches!(
            self,
            MethodResolution::Resolved(_) | MethodResolution::UsedMethodMissing { .. }
        )
    }

    /// Get the best matching candidate (highest quality)
    pub fn best_match(&self) -> Option<MethodCandidate> {
        match self {
            MethodResolution::Resolved(candidates) => {
                // Sort by quality (descending) then by source priority
                let mut sorted = candidates.clone();
                sorted.sort_by(|a, b| {
                    // First by quality (highest first)
                    let quality_cmp = b.quality.cmp(&a.quality);
                    if quality_cmp != std::cmp::Ordering::Equal {
                        return quality_cmp;
                    }
                    // Then by source priority: Declared > Inherited > Extension
                    let source_priority = |s: MethodSource| match s {
                        MethodSource::Declared => 3,
                        MethodSource::Inherited => 2,
                        MethodSource::Extension => 1,
                        MethodSource::Missing => 0,
                    };
                    source_priority(b.source).cmp(&source_priority(a.source))
                });
                sorted.into_iter().next()
            }
            _ => None,
        }
    }
}

/// Argument types for matching against method parameters
pub struct ArgumentTypes {
    /// Class names of the arguments (for type matching)
    pub types: Vec<String>,
}

impl ArgumentTypes {
    /// Create from class names
    pub fn from_classes(types: Vec<String>) -> Self {
        ArgumentTypes { types }
    }

    /// Check if these arguments match a method's parameters
    pub fn matches(&self, method: &MetaMethod) -> MatchQuality {
        // If argument count doesn't match, no match
        if self.types.len() != method.parameter_types.len() {
            return MatchQuality::MethodMissing; // Will fall through to methodMissing
        }

        // Check all parameters for assignability
        let mut all_exact = true;
        for (arg_type, param_type) in self.types.iter().zip(method.parameter_types.iter()) {
            if arg_type == param_type {
                // Exact match for this parameter
                continue;
            } else if self.is_assignable(arg_type, param_type) {
                // Assignable but not exact
                all_exact = false;
            } else if self.is_coercible(arg_type, param_type) {
                // Can be coerced
                all_exact = false;
            } else {
                // No match
                return MatchQuality::MethodMissing;
            }
        }

        if all_exact {
            MatchQuality::Exact
        } else {
            MatchQuality::Coercible
        }
    }

    /// Check if one type is assignable to another
    fn is_assignable(&self, from: &str, to: &str) -> bool {
        // For now, simple rules:
        // - Object can accept anything
        // - Class hierarchy relationships (would need registry)
        // - Primitive boxing: int -> Integer, etc.

        if to == "Object" {
            return true; // Any type is assignable to Object
        }

        // Primitive boxing relationships
        let boxing_pairs = [
            ("int", "Integer"),
            ("long", "Long"),
            ("float", "Float"),
            ("double", "Double"),
            ("boolean", "Boolean"),
            ("char", "Character"),
            ("byte", "Byte"),
            ("short", "Short"),
        ];

        for (prim, boxed) in &boxing_pairs {
            if from == *prim && to == *boxed {
                return true;
            }
            if from == *boxed && to == *prim {
                return true;
            }
        }

        false
    }

    /// Check if one type can be coerced to another
    fn is_coercible(&self, from: &str, to: &str) -> bool {
        // GString -> String coercion
        if from == "GString" && to == "String" {
            return true;
        }

        // Numeric coercions
        let numeric_types = ["int", "long", "float", "double"];
        if numeric_types.contains(&from) && numeric_types.contains(&to) {
            return true; // Allow numeric coercion
        }

        false
    }
}

/// Resolves method calls using Groovy's Meta-Object Protocol
pub struct MethodResolver {
    /// Registry of all metaclasses
    registry: MetaClassRegistry,
    /// Built-in extension methods (DefaultGroovyMethods)
    extensions: ExtensionRegistry,
}

impl MethodResolver {
    /// Create a new method resolver with a registry
    pub fn new(registry: MetaClassRegistry) -> Self {
        MethodResolver {
            registry,
            extensions: ExtensionRegistry::new(),
        }
    }

    /// Create a method resolver with custom extension registry
    pub fn with_extensions(registry: MetaClassRegistry, extensions: ExtensionRegistry) -> Self {
        MethodResolver {
            registry,
            extensions,
        }
    }

    /// Resolve a method call on a receiver type
    ///
    /// Follows Groovy's method resolution order:
    /// 1. Check invokeMethod handler (if present)
    /// 2. Look for declared methods (exact match preferred)
    /// 3. Look for inherited methods
    /// 4. Look for extension methods (from MetaClass)
    /// 5. Look for DefaultGroovyMethods (DGMM) extensions
    /// 6. Fall back to methodMissing if available
    pub fn resolve_method(
        &self,
        receiver_class: &str,
        method_name: &str,
        args: &ArgumentTypes,
    ) -> MethodResolution {
        let meta_class = match self.registry.get(receiver_class) {
            Some(mc) => mc,
            None => {
                // Class not found in registry - cannot resolve
                return MethodResolution::NotFound {
                    method_name: method_name.to_string(),
                };
            }
        };

        // Collect all candidate methods
        let mut candidates = Vec::new();

        // Step 2: Check declared methods
        let declared = meta_class.get_methods(method_name);
        for method in declared {
            let quality = args.matches(method);
            if quality != MatchQuality::MethodMissing {
                candidates.push(MethodCandidate {
                    method: method.clone(),
                    quality,
                    source: MethodSource::Declared,
                });
            }
        }

        // If we found an exact match, return it immediately
        if candidates.iter().any(|c| c.quality == MatchQuality::Exact) {
            return MethodResolution::Resolved(candidates);
        }

        // Step 3: Check inherited methods (from superclass)
        if let Some(super_class) = &meta_class.super_class_name {
            if let Some(super_meta) = self.registry.get(super_class) {
                let inherited = super_meta.get_methods(method_name);
                for method in inherited {
                    let quality = args.matches(method);
                    if quality != MatchQuality::MethodMissing {
                        candidates.push(MethodCandidate {
                            method: method.clone(),
                            quality,
                            source: MethodSource::Inherited,
                        });
                    }
                }
            }
        }

        // Return if we have good candidates
        if !candidates.is_empty() {
            return MethodResolution::Resolved(candidates);
        }

        // Step 4: Check extension methods
        let extensions = &meta_class.extension_methods;
        if let Some(ext_methods) = extensions.get(method_name) {
            for method in ext_methods {
                let quality = args.matches(method);
                if quality != MatchQuality::MethodMissing {
                    candidates.push(MethodCandidate {
                        method: method.clone(),
                        quality,
                        source: MethodSource::Extension,
                    });
                }
            }
        }

        // Return if we found extensions
        if !candidates.is_empty() {
            return MethodResolution::Resolved(candidates);
        }

        // Step 5: Check DefaultGroovyMethods (DGMM) extensions
        let dgmm_methods = self.extensions.methods_for_type(receiver_class);
        for method in dgmm_methods {
            if method.name == method_name {
                let quality = args.matches(&method);
                if quality != MatchQuality::MethodMissing {
                    candidates.push(MethodCandidate {
                        method,
                        quality,
                        source: MethodSource::Extension,
                    });
                }
            }
        }

        // Return if we found DGMM extensions
        if !candidates.is_empty() {
            return MethodResolution::Resolved(candidates);
        }

        // Step 6: Fall back to methodMissing
        if meta_class.method_missing_handler.is_some() {
            MethodResolution::UsedMethodMissing {
                method_name: method_name.to_string(),
            }
        } else {
            MethodResolution::NotFound {
                method_name: method_name.to_string(),
            }
        }
    }

    /// Resolve a property access
    pub fn resolve_property(&self, receiver_class: &str, property_name: &str) -> Option<String> {
        let meta_class = self.registry.get(receiver_class)?;

        // Check properties first
        if let Some(prop) = meta_class.properties.get(property_name) {
            if prop.readable {
                return Some(prop.property_type.clone());
            }
        }

        // Check for getter method (getXxx or isXxx for booleans)
        let getter_names = vec![
            format!("get{}", capitalize(property_name)),
            format!("is{}", capitalize(property_name)),
        ];

        for getter_name in getter_names {
            let methods = meta_class.get_methods(&getter_name);
            if !methods.is_empty() {
                // Return the first method's return type
                return Some(methods[0].return_type.clone());
            }
        }

        None
    }

    /// Set a property on a receiver
    pub fn resolve_property_setter(
        &self,
        receiver_class: &str,
        property_name: &str,
        _value_type: &str,
    ) -> bool {
        let meta_class = match self.registry.get(receiver_class) {
            Some(mc) => mc,
            None => return false,
        };

        // Check properties first
        if let Some(prop) = meta_class.properties.get(property_name) {
            return prop.writable;
        }

        // Check for setter method (setXxx)
        let setter_name = format!("set{}", capitalize(property_name));
        let methods = meta_class.get_methods(&setter_name);

        // A setter must take exactly one parameter
        methods.iter().any(|m| m.parameter_types.len() == 1)
    }
}

/// Capitalize the first letter of a string
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_class::MetaClassRegistry;

    #[test]
    fn test_argument_types_exact_match() {
        let args = ArgumentTypes::from_classes(vec!["int".to_string(), "String".to_string()]);
        let method = MetaMethod {
            name: "foo".to_string(),
            parameter_types: vec!["int".to_string(), "String".to_string()],
            return_type: "void".to_string(),
            synthetic: false,
        };

        assert_eq!(args.matches(&method), MatchQuality::Exact);
    }

    #[test]
    fn test_argument_types_mismatch() {
        let args = ArgumentTypes::from_classes(vec!["String".to_string()]);
        let method = MetaMethod {
            name: "foo".to_string(),
            parameter_types: vec!["int".to_string()],
            return_type: "void".to_string(),
            synthetic: false,
        };

        assert_eq!(args.matches(&method), MatchQuality::MethodMissing);
    }

    #[test]
    fn test_gstring_to_string_coercion() {
        let args = ArgumentTypes::from_classes(vec!["GString".to_string()]);
        let method = MetaMethod {
            name: "foo".to_string(),
            parameter_types: vec!["String".to_string()],
            return_type: "void".to_string(),
            synthetic: false,
        };

        assert_eq!(args.matches(&method), MatchQuality::Coercible);
    }

    #[test]
    fn test_object_accepts_any_type() {
        let args = ArgumentTypes::from_classes(vec!["String".to_string()]);
        let method = MetaMethod {
            name: "foo".to_string(),
            parameter_types: vec!["Object".to_string()],
            return_type: "void".to_string(),
            synthetic: false,
        };

        assert!(args.matches(&method) > MatchQuality::MethodMissing);
    }

    #[test]
    fn test_primitive_boxing() {
        let args = ArgumentTypes::from_classes(vec!["int".to_string()]);
        let method = MetaMethod {
            name: "foo".to_string(),
            parameter_types: vec!["Integer".to_string()],
            return_type: "void".to_string(),
            synthetic: false,
        };

        assert_eq!(args.matches(&method), MatchQuality::Coercible);
    }

    #[test]
    fn test_method_resolution_not_found() {
        let registry = MetaClassRegistry::new();
        let resolver = MethodResolver::new(registry);

        let args = ArgumentTypes::from_classes(vec![]);
        let result = resolver.resolve_method("NonExistentClass", "foo", &args);

        assert!(!result.is_resolved());
    }

    #[test]
    fn test_match_quality_ordering() {
        assert!(MatchQuality::Exact > MatchQuality::Coercible);
        assert!(MatchQuality::Coercible > MatchQuality::MethodMissing);
    }

    #[test]
    fn test_capitalize() {
        assert_eq!(capitalize("foo"), "Foo");
        assert_eq!(capitalize("fooBar"), "FooBar");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn test_best_match_selection() {
        let candidates = vec![
            MethodCandidate {
                method: MetaMethod {
                    name: "foo".to_string(),
                    parameter_types: vec![],
                    return_type: "void".to_string(),
                    synthetic: false,
                },
                quality: MatchQuality::Coercible,
                source: MethodSource::Declared,
            },
            MethodCandidate {
                method: MetaMethod {
                    name: "foo".to_string(),
                    parameter_types: vec![],
                    return_type: "void".to_string(),
                    synthetic: false,
                },
                quality: MatchQuality::Exact,
                source: MethodSource::Inherited,
            },
        ];

        let resolution = MethodResolution::Resolved(candidates);
        let best = resolution.best_match().unwrap();
        assert_eq!(best.quality, MatchQuality::Exact);
    }
}
