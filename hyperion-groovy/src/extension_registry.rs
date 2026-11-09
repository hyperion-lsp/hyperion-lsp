// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! DefaultGroovyMethods (DGMM) extension method registry
//!
//! Implements Groovy's built-in extension methods that are available on all objects.
//! These methods are added via MOP at runtime and provide convenient manipulation methods
//! for collections, strings, files, and objects.

use crate::meta_class::MetaMethod;
use std::collections::HashMap;

/// Registry of DefaultGroovyMethods extension methods
pub struct ExtensionRegistry {
    /// Map from receiver type to methods
    extensions: HashMap<String, Vec<MetaMethod>>,
}

impl ExtensionRegistry {
    /// Create a new extension registry with built-in DGMM methods
    pub fn new() -> Self {
        let mut registry = ExtensionRegistry {
            extensions: HashMap::new(),
        };
        registry.load_builtin_methods();
        registry
    }

    /// Load all built-in DefaultGroovyMethods
    fn load_builtin_methods(&mut self) {
        // Object methods (available on all objects)
        self.add_object_methods();

        // Collection/Iterable methods (List, Collection, etc.)
        self.add_collection_methods();

        // String methods
        self.add_string_methods();

        // List methods
        self.add_list_methods();

        // Map methods
        self.add_map_methods();

        // File methods
        self.add_file_methods();

        // Closure methods
        self.add_closure_methods();

        // Number methods
        self.add_number_methods();
    }

    /// Add a method to the registry
    fn add_method(&mut self, receiver_type: impl Into<String>, method: MetaMethod) {
        self.extensions
            .entry(receiver_type.into())
            .or_default()
            .push(method);
    }

    /// Get all extension methods for a receiver type
    pub fn methods_for_type(&self, receiver_type: &str) -> Vec<MetaMethod> {
        self.extensions
            .get(receiver_type)
            .cloned()
            .unwrap_or_default()
    }

    /// Check if a method exists for a receiver type
    pub fn has_method(&self, receiver_type: &str, method_name: &str) -> bool {
        if let Some(methods) = self.extensions.get(receiver_type) {
            methods.iter().any(|m| m.name == method_name)
        } else {
            false
        }
    }

    // --- Object Methods ---

    fn add_object_methods(&mut self) {
        let object_methods = vec![
            MetaMethod {
                name: "with".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "tap".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "dump".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "inspect".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "identity".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "is".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "print".to_string(),
                parameter_types: vec![],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "println".to_string(),
                parameter_types: vec![],
                return_type: "void".to_string(),
                synthetic: false,
            },
        ];

        for method in object_methods {
            self.add_method("Object", method);
        }
    }

    // --- Collection Methods ---

    fn add_collection_methods(&mut self) {
        let collection_types = vec!["Collection", "Iterable", "List", "Set"];

        let collection_methods = vec![
            MetaMethod {
                name: "each".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "eachWithIndex".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "collect".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "find".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "findAll".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "any".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "every".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "inject".to_string(),
                parameter_types: vec!["Object".to_string(), "Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "reduce".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "groupBy".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Map".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "sort".to_string(),
                parameter_types: vec![],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "sort".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "unique".to_string(),
                parameter_types: vec![],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "flatten".to_string(),
                parameter_types: vec![],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "join".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "sum".to_string(),
                parameter_types: vec![],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "max".to_string(),
                parameter_types: vec![],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "min".to_string(),
                parameter_types: vec![],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "size".to_string(),
                parameter_types: vec![],
                return_type: "int".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "isEmpty".to_string(),
                parameter_types: vec![],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
        ];

        for collection_type in collection_types {
            for method in &collection_methods {
                self.add_method(collection_type, method.clone());
            }
        }
    }

    // --- String Methods ---

    fn add_string_methods(&mut self) {
        let string_methods = vec![
            MetaMethod {
                name: "capitalize".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "uncapitalize".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "reverse".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "padLeft".to_string(),
                parameter_types: vec!["int".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "padLeft".to_string(),
                parameter_types: vec!["int".to_string(), "String".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "padRight".to_string(),
                parameter_types: vec!["int".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "padRight".to_string(),
                parameter_types: vec!["int".to_string(), "String".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "tokenize".to_string(),
                parameter_types: vec![],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "tokenize".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "split".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "replaceAll".to_string(),
                parameter_types: vec!["String".to_string(), "String".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "replaceFirst".to_string(),
                parameter_types: vec!["String".to_string(), "String".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "toUpperCase".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "toLowerCase".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "tr".to_string(),
                parameter_types: vec!["String".to_string(), "String".to_string()],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "startsWith".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "endsWith".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "contains".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "count".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "int".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "matches".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "compareTo".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "int".to_string(),
                synthetic: false,
            },
        ];

        for method in string_methods {
            self.add_method("String", method);
        }
    }

    // --- List Methods ---

    fn add_list_methods(&mut self) {
        let list_methods = vec![
            MetaMethod {
                name: "getAt".to_string(),
                parameter_types: vec!["int".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "putAt".to_string(),
                parameter_types: vec!["int".to_string(), "Object".to_string()],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "first".to_string(),
                parameter_types: vec![],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "last".to_string(),
                parameter_types: vec![],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "head".to_string(),
                parameter_types: vec!["int".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "tail".to_string(),
                parameter_types: vec![],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "init".to_string(),
                parameter_types: vec![],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "add".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "remove".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "remove".to_string(),
                parameter_types: vec!["int".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "clear".to_string(),
                parameter_types: vec![],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "contains".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "indexOf".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "int".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "plus".to_string(),
                parameter_types: vec!["List".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "minus".to_string(),
                parameter_types: vec!["List".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
        ];

        for method in list_methods {
            self.add_method("List", method);
        }
    }

    // --- Map Methods ---

    fn add_map_methods(&mut self) {
        let map_methods = vec![
            MetaMethod {
                name: "getAt".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "putAt".to_string(),
                parameter_types: vec!["Object".to_string(), "Object".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "get".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "put".to_string(),
                parameter_types: vec!["Object".to_string(), "Object".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "keySet".to_string(),
                parameter_types: vec![],
                return_type: "Set".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "values".to_string(),
                parameter_types: vec![],
                return_type: "Collection".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "entrySet".to_string(),
                parameter_types: vec![],
                return_type: "Set".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "containsKey".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "containsValue".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "remove".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "clear".to_string(),
                parameter_types: vec![],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "each".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Map".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "eachWithIndex".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Map".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "collect".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "List".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "find".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Map".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "findAll".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Map".to_string(),
                synthetic: false,
            },
        ];

        for method in map_methods {
            self.add_method("Map", method);
        }
    }

    // --- File Methods ---

    fn add_file_methods(&mut self) {
        let file_methods = vec![
            MetaMethod {
                name: "getText".to_string(),
                parameter_types: vec![],
                return_type: "String".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "getBytes".to_string(),
                parameter_types: vec![],
                return_type: "byte[]".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "eachLine".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "withInputStream".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "withOutputStream".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "write".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "append".to_string(),
                parameter_types: vec!["String".to_string()],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "delete".to_string(),
                parameter_types: vec![],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "exists".to_string(),
                parameter_types: vec![],
                return_type: "boolean".to_string(),
                synthetic: false,
            },
        ];

        for method in file_methods {
            self.add_method("File", method);
        }
    }

    // --- Closure Methods ---

    fn add_closure_methods(&mut self) {
        let closure_methods = vec![
            MetaMethod {
                name: "call".to_string(),
                parameter_types: vec![],
                return_type: "Object".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "curry".to_string(),
                parameter_types: vec!["Object".to_string()],
                return_type: "Closure".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "memoize".to_string(),
                parameter_types: vec![],
                return_type: "Closure".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "getParameterTypes".to_string(),
                parameter_types: vec![],
                return_type: "Class[]".to_string(),
                synthetic: false,
            },
        ];

        for method in closure_methods {
            self.add_method("Closure", method);
        }
    }

    // --- Number Methods ---

    fn add_number_methods(&mut self) {
        let number_types = vec!["Number", "Integer", "Long", "Float", "Double", "BigDecimal"];

        let number_methods = vec![
            MetaMethod {
                name: "abs".to_string(),
                parameter_types: vec![],
                return_type: "Number".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "ceil".to_string(),
                parameter_types: vec![],
                return_type: "Number".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "floor".to_string(),
                parameter_types: vec![],
                return_type: "Number".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "round".to_string(),
                parameter_types: vec![],
                return_type: "Number".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "toInteger".to_string(),
                parameter_types: vec![],
                return_type: "Integer".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "toLong".to_string(),
                parameter_types: vec![],
                return_type: "Long".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "toFloat".to_string(),
                parameter_types: vec![],
                return_type: "Float".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "toDouble".to_string(),
                parameter_types: vec![],
                return_type: "Double".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "times".to_string(),
                parameter_types: vec!["Closure".to_string()],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "upto".to_string(),
                parameter_types: vec!["Number".to_string(), "Closure".to_string()],
                return_type: "void".to_string(),
                synthetic: false,
            },
            MetaMethod {
                name: "downto".to_string(),
                parameter_types: vec!["Number".to_string(), "Closure".to_string()],
                return_type: "void".to_string(),
                synthetic: false,
            },
        ];

        for number_type in number_types {
            for method in &number_methods {
                self.add_method(number_type, method.clone());
            }
        }
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_methods() {
        let registry = ExtensionRegistry::new();
        let methods = registry.methods_for_type("Object");
        assert!(!methods.is_empty());
        assert!(registry.has_method("Object", "with"));
        assert!(registry.has_method("Object", "tap"));
        assert!(registry.has_method("Object", "dump"));
    }

    #[test]
    fn test_collection_methods() {
        let registry = ExtensionRegistry::new();
        assert!(registry.has_method("List", "each"));
        assert!(registry.has_method("List", "collect"));
        assert!(registry.has_method("List", "find"));
        assert!(registry.has_method("Collection", "findAll"));
        assert!(registry.has_method("Iterable", "any"));
    }

    #[test]
    fn test_string_methods() {
        let registry = ExtensionRegistry::new();
        assert!(registry.has_method("String", "capitalize"));
        assert!(registry.has_method("String", "reverse"));
        assert!(registry.has_method("String", "padLeft"));
        assert!(registry.has_method("String", "tokenize"));
    }

    #[test]
    fn test_list_methods() {
        let registry = ExtensionRegistry::new();
        assert!(registry.has_method("List", "getAt"));
        assert!(registry.has_method("List", "putAt"));
        assert!(registry.has_method("List", "first"));
        assert!(registry.has_method("List", "last"));
    }

    #[test]
    fn test_map_methods() {
        let registry = ExtensionRegistry::new();
        assert!(registry.has_method("Map", "keySet"));
        assert!(registry.has_method("Map", "values"));
        assert!(registry.has_method("Map", "collect"));
    }

    #[test]
    fn test_file_methods() {
        let registry = ExtensionRegistry::new();
        assert!(registry.has_method("File", "getText"));
        assert!(registry.has_method("File", "eachLine"));
    }

    #[test]
    fn test_closure_methods() {
        let registry = ExtensionRegistry::new();
        assert!(registry.has_method("Closure", "call"));
        assert!(registry.has_method("Closure", "curry"));
    }

    #[test]
    fn test_number_methods() {
        let registry = ExtensionRegistry::new();
        assert!(registry.has_method("Integer", "abs"));
        assert!(registry.has_method("Double", "ceil"));
        assert!(registry.has_method("Long", "toLong"));
    }

    #[test]
    fn test_get_methods_returns_empty_for_unknown_type() {
        let registry = ExtensionRegistry::new();
        let methods = registry.methods_for_type("UnknownType");
        assert!(methods.is_empty());
    }

    #[test]
    fn test_overloaded_methods() {
        let registry = ExtensionRegistry::new();
        // padLeft has two overloads
        assert!(registry.has_method("String", "padLeft"));
        let methods = registry.methods_for_type("String");
        let pad_left_count = methods.iter().filter(|m| m.name == "padLeft").count();
        assert!(pad_left_count >= 2);
    }
}
