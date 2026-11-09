// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy Categories and Mixins support
//!
//! Implements Groovy's category injection mechanism for temporary (Categories via `use`) and
//! permanent (@Mixin annotations) method additions to classes.
//!
//! **Categories:** Temporary method injection within a scope
//! ```groovy
//! use(StringCategory) {
//!     "hello".reverse()  // Uses reverse() from StringCategory
//! }
//! // reverse() no longer available
//! ```
//!
//! **Mixins:** Permanent method injection at class definition time
//! ```groovy
//! @Mixin(StringCategory)
//! class MyString { }
//! ```

use crate::meta_class::{MetaClass, MetaMethod};
use std::collections::HashMap;

/// Represents a Groovy category (class with static methods for injection)
#[derive(Clone, Debug)]
pub struct Category {
    /// Fully qualified name of the category class
    pub class_name: String,
    /// Methods that can be injected (name -> method definitions)
    pub methods: HashMap<String, Vec<MetaMethod>>,
}

impl Category {
    /// Create a new category
    pub fn new(class_name: String) -> Self {
        Category {
            class_name,
            methods: HashMap::new(),
        }
    }

    /// Add a method to this category
    pub fn add_method(&mut self, receiver_type: String, method: MetaMethod) {
        self.methods.entry(receiver_type).or_default().push(method);
    }

    /// Get methods for a receiver type
    pub fn methods_for_type(&self, receiver_type: &str) -> Vec<MetaMethod> {
        self.methods.get(receiver_type).cloned().unwrap_or_default()
    }

    /// Check if this category has methods for a receiver type
    pub fn has_methods_for_type(&self, receiver_type: &str) -> bool {
        self.methods.contains_key(receiver_type)
    }
}

/// Registry of active categories within a scope
#[derive(Clone, Debug)]
pub struct CategoryRegistry {
    /// Stack of active categories (most recent first)
    active_categories: Vec<Category>,
}

impl CategoryRegistry {
    /// Create a new empty category registry
    pub fn new() -> Self {
        CategoryRegistry {
            active_categories: Vec::new(),
        }
    }

    /// Push a category onto the active stack
    pub fn push_category(&mut self, category: Category) {
        self.active_categories.push(category);
    }

    /// Pop the most recent category from the active stack
    pub fn pop_category(&mut self) -> Option<Category> {
        self.active_categories.pop()
    }

    /// Get all active categories
    pub fn active_categories(&self) -> &[Category] {
        &self.active_categories
    }

    /// Get all methods injected by active categories for a receiver type
    pub fn injected_methods(&self, receiver_type: &str) -> Vec<MetaMethod> {
        let mut methods = Vec::new();

        // Iterate in reverse order (most recent categories first)
        for category in self.active_categories.iter().rev() {
            let cat_methods = category.methods_for_type(receiver_type);
            methods.extend(cat_methods);
        }

        methods
    }

    /// Check if any active category has methods for a receiver type
    pub fn has_injected_methods(&self, receiver_type: &str) -> bool {
        self.active_categories
            .iter()
            .any(|cat| cat.has_methods_for_type(receiver_type))
    }
}

impl Default for CategoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Mixin metadata for @Mixin annotation
#[derive(Clone, Debug)]
pub struct MixinMetadata {
    /// Target class being mixed into
    pub target_class: String,
    /// Mixin category class
    pub mixin_class: String,
    /// Methods added by this mixin
    pub methods: Vec<MetaMethod>,
}

impl MixinMetadata {
    /// Create new mixin metadata
    pub fn new(target_class: String, mixin_class: String) -> Self {
        MixinMetadata {
            target_class,
            mixin_class,
            methods: Vec::new(),
        }
    }

    /// Add a method to this mixin
    pub fn add_method(&mut self, method: MetaMethod) {
        self.methods.push(method);
    }
}

/// Global mixin registry
pub struct MixinRegistry {
    /// Mixins applied to each class
    mixins: HashMap<String, Vec<MixinMetadata>>,
}

impl MixinRegistry {
    /// Create a new mixin registry
    pub fn new() -> Self {
        MixinRegistry {
            mixins: HashMap::new(),
        }
    }

    /// Register a mixin for a target class
    pub fn register_mixin(&mut self, mixin: MixinMetadata) {
        self.mixins
            .entry(mixin.target_class.clone())
            .or_default()
            .push(mixin);
    }

    /// Get all mixins for a target class
    pub fn get_mixins(&self, target_class: &str) -> Vec<&MixinMetadata> {
        self.mixins
            .get(target_class)
            .map(|mixins| mixins.iter().collect())
            .unwrap_or_default()
    }

    /// Get all methods injected by mixins for a target class
    pub fn injected_methods(&self, target_class: &str) -> Vec<MetaMethod> {
        let mut methods = Vec::new();
        if let Some(mixins) = self.mixins.get(target_class) {
            for mixin in mixins {
                methods.extend(mixin.methods.clone());
            }
        }
        methods
    }

    /// Check if a target class has any mixins
    pub fn has_mixins(&self, target_class: &str) -> bool {
        self.mixins.contains_key(target_class)
    }
}

impl Default for MixinRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Add category methods to a MetaClass (simulates use() block behavior)
pub fn apply_category(meta_class: &mut MetaClass, category: &Category) {
    let methods = category.methods_for_type(&meta_class.class_name);
    for method in methods {
        meta_class
            .extension_methods
            .entry(method.name.clone())
            .or_default()
            .push(method);
    }
}

/// Add mixin methods to a MetaClass (simulates @Mixin annotation behavior)
pub fn apply_mixin(meta_class: &mut MetaClass, mixin: &MixinMetadata) {
    for method in &mixin.methods {
        meta_class
            .extension_methods
            .entry(method.name.clone())
            .or_default()
            .push(method.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_category() {
        let category = Category::new("StringCategory".to_string());
        assert_eq!(category.class_name, "StringCategory");
        assert!(category.methods.is_empty());
    }

    #[test]
    fn test_add_method_to_category() {
        let mut category = Category::new("StringCategory".to_string());
        let method = MetaMethod {
            name: "reverse".to_string(),
            parameter_types: vec![],
            return_type: "String".to_string(),
            synthetic: false,
        };

        category.add_method("String".to_string(), method);
        assert!(category.has_methods_for_type("String"));
        assert_eq!(category.methods_for_type("String").len(), 1);
    }

    #[test]
    fn test_category_registry_push_pop() {
        let mut registry = CategoryRegistry::new();
        let category = Category::new("StringCategory".to_string());

        registry.push_category(category.clone());
        assert_eq!(registry.active_categories().len(), 1);

        let popped = registry.pop_category();
        assert!(popped.is_some());
        assert_eq!(registry.active_categories().len(), 0);
    }

    #[test]
    fn test_category_registry_order() {
        let mut registry = CategoryRegistry::new();

        let mut cat1 = Category::new("Category1".to_string());
        let method1 = MetaMethod {
            name: "method1".to_string(),
            parameter_types: vec![],
            return_type: "void".to_string(),
            synthetic: false,
        };
        cat1.add_method("String".to_string(), method1);

        let mut cat2 = Category::new("Category2".to_string());
        let method2 = MetaMethod {
            name: "method2".to_string(),
            parameter_types: vec![],
            return_type: "void".to_string(),
            synthetic: false,
        };
        cat2.add_method("String".to_string(), method2);

        registry.push_category(cat1);
        registry.push_category(cat2);

        // Should have both methods, with cat2's method first (more recent)
        let methods = registry.injected_methods("String");
        assert_eq!(methods.len(), 2);
        assert_eq!(&*methods[0].name, "method2");
        assert_eq!(&*methods[1].name, "method1");
    }

    #[test]
    fn test_mixin_metadata() {
        let mut mixin = MixinMetadata::new("MyClass".to_string(), "StringCategory".to_string());
        let method = MetaMethod {
            name: "reverse".to_string(),
            parameter_types: vec![],
            return_type: "String".to_string(),
            synthetic: false,
        };

        mixin.add_method(method);
        assert_eq!(mixin.methods.len(), 1);
        assert_eq!(mixin.methods[0].name, "reverse");
    }

    #[test]
    fn test_mixin_registry() {
        let mut registry = MixinRegistry::new();

        let mut mixin = MixinMetadata::new("MyClass".to_string(), "StringCategory".to_string());
        let method = MetaMethod {
            name: "reverse".to_string(),
            parameter_types: vec![],
            return_type: "String".to_string(),
            synthetic: false,
        };
        mixin.add_method(method);

        registry.register_mixin(mixin);
        assert!(registry.has_mixins("MyClass"));
        assert_eq!(registry.get_mixins("MyClass").len(), 1);
        assert_eq!(registry.injected_methods("MyClass").len(), 1);
    }

    #[test]
    fn test_apply_category_to_metaclass() {
        let mut meta_class = MetaClass::new("String".to_string());
        let mut category = Category::new("StringCategory".to_string());

        let method = MetaMethod {
            name: "reverse".to_string(),
            parameter_types: vec![],
            return_type: "String".to_string(),
            synthetic: false,
        };
        category.add_method("String".to_string(), method);

        apply_category(&mut meta_class, &category);
        assert!(meta_class.has_method("reverse"));
    }

    #[test]
    fn test_apply_mixin_to_metaclass() {
        let mut meta_class = MetaClass::new("MyClass".to_string());
        let mut mixin = MixinMetadata::new("MyClass".to_string(), "StringCategory".to_string());

        let method = MetaMethod {
            name: "greet".to_string(),
            parameter_types: vec![],
            return_type: "String".to_string(),
            synthetic: false,
        };
        mixin.add_method(method);

        apply_mixin(&mut meta_class, &mixin);
        assert!(meta_class.has_method("greet"));
    }

    #[test]
    fn test_multiple_categories_same_type() {
        let mut registry = CategoryRegistry::new();

        let mut cat1 = Category::new("Category1".to_string());
        let method1 = MetaMethod {
            name: "method1".to_string(),
            parameter_types: vec![],
            return_type: "void".to_string(),
            synthetic: false,
        };
        cat1.add_method("Integer".to_string(), method1);

        let mut cat2 = Category::new("Category2".to_string());
        let method2 = MetaMethod {
            name: "method2".to_string(),
            parameter_types: vec![],
            return_type: "void".to_string(),
            synthetic: false,
        };
        cat2.add_method("Integer".to_string(), method2);

        registry.push_category(cat1);
        registry.push_category(cat2);

        assert!(registry.has_injected_methods("Integer"));
        let methods = registry.injected_methods("Integer");
        assert_eq!(methods.len(), 2);
    }

    #[test]
    fn test_category_isolation() {
        let mut registry = CategoryRegistry::new();

        let mut category = Category::new("StringCategory".to_string());
        let method = MetaMethod {
            name: "reverse".to_string(),
            parameter_types: vec![],
            return_type: "String".to_string(),
            synthetic: false,
        };
        category.add_method("String".to_string(), method);

        registry.push_category(category);
        assert!(registry.has_injected_methods("String"));
        assert!(!registry.has_injected_methods("Integer"));

        registry.pop_category();
        assert!(!registry.has_injected_methods("String"));
    }
}
