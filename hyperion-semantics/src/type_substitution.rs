// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Substitution for Generic Type Parameters
//!
//! This module provides type parameter substitution for generic types, enabling
//! the type inference engine to resolve parameterized types to their concrete forms.
//!
//! # Examples
//!
//! ```rust
//! use hyperion_semantics::TypeSubstitution;
//! use hyperion_semantics::{Type, ClassType};
//!
//! // Create substitution mapping: T -> String
//! let mut substitution = TypeSubstitution::new();
//! substitution.bind("T", Type::Class(ClassType::string()));
//!
//! // Apply to List<T> -> List<String>
//! let list_t = Type::Class(ClassType::with_type_args(
//!     "List".into(),
//!     Some("java.util".into()),
//!     vec![Type::TypeVariable {
//!         name: "T".into(),
//!         bounds: vec![],
//!     }]
//! ));
//!
//! let list_string = substitution.apply(&list_t);
//! // list_string is now List<String>
//! ```

use crate::symbol::{ClassType, Type, TypeParameter};
use std::collections::{HashMap, HashSet};
use tracing::debug;

/// Type parameter substitution engine for generic types
///
/// Maps type parameter names (T, E, K, V) to concrete types and applies
/// substitutions recursively through complex type structures.
#[derive(Debug, Clone, Default)]
pub struct TypeSubstitution {
    /// Mapping from type parameter name to concrete type
    bindings: HashMap<String, Type>,
}

impl TypeSubstitution {
    /// Create a new empty type substitution
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// Create a substitution from a list of type parameters and arguments
    ///
    /// # Arguments
    /// * `params` - Generic type parameters (e.g., ["T", "E"])
    /// * `args` - Concrete type arguments (e.g., [Type::String, Type::Int])
    ///
    /// # Returns
    /// A TypeSubstitution mapping each parameter to its corresponding argument
    pub fn from_params(params: &[TypeParameter], args: &[Type]) -> Self {
        let mut substitution = Self::new();
        for (param, arg) in params.iter().zip(args.iter()) {
            substitution.bind(&param.name, arg.clone());
        }
        substitution
    }

    /// Bind a type parameter to a concrete type
    ///
    /// # Arguments
    /// * `param_name` - The type parameter name (e.g., "T")
    /// * `concrete_type` - The concrete type to substitute (e.g., Type::String)
    pub fn bind(&mut self, param_name: &str, concrete_type: Type) {
        debug!(
            "TypeSubstitution::bind {} -> {:?}",
            param_name, concrete_type
        );
        self.bindings.insert(param_name.to_string(), concrete_type);
    }

    /// Apply substitution to a type, replacing type parameters with concrete types
    ///
    /// This method recursively traverses the type structure, replacing all
    /// type variables with their bound concrete types.
    ///
    /// # Arguments
    /// * `ty` - The type to apply substitution to
    ///
    /// # Returns
    /// A new type with all type parameters replaced by their concrete bindings
    ///
    /// # Examples
    ///
    /// ```
    /// // List<T> with T=String -> List<String>
    /// // Map<K,V> with K=String, V=Integer -> Map<String, Integer>
    /// // T[] with T=String -> String[]
    /// ```
    pub fn apply(&self, ty: &Type) -> Type {
        let mut visited = HashSet::new();
        self.apply_internal(ty, &mut visited)
    }

    /// Internal apply with cycle detection for self-referential type bounds
    ///
    /// This prevents infinite recursion for patterns like `T extends Comparable<T>`
    fn apply_internal(&self, ty: &Type, visited: &mut HashSet<String>) -> Type {
        match ty {
            // Type variable: substitute if bound, otherwise keep as-is
            Type::TypeVariable { name, bounds } => {
                if let Some(concrete_type) = self.bindings.get(name) {
                    debug!(
                        "TypeSubstitution::apply TypeVariable {} -> {:?}",
                        name, concrete_type
                    );
                    concrete_type.clone()
                } else {
                    // Check for cycle: if we're already processing this type variable's bounds,
                    // return without recursing to prevent infinite loop on T extends Comparable<T>
                    if visited.contains(name) {
                        debug!(
                            "TypeSubstitution::apply detected cycle for type variable {}",
                            name
                        );
                        return ty.clone();
                    }

                    // Mark this type variable as being processed
                    visited.insert(name.clone());

                    // Unbound type variable - apply to bounds with cycle detection
                    let result = Type::TypeVariable {
                        name: name.clone(),
                        bounds: bounds
                            .iter()
                            .map(|b| self.apply_internal(b, visited))
                            .collect(),
                    };

                    // Remove from visited after processing (allows same variable in different branches)
                    visited.remove(name);

                    result
                }
            }

            // Class type: apply substitution to type arguments recursively
            Type::Class(class_type) => {
                if class_type.type_arguments.is_empty() {
                    // No type arguments - return as-is
                    Type::Class(class_type.clone())
                } else {
                    let substituted_args: Vec<Type> = class_type
                        .type_arguments
                        .iter()
                        .map(|arg| self.apply_internal(arg, visited))
                        .collect();

                    Type::Class(ClassType::with_type_args(
                        class_type.name.clone(),
                        class_type.package.clone(),
                        substituted_args,
                    ))
                }
            }

            // Array type: apply substitution to element type
            Type::Array {
                element_type,
                dimensions,
            } => {
                let substituted_element = self.apply_internal(element_type, visited);
                Type::Array {
                    element_type: Box::new(substituted_element),
                    dimensions: *dimensions,
                }
            }

            // Wildcard: apply substitution to bounds
            Type::Wildcard {
                upper_bound,
                lower_bound,
            } => Type::Wildcard {
                upper_bound: upper_bound
                    .as_ref()
                    .map(|b| Box::new(self.apply_internal(b, visited))),
                lower_bound: lower_bound
                    .as_ref()
                    .map(|b| Box::new(self.apply_internal(b, visited))),
            },

            // Intersection type: apply substitution to all types in the intersection
            Type::Intersection { types } => Type::Intersection {
                types: types
                    .iter()
                    .map(|t| self.apply_internal(t, visited))
                    .collect(),
            },

            // Union type: apply substitution to all types in the union
            Type::Union { types } => Type::Union {
                types: types
                    .iter()
                    .map(|t| self.apply_internal(t, visited))
                    .collect(),
            },

            // Primitive types, Void, Null, Unknown: no substitution needed
            Type::Primitive(_) | Type::Void | Type::Null | Type::Unknown => ty.clone(),
        }
    }

    /// Check if a type parameter has a binding
    pub fn has_binding(&self, param_name: &str) -> bool {
        self.bindings.contains_key(param_name)
    }

    /// Get the concrete type bound to a type parameter
    pub fn get_binding(&self, param_name: &str) -> Option<&Type> {
        self.bindings.get(param_name)
    }

    /// Get all bindings as a reference to the internal map
    pub const fn bindings(&self) -> &HashMap<String, Type> {
        &self.bindings
    }

    /// Merge another substitution into this one
    ///
    /// If both substitutions have bindings for the same parameter,
    /// the binding from `other` takes precedence.
    pub fn merge(&mut self, other: &Self) {
        for (param, ty) in &other.bindings {
            self.bindings.insert(param.clone(), ty.clone());
        }
    }

    /// Create a substitution by composing two substitutions
    ///
    /// First applies `self`, then applies `other` to the result.
    /// This is useful for nested generic contexts.
    pub fn compose(&self, other: &Self) -> Self {
        let mut composed = Self::new();

        // Apply other to all bindings in self
        for (param, ty) in &self.bindings {
            composed.bind(param, other.apply(ty));
        }

        // Add bindings from other that aren't in self
        for (param, ty) in &other.bindings {
            if !composed.has_binding(param) {
                composed.bind(param, ty.clone());
            }
        }

        composed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::PrimitiveType;

    #[test]
    fn test_bind_and_apply_simple_type_variable() {
        let mut substitution = TypeSubstitution::new();
        substitution.bind("T", Type::Class(ClassType::string()));

        let type_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let result = substitution.apply(&type_var);
        assert!(matches!(result, Type::Class(ref ct) if ct.qualified_name() == "java.lang.String"));
    }

    #[test]
    fn test_apply_to_generic_class() {
        let mut substitution = TypeSubstitution::new();
        substitution.bind("T", Type::Class(ClassType::string()));

        // List<T>
        let list_t = Type::Class(ClassType::with_type_args(
            "List".into(),
            Some("java.util".into()),
            vec![Type::TypeVariable {
                name: "T".into(),
                bounds: vec![],
            }],
        ));

        let result = substitution.apply(&list_t);

        // Should be List<String>
        if let Type::Class(class_type) = result {
            assert_eq!(class_type.qualified_name(), "java.util.List");
            assert!(!class_type.type_arguments.is_empty());
            assert_eq!(class_type.type_arguments.len(), 1);
            assert!(
                matches!(class_type.type_arguments[0], Type::Class(ref ct) if ct.qualified_name() == "java.lang.String")
            );
        } else {
            panic!("Expected Type::Class, got {:?}", result);
        }
    }

    #[test]
    fn test_apply_to_nested_generics() {
        let mut substitution = TypeSubstitution::new();
        substitution.bind("K", Type::Class(ClassType::string()));
        substitution.bind("V", Type::Primitive(PrimitiveType::Int));

        // Map<K, V>
        let map_kv = Type::Class(ClassType::with_type_args(
            "Map".into(),
            Some("java.util".into()),
            vec![
                Type::TypeVariable {
                    name: "K".into(),
                    bounds: vec![],
                },
                Type::TypeVariable {
                    name: "V".into(),
                    bounds: vec![],
                },
            ],
        ));

        let result = substitution.apply(&map_kv);

        // Should be Map<String, Integer>
        if let Type::Class(class_type) = result {
            assert_eq!(class_type.qualified_name(), "java.util.Map");
            assert_eq!(class_type.type_arguments.len(), 2);
            assert!(
                matches!(class_type.type_arguments[0], Type::Class(ref ct) if ct.qualified_name() == "java.lang.String")
            );
            assert!(matches!(
                class_type.type_arguments[1],
                Type::Primitive(PrimitiveType::Int)
            ));
        } else {
            panic!("Expected Type::Class, got {:?}", result);
        }
    }

    #[test]
    fn test_apply_to_array_type() {
        let mut substitution = TypeSubstitution::new();
        substitution.bind("T", Type::Class(ClassType::string()));

        // T[]
        let array_t = Type::Array {
            element_type: Box::new(Type::TypeVariable {
                name: "T".into(),
                bounds: vec![],
            }),
            dimensions: 1,
        };

        let result = substitution.apply(&array_t);

        // Should be String[]
        if let Type::Array {
            element_type,
            dimensions,
        } = result
        {
            assert_eq!(dimensions, 1);
            assert!(
                matches!(*element_type, Type::Class(ref ct) if ct.qualified_name() == "java.lang.String")
            );
        } else {
            panic!("Expected Type::Array, got {:?}", result);
        }
    }

    #[test]
    fn test_unbound_type_variable() {
        let substitution = TypeSubstitution::new(); // No bindings

        let type_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let result = substitution.apply(&type_var);

        // Should remain as type variable
        assert!(matches!(result, Type::TypeVariable { .. }));
    }

    #[test]
    fn test_primitive_types_unchanged() {
        let substitution = TypeSubstitution::new();

        let int_type = Type::Primitive(PrimitiveType::Int);
        assert_eq!(substitution.apply(&int_type), int_type);

        let void_type = Type::Void;
        assert_eq!(substitution.apply(&void_type), void_type);
    }

    #[test]
    fn test_merge_substitutions() {
        let mut sub1 = TypeSubstitution::new();
        sub1.bind("T", Type::Class(ClassType::string()));

        let mut sub2 = TypeSubstitution::new();
        sub2.bind("E", Type::Primitive(PrimitiveType::Int));

        sub1.merge(&sub2);

        assert!(sub1.has_binding("T"));
        assert!(sub1.has_binding("E"));
    }

    #[test]
    fn test_compose_substitutions() {
        // First substitution: T -> U
        let mut sub1 = TypeSubstitution::new();
        sub1.bind(
            "T",
            Type::TypeVariable {
                name: "U".into(),
                bounds: vec![],
            },
        );

        // Second substitution: U -> String
        let mut sub2 = TypeSubstitution::new();
        sub2.bind("U", Type::Class(ClassType::string()));

        // Compose: T -> U -> String
        let composed = sub1.compose(&sub2);

        let type_var = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![],
        };

        let result = composed.apply(&type_var);
        assert!(matches!(result, Type::Class(ref ct) if ct.qualified_name() == "java.lang.String"));
    }

    #[test]
    fn test_wildcard_substitution() {
        let mut substitution = TypeSubstitution::new();
        substitution.bind("T", Type::Class(ClassType::string()));

        // ? extends T
        let wildcard = Type::Wildcard {
            upper_bound: Some(Box::new(Type::TypeVariable {
                name: "T".into(),
                bounds: vec![],
            })),
            lower_bound: None,
        };

        let result = substitution.apply(&wildcard);

        // Should be ? extends String
        if let Type::Wildcard {
            upper_bound,
            lower_bound,
        } = result
        {
            assert!(lower_bound.is_none());
            assert!(upper_bound.is_some());
            let upper = *upper_bound.unwrap();
            assert!(
                matches!(upper, Type::Class(ref ct) if ct.qualified_name() == "java.lang.String")
            );
        } else {
            panic!("Expected Type::Wildcard, got {:?}", result);
        }
    }

    #[test]
    fn test_self_referential_type_bounds_no_infinite_recursion() {
        // Test for: T extends Comparable<T>
        // This pattern is common in Java (e.g., Enum, Comparable patterns)
        // Previously caused infinite recursion, now should complete with cycle detection
        let substitution = TypeSubstitution::new();

        // Create T with bound Comparable<T>
        let comparable_of_t = Type::Class(ClassType::with_type_args(
            "Comparable".into(),
            Some("java.lang".into()),
            vec![Type::TypeVariable {
                name: "T".into(),
                bounds: vec![], // Inner T has no explicit bounds (cycle would happen here)
            }],
        ));

        let type_var_t = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![comparable_of_t],
        };

        // This should complete without stack overflow
        let result = substitution.apply(&type_var_t);

        // Result should be the same structure (no substitution since T is unbound)
        // but importantly, it shouldn't infinite loop
        assert!(matches!(result, Type::TypeVariable { ref name, .. } if name == "T"));
    }

    #[test]
    fn test_mutual_type_bounds_no_infinite_recursion() {
        // Test for mutual references: T extends U, U extends T (pathological but possible)
        let substitution = TypeSubstitution::new();

        // Create U that references T
        let type_var_u = Type::TypeVariable {
            name: "U".into(),
            bounds: vec![Type::TypeVariable {
                name: "T".into(),
                bounds: vec![],
            }],
        };

        // Create T that references U
        let type_var_t = Type::TypeVariable {
            name: "T".into(),
            bounds: vec![type_var_u],
        };

        // Should complete without infinite recursion
        let result = substitution.apply(&type_var_t);
        assert!(matches!(result, Type::TypeVariable { ref name, .. } if name == "T"));
    }
}
