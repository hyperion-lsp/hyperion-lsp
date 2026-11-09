// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! User-Defined Generic Method Registry
//!
//! This module provides type inference for user-defined generic classes,
//! complementing the stdlib registry for standard library functions.
//!
//! # Example
//!
//! For a user-defined class:
//! ```kotlin
//! class Repository<T> {
//!     fun findById(id: Int): T = ...
//! }
//!
//! val repo = Repository<User>()
//! val user = repo.findById(1)  // Should infer User type
//! ```
//!
//! The registry:
//! 1. Looks up the Repository class symbol with type parameter T
//! 2. Finds the findById method with return type T
//! 3. Substitutes T with User (from the receiver type)
//! 4. Returns User as the inferred return type

use crate::types::KotlinType;
use std::collections::HashMap;

/// Information about a generic class from user code
#[derive(Debug, Clone)]
pub struct UserGenericClass {
    /// Fully qualified name
    pub fqn: String,
    /// Type parameter names in order (e.g., ["T"] for Repository<T>)
    pub type_params: Vec<String>,
    /// Methods with their signatures
    pub methods: Vec<UserMethodSignature>,
}

/// Information about a method in a user-defined class
#[derive(Debug, Clone)]
pub struct UserMethodSignature {
    /// Method name
    pub name: String,
    /// Return type as a string (may reference type variables)
    pub return_type: Option<String>,
    /// Parameter types
    pub parameter_types: Vec<String>,
}

/// Registry for user-defined generic classes
///
/// Built dynamically from workspace symbols
#[derive(Debug, Default)]
pub struct UserGenericRegistry {
    /// Map from class FQN to class info
    classes: HashMap<String, UserGenericClass>,
}

impl UserGenericRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            classes: HashMap::new(),
        }
    }

    /// Register a generic class
    pub fn register_class(&mut self, class: UserGenericClass) {
        self.classes.insert(class.fqn.clone(), class);
    }

    /// Look up a class by FQN
    pub fn get_class(&self, fqn: &str) -> Option<&UserGenericClass> {
        self.classes.get(fqn)
    }

    /// Infer the return type of a method call on a user-defined generic type
    ///
    /// # Arguments
    ///
    /// * `receiver_type` - The type of the receiver (e.g., Repository<User>)
    /// * `method_name` - The method being called (e.g., "findById")
    ///
    /// # Returns
    ///
    /// The inferred return type with type variables substituted, or None if
    /// the method cannot be resolved
    pub fn infer_method_return_type(
        &self,
        receiver_type: &KotlinType,
        method_name: &str,
    ) -> Option<KotlinType> {
        // Extract class FQN and type arguments from receiver
        let (class_fqn, type_args) = match receiver_type {
            KotlinType::Class {
                fqn,
                type_arguments,
                ..
            } => (fqn.as_str(), type_arguments),
            _ => return None,
        };

        // Look up the class
        let class_info = self.classes.get(class_fqn)?;

        // Find the method
        let method = class_info.methods.iter().find(|m| m.name == method_name)?;

        // Get the return type string
        let return_type_str = method.return_type.as_ref()?;

        // Build substitution map: type param name -> concrete type
        let mut substitutions: HashMap<String, KotlinType> = HashMap::new();
        for (i, param_name) in class_info.type_params.iter().enumerate() {
            if let Some(crate::types::TypeArgument::Type(concrete_type)) = type_args.get(i) {
                substitutions.insert(param_name.clone(), concrete_type.clone());
            }
        }

        // Substitute type variables in return type
        substitute_type_string(return_type_str, &substitutions)
    }
}

/// Substitute type variables in a type string with concrete types
///
/// For simple cases like "T" -> User, this returns the concrete type.
/// For complex cases like "List<T>" -> List<User>, we build a generic type.
fn substitute_type_string(
    type_str: &str,
    substitutions: &HashMap<String, KotlinType>,
) -> Option<KotlinType> {
    let type_str = type_str.trim();

    // Check for nullable type
    if let Some(inner) = type_str.strip_suffix('?') {
        return substitute_type_string(inner, substitutions)
            .map(super::types::KotlinType::nullable);
    }

    // Check for direct type variable substitution
    if let Some(concrete) = substitutions.get(type_str) {
        return Some(concrete.clone());
    }

    // Check for generic type like List<T>
    if let Some(lt_pos) = type_str.find('<') {
        let base_name = &type_str[..lt_pos];
        let args_str = &type_str[lt_pos + 1..type_str.len() - 1]; // Remove < and >

        // Parse type arguments (simplified: split by comma)
        let type_args: Vec<KotlinType> = args_str
            .split(',')
            .filter_map(|arg| substitute_type_string(arg.trim(), substitutions))
            .collect();

        return Some(KotlinType::generic(base_name, type_args));
    }

    // Return as-is (concrete type name)
    Some(KotlinType::simple(type_str))
}

/// Build a registry from workspace symbols
///
/// This function takes workspace symbols and builds a registry of user-defined
/// generic classes with their methods.
pub fn build_registry_from_symbols<'a, I>(symbols: I) -> UserGenericRegistry
where
    I: Iterator<
        Item = (
            &'a str,
            &'a [String],
            Option<&'a str>,
            &'a str,
            Option<&'a str>,
            &'a [String],
        ),
    >,
    // (symbol_fqn, type_params, container, simple_name, return_type, parameter_types)
{
    let mut registry = UserGenericRegistry::new();
    let mut class_methods: HashMap<String, Vec<UserMethodSignature>> = HashMap::new();
    let mut class_type_params: HashMap<String, Vec<String>> = HashMap::new();

    for (fqn, type_params, container, simple_name, return_type, parameter_types) in symbols {
        // Check if this is a class/interface with type parameters
        if !type_params.is_empty() && container.is_none() {
            // This is a generic class definition
            class_type_params.insert(fqn.to_string(), type_params.to_vec());
        }

        // Check if this is a method in a class
        if let Some(container_fqn) = container {
            let method_sig = UserMethodSignature {
                name: simple_name.to_string(),
                return_type: return_type.map(String::from),
                parameter_types: parameter_types.to_vec(),
            };

            class_methods
                .entry(container_fqn.to_string())
                .or_default()
                .push(method_sig);
        }
    }

    // Build final registry
    for (fqn, type_params) in class_type_params {
        let methods = class_methods.remove(&fqn).unwrap_or_default();
        registry.register_class(UserGenericClass {
            fqn,
            type_params,
            methods,
        });
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Nullability, TypeArgument};

    #[test]
    fn test_substitute_simple_type_var() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::simple("User"));

        let result = substitute_type_string("T", &subs);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "User");
    }

    #[test]
    fn test_substitute_generic_type() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::simple("User"));

        let result = substitute_type_string("List<T>", &subs);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<User>");
    }

    #[test]
    fn test_substitute_nullable_type() {
        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::simple("User"));

        let result = substitute_type_string("T?", &subs);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "User?");
    }

    #[test]
    fn test_infer_method_return_type() {
        let mut registry = UserGenericRegistry::new();
        registry.register_class(UserGenericClass {
            fqn: "com.example.Repository".to_string(),
            type_params: vec!["T".to_string()],
            methods: vec![
                UserMethodSignature {
                    name: "findById".to_string(),
                    return_type: Some("T".to_string()),
                    parameter_types: vec![],
                },
                UserMethodSignature {
                    name: "findAll".to_string(),
                    return_type: Some("List<T>".to_string()),
                    parameter_types: vec![],
                },
            ],
        });

        // Test: Repository<User>.findById() should return User
        let receiver = KotlinType::Class {
            fqn: "com.example.Repository".to_string(),
            type_arguments: vec![TypeArgument::Type(KotlinType::simple("User"))],
            nullability: Nullability::NonNull,
        };

        let result = registry.infer_method_return_type(&receiver, "findById");
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "User");

        // Test: Repository<User>.findAll() should return List<User>
        let result = registry.infer_method_return_type(&receiver, "findAll");
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<User>");
    }

    #[test]
    fn test_infer_method_return_type_two_params() {
        let mut registry = UserGenericRegistry::new();
        registry.register_class(UserGenericClass {
            fqn: "com.example.Pair".to_string(),
            type_params: vec!["A".to_string(), "B".to_string()],
            methods: vec![
                UserMethodSignature {
                    name: "first".to_string(),
                    return_type: Some("A".to_string()),
                    parameter_types: vec![],
                },
                UserMethodSignature {
                    name: "second".to_string(),
                    return_type: Some("B".to_string()),
                    parameter_types: vec![],
                },
            ],
        });

        // Test: Pair<String, Int>.first() should return String
        let receiver = KotlinType::Class {
            fqn: "com.example.Pair".to_string(),
            type_arguments: vec![
                TypeArgument::Type(KotlinType::string()),
                TypeArgument::Type(KotlinType::int()),
            ],
            nullability: Nullability::NonNull,
        };

        let result = registry.infer_method_return_type(&receiver, "first");
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "String");

        let result = registry.infer_method_return_type(&receiver, "second");
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Int");
    }
}
