// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy MetaClass system
//!
//! Implements the core MetaClass registry and MetaObject behavior that powers
//! Groovy's Meta-Object Protocol (MOP). This enables dynamic method dispatch,
//! method missing handling, and runtime method injection.

use std::collections::HashMap;

/// A method signature for stored methods
#[derive(Clone, Debug)]
pub struct MetaMethod {
    /// Method name
    pub name: String,
    /// Parameter types (simplified - just names)
    pub parameter_types: Vec<String>,
    /// Return type
    pub return_type: String,
    /// Whether this is a synthetic method
    pub synthetic: bool,
}

/// Descriptor for a property/field
#[derive(Clone, Debug)]
pub struct MetaProperty {
    /// Property name
    pub name: String,
    /// Property type
    pub property_type: String,
    /// Whether it's readable
    pub readable: bool,
    /// Whether it's writable
    pub writable: bool,
}

/// Represents a single MetaClass for a Groovy class
#[derive(Clone, Debug)]
pub struct MetaClass {
    /// The Java/Groovy class name (fully qualified)
    pub class_name: String,

    /// Superclass metaclass name (if any)
    pub super_class_name: Option<String>,

    /// Declared methods (declared in this class)
    pub declared_methods: HashMap<String, Vec<MetaMethod>>,

    /// Inherited methods (from superclasses and mixins)
    pub inherited_methods: HashMap<String, Vec<MetaMethod>>,

    /// Extension methods (added via Categories or @Mixin)
    pub extension_methods: HashMap<String, Vec<MetaMethod>>,

    /// Properties
    pub properties: HashMap<String, MetaProperty>,

    /// Method missing handler callback (if defined)
    pub method_missing_handler: Option<String>,

    /// Property missing handler callback (if defined)
    pub property_missing_handler: Option<String>,

    /// Custom invoke method (for advanced metaprogramming)
    pub invoke_method_handler: Option<String>,

    /// Get property handler (for intercepting property access)
    pub get_property_handler: Option<String>,

    /// Set property handler (for intercepting property assignment)
    pub set_property_handler: Option<String>,

    /// Whether this class is final
    pub is_final: bool,

    /// Whether this class is abstract
    pub is_abstract: bool,

    /// Whether this class is an interface
    pub is_interface: bool,

    /// User-defined metadata map
    pub metadata: HashMap<String, String>,
}

impl MetaClass {
    /// Create a new MetaClass for a given class name
    pub fn new(class_name: String) -> Self {
        MetaClass {
            class_name,
            super_class_name: None,
            declared_methods: HashMap::new(),
            inherited_methods: HashMap::new(),
            extension_methods: HashMap::new(),
            properties: HashMap::new(),
            method_missing_handler: None,
            property_missing_handler: None,
            invoke_method_handler: None,
            get_property_handler: None,
            set_property_handler: None,
            is_final: false,
            is_abstract: false,
            is_interface: false,
            metadata: HashMap::new(),
        }
    }

    /// Add a declared method to this metaclass
    pub fn add_method(&mut self, method: MetaMethod) {
        self.declared_methods
            .entry(method.name.clone())
            .or_default()
            .push(method);
    }

    /// Add a property descriptor
    pub fn add_property(&mut self, property: MetaProperty) {
        self.properties.insert(property.name.clone(), property);
    }

    /// Get all methods by name (combining declared, inherited, and extensions)
    pub fn get_methods(&self, name: &str) -> Vec<&MetaMethod> {
        let mut methods = Vec::new();

        // First check declared methods (highest priority)
        if let Some(declared) = self.declared_methods.get(name) {
            methods.extend(declared.iter());
        }

        // Then check inherited methods
        if let Some(inherited) = self.inherited_methods.get(name) {
            methods.extend(inherited.iter());
        }

        // Finally check extension methods
        if let Some(extensions) = self.extension_methods.get(name) {
            methods.extend(extensions.iter());
        }

        methods
    }

    /// Check if this metaclass has a method with the given name
    pub fn has_method(&self, name: &str) -> bool {
        self.declared_methods.contains_key(name)
            || self.inherited_methods.contains_key(name)
            || self.extension_methods.contains_key(name)
    }

    /// Check if this metaclass has a property with the given name
    pub fn has_property(&self, name: &str) -> bool {
        self.properties.contains_key(name)
    }
}

/// Global registry of MetaClasses
pub struct MetaClassRegistry {
    /// Map from class name to MetaClass
    classes: HashMap<String, MetaClass>,
}

impl MetaClassRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        MetaClassRegistry {
            classes: HashMap::new(),
        }
    }

    /// Register or get a metaclass for a given class name
    pub fn get_or_create(&mut self, class_name: String) -> &mut MetaClass {
        self.classes
            .entry(class_name.clone())
            .or_insert_with(|| MetaClass::new(class_name))
    }

    /// Get a metaclass by name
    pub fn get(&self, class_name: &str) -> Option<&MetaClass> {
        self.classes.get(class_name)
    }

    /// Get a mutable reference to a metaclass
    pub fn get_mut(&mut self, class_name: &str) -> Option<&mut MetaClass> {
        self.classes.get_mut(class_name)
    }

    /// Register a new metaclass
    pub fn register(&mut self, metaclass: MetaClass) {
        self.classes.insert(metaclass.class_name.clone(), metaclass);
    }

    /// Get all registered class names
    pub fn class_names(&self) -> Vec<&str> {
        self.classes.keys().map(|s| s.as_str()).collect()
    }

    /// Check if a class is registered
    pub fn contains(&self, class_name: &str) -> bool {
        self.classes.contains_key(class_name)
    }
}

impl Default for MetaClassRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_metaclass() {
        let mc = MetaClass::new("com.example.User".to_string());
        assert_eq!(mc.class_name, "com.example.User");
        assert!(mc.declared_methods.is_empty());
    }

    #[test]
    fn add_method_to_metaclass() {
        let mut mc = MetaClass::new("com.example.User".to_string());
        let method = MetaMethod {
            name: "getName".to_string(),
            parameter_types: vec![],
            return_type: "String".to_string(),
            synthetic: false,
        };

        mc.add_method(method);
        assert!(mc.has_method("getName"));
        assert_eq!(mc.get_methods("getName").len(), 1);
    }

    #[test]
    fn add_property_to_metaclass() {
        let mut mc = MetaClass::new("com.example.User".to_string());
        let prop = MetaProperty {
            name: "name".to_string(),
            property_type: "String".to_string(),
            readable: true,
            writable: true,
        };

        mc.add_property(prop);
        assert!(mc.has_property("name"));
    }

    #[test]
    fn metaclass_registry() {
        let mut registry = MetaClassRegistry::new();

        {
            let mc = registry.get_or_create("com.example.User".to_string());
            let method = MetaMethod {
                name: "test".to_string(),
                parameter_types: vec![],
                return_type: "void".to_string(),
                synthetic: false,
            };
            mc.add_method(method);
        }

        assert!(registry.contains("com.example.User"));
        assert!(registry.get("com.example.User").unwrap().has_method("test"));
    }

    #[test]
    fn method_resolution_order() {
        let mut mc = MetaClass::new("com.example.User".to_string());

        let declared = MetaMethod {
            name: "getValue".to_string(),
            parameter_types: vec![],
            return_type: "int".to_string(),
            synthetic: false,
        };

        let inherited = MetaMethod {
            name: "getValue".to_string(),
            parameter_types: vec!["int".to_string()],
            return_type: "int".to_string(),
            synthetic: false,
        };

        mc.add_method(declared);
        mc.inherited_methods
            .entry("getValue".to_string())
            .or_default()
            .push(inherited);

        let methods = mc.get_methods("getValue");
        assert_eq!(methods.len(), 2);
        // Declared methods come first
        assert_eq!(methods[0].parameter_types.len(), 0);
        assert_eq!(methods[1].parameter_types.len(), 1);
    }
}
