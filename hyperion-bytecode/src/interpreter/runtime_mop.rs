// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Runtime Meta-Object Protocol for Groovy
//!
//! This module provides runtime method resolution for Groovy scripts,
//! integrating the MetaClass system with the JVM interpreter.

use super::call_site::{CallSite, CallSiteKind, MethodHandle};
use super::errors::VMError;
use super::method_area::LoadedClass;
use std::collections::HashMap;

fn loaded_class<'a>(
    classes: &'a HashMap<String, LoadedClass>,
    name: &str,
) -> Option<&'a LoadedClass> {
    classes.get(&name.replace('/', "."))
}

/// Runtime MOP for dynamic method dispatch
///
/// Provides Groovy-style method resolution that handles:
/// - Direct method calls (receiver.method())
/// - Property access (receiver.property -> getProperty())
/// - Method missing fallback
/// - Extension methods (DGMM)
pub struct RuntimeMOP {
    /// Cached method handles by (class, method_name, descriptor)
    method_cache: HashMap<(String, String, String), ResolvedMethod>,

    /// Default Groovy Methods registry
    dgmm_methods: HashMap<String, Vec<DGMMMethod>>,
}

/// A resolved method ready for invocation
#[derive(Clone, Debug)]
pub struct ResolvedMethod {
    /// The method handle to invoke
    pub handle: MethodHandle,
    /// Whether this is a DGMM extension method
    pub is_extension: bool,
    /// Whether this uses methodMissing fallback
    pub is_missing_handler: bool,
}

/// Default Groovy Method (extension method)
#[derive(Clone, Debug)]
struct DGMMMethod {
    /// Method name (stored for future DGMM reflection)
    _name: String,
    /// Receiver type this extends
    receiver_type: String,
    /// Method descriptor (stored for future DGMM reflection)
    _descriptor: String,
    /// Implementation class
    impl_class: String,
}

/// Capitalize first character of a string for property accessor naming.
/// Returns None if the string is empty.
fn capitalize_first(s: &str) -> Option<String> {
    let mut chars = s.chars();
    let first = chars.next()?;
    let rest: String = chars.collect();
    Some(format!("{}{}", first.to_uppercase(), rest))
}

impl RuntimeMOP {
    /// Create a new runtime MOP
    pub fn new() -> Self {
        let mut mop = RuntimeMOP {
            method_cache: HashMap::new(),
            dgmm_methods: HashMap::new(),
        };
        mop.register_builtin_dgmm();
        mop
    }

    /// Register built-in Default Groovy Methods
    fn register_builtin_dgmm(&mut self) {
        // Collection methods
        self.add_dgmm("each", "java/util/Collection", "(Lgroovy/lang/Closure;)V");
        self.add_dgmm(
            "collect",
            "java/util/Collection",
            "(Lgroovy/lang/Closure;)Ljava/util/List;",
        );
        self.add_dgmm(
            "find",
            "java/util/Collection",
            "(Lgroovy/lang/Closure;)Ljava/lang/Object;",
        );
        self.add_dgmm(
            "findAll",
            "java/util/Collection",
            "(Lgroovy/lang/Closure;)Ljava/util/Collection;",
        );
        self.add_dgmm("size", "java/util/Collection", "()I");
        self.add_dgmm("isEmpty", "java/util/Collection", "()Z");

        // Object methods
        self.add_dgmm("toString", "java/lang/Object", "()Ljava/lang/String;");
        self.add_dgmm("with", "java/lang/Object", "(Lgroovy/lang/Closure;)V");
        self.add_dgmm(
            "tap",
            "java/lang/Object",
            "(Lgroovy/lang/Closure;)Ljava/lang/Object;",
        );

        // String methods
        self.add_dgmm(
            "plus",
            "java/lang/String",
            "(Ljava/lang/Object;)Ljava/lang/String;",
        );
        self.add_dgmm("multiply", "java/lang/String", "(I)Ljava/lang/String;");

        // Number methods
        self.add_dgmm(
            "plus",
            "java/lang/Number",
            "(Ljava/lang/Number;)Ljava/lang/Number;",
        );
        self.add_dgmm(
            "minus",
            "java/lang/Number",
            "(Ljava/lang/Number;)Ljava/lang/Number;",
        );
        self.add_dgmm("times", "java/lang/Closure", "()V");
    }

    /// Add a DGMM method
    fn add_dgmm(&mut self, name: &str, receiver_type: &str, descriptor: &str) {
        let method = DGMMMethod {
            _name: name.to_string(),
            receiver_type: receiver_type.to_string(),
            _descriptor: descriptor.to_string(),
            impl_class: "org/codehaus/groovy/runtime/DefaultGroovyMethods".to_string(),
        };
        self.dgmm_methods
            .entry(name.to_string())
            .or_default()
            .push(method);
    }

    /// Resolve a method call using Groovy MOP semantics
    ///
    /// Resolution order:
    /// 1. Declared methods on the receiver class
    /// 2. Inherited methods from superclasses
    /// 3. Extension methods (DGMM)
    /// 4. methodMissing fallback
    pub fn resolve_method(
        &mut self,
        receiver_class: &str,
        method_name: &str,
        descriptor: &str,
        loaded_classes: &HashMap<String, LoadedClass>,
    ) -> Result<ResolvedMethod, VMError> {
        let cache_key = (
            receiver_class.to_string(),
            method_name.to_string(),
            descriptor.to_string(),
        );

        // Check cache first
        if let Some(cached) = self.method_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        // 1. Check declared/inherited methods on receiver class (via vtable)
        if let Some(class) = loaded_class(loaded_classes, receiver_class) {
            if class.vtable.iter().any(|m| m.name.as_ref() == method_name) {
                let resolved = ResolvedMethod {
                    handle: MethodHandle::invoke_virtual(receiver_class, method_name, descriptor),
                    is_extension: false,
                    is_missing_handler: false,
                };
                self.method_cache.insert(cache_key, resolved.clone());
                return Ok(resolved);
            }

            // Check superclass chain
            if let Some(ref super_name) = class.superclass {
                let mut current_super = Some(super_name.clone());
                while let Some(ref super_class) = current_super {
                    if let Some(super_loaded) = loaded_class(loaded_classes, super_class) {
                        if super_loaded
                            .vtable
                            .iter()
                            .any(|m| m.name.as_ref() == method_name)
                        {
                            let resolved = ResolvedMethod {
                                handle: MethodHandle::invoke_virtual(
                                    super_class,
                                    method_name,
                                    descriptor,
                                ),
                                is_extension: false,
                                is_missing_handler: false,
                            };
                            self.method_cache.insert(cache_key, resolved.clone());
                            return Ok(resolved);
                        }
                        current_super = super_loaded.superclass.clone();
                    } else {
                        break;
                    }
                }
            }
        }

        // Allow gradle/* receiver types even when classes are not loaded.
        if receiver_class.starts_with("gradle/") {
            let resolved = ResolvedMethod {
                handle: MethodHandle::invoke_virtual(receiver_class, method_name, descriptor),
                is_extension: false,
                is_missing_handler: false,
            };
            self.method_cache.insert(cache_key, resolved.clone());
            return Ok(resolved);
        }

        // 2. Check DGMM extension methods
        if let Some(dgmm_list) = self.dgmm_methods.get(method_name) {
            // Find matching extension for receiver type or its supertypes
            for dgmm in dgmm_list {
                if self.is_assignable_from(receiver_class, &dgmm.receiver_type, loaded_classes) {
                    let resolved = ResolvedMethod {
                        handle: MethodHandle::invoke_static(
                            &dgmm.impl_class,
                            method_name,
                            // DGMM methods prepend receiver as first arg
                            &format!("(L{};{}", dgmm.receiver_type, &descriptor[1..]),
                        ),
                        is_extension: true,
                        is_missing_handler: false,
                    };
                    self.method_cache.insert(cache_key, resolved.clone());
                    return Ok(resolved);
                }
            }
        }

        // 3. Check for methodMissing
        if let Some(class) = loaded_class(loaded_classes, receiver_class) {
            if class
                .vtable
                .iter()
                .any(|m| m.name.as_ref() == "methodMissing")
            {
                let resolved = ResolvedMethod {
                    handle: MethodHandle::invoke_virtual(
                        receiver_class,
                        "methodMissing",
                        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;",
                    ),
                    is_extension: false,
                    is_missing_handler: true,
                };
                self.method_cache.insert(cache_key, resolved.clone());
                return Ok(resolved);
            }
        }

        // 4. No method found
        Err(VMError::NoSuchMethod(format!(
            "{}::{}{}",
            receiver_class, method_name, descriptor
        )))
    }

    /// Resolve a property access (converts to getter call)
    pub fn resolve_property_get(
        &mut self,
        receiver_class: &str,
        property_name: &str,
        loaded_classes: &HashMap<String, LoadedClass>,
    ) -> Result<ResolvedMethod, VMError> {
        // Handle empty property name early
        let capitalized = match capitalize_first(property_name) {
            Some(cap) => cap,
            None => {
                return Err(VMError::NoSuchMethod(format!(
                    "Empty property name on {}",
                    receiver_class
                )));
            }
        };

        // Try getXxx first
        let getter_name = format!("get{}", capitalized);

        // Try getter method
        if let Ok(resolved) = self.resolve_method(
            receiver_class,
            &getter_name,
            "()Ljava/lang/Object;",
            loaded_classes,
        ) {
            return Ok(resolved);
        }

        // Try isXxx for booleans
        let is_getter = format!("is{}", capitalized);
        if let Ok(resolved) = self.resolve_method(receiver_class, &is_getter, "()Z", loaded_classes)
        {
            return Ok(resolved);
        }

        // Check for propertyMissing
        if let Some(class) = loaded_class(loaded_classes, receiver_class) {
            if class
                .vtable
                .iter()
                .any(|m| m.name.as_ref() == "propertyMissing")
            {
                return Ok(ResolvedMethod {
                    handle: MethodHandle::invoke_virtual(
                        receiver_class,
                        "propertyMissing",
                        "(Ljava/lang/String;)Ljava/lang/Object;",
                    ),
                    is_extension: false,
                    is_missing_handler: true,
                });
            }
        }

        Err(VMError::NoSuchMethod(format!(
            "Property '{}' not found on {}",
            property_name, receiver_class
        )))
    }

    /// Resolve a property setter
    pub fn resolve_property_set(
        &mut self,
        receiver_class: &str,
        property_name: &str,
        loaded_classes: &HashMap<String, LoadedClass>,
    ) -> Result<ResolvedMethod, VMError> {
        // Handle empty property name early
        let capitalized = match capitalize_first(property_name) {
            Some(cap) => cap,
            None => {
                return Err(VMError::NoSuchMethod(format!(
                    "Empty property name on {}",
                    receiver_class
                )));
            }
        };

        let setter_name = format!("set{}", capitalized);

        if let Ok(resolved) = self.resolve_method(
            receiver_class,
            &setter_name,
            "(Ljava/lang/Object;)V",
            loaded_classes,
        ) {
            return Ok(resolved);
        }

        // Check for propertyMissing with setter signature
        if let Some(class) = loaded_class(loaded_classes, receiver_class) {
            if class
                .vtable
                .iter()
                .any(|m| m.name.as_ref() == "propertyMissing")
            {
                return Ok(ResolvedMethod {
                    handle: MethodHandle::invoke_virtual(
                        receiver_class,
                        "propertyMissing",
                        "(Ljava/lang/String;Ljava/lang/Object;)V",
                    ),
                    is_extension: false,
                    is_missing_handler: true,
                });
            }
        }

        Err(VMError::NoSuchMethod(format!(
            "Property setter '{}' not found on {}",
            property_name, receiver_class
        )))
    }

    /// Check if a class is assignable from another (for DGMM matching)
    fn is_assignable_from(
        &self,
        class: &str,
        target: &str,
        loaded_classes: &HashMap<String, LoadedClass>,
    ) -> bool {
        if class == target {
            return true;
        }

        // Check superclass chain
        if let Some(loaded) = loaded_class(loaded_classes, class) {
            if let Some(ref super_class) = loaded.superclass {
                if self.is_assignable_from(super_class, target, loaded_classes) {
                    return true;
                }
            }

            // Check interfaces
            for iface in &loaded.interfaces {
                if iface == target || self.is_assignable_from(iface, target, loaded_classes) {
                    return true;
                }
            }
        }

        // Common type relationships without full class loading
        matches!(
            (class, target),
            (_, "java/lang/Object")
                | ("java/util/ArrayList", "java/util/Collection")
                | ("java/util/ArrayList", "java/util/List")
                | ("java/util/LinkedList", "java/util/Collection")
                | ("java/util/LinkedList", "java/util/List")
                | ("java/util/HashSet", "java/util/Collection")
                | ("java/util/HashSet", "java/util/Set")
                | ("java/util/HashMap", "java/util/Map")
                | ("java/lang/Integer", "java/lang/Number")
                | ("java/lang/Long", "java/lang/Number")
                | ("java/lang/Double", "java/lang/Number")
                | ("java/lang/Float", "java/lang/Number")
        )
    }

    /// Create a CallSite for invokedynamic using MOP resolution
    pub fn create_call_site(
        &mut self,
        receiver_class: &str,
        method_name: &str,
        descriptor: &str,
        loaded_classes: &HashMap<String, LoadedClass>,
    ) -> Result<CallSite, VMError> {
        let resolved =
            self.resolve_method(receiver_class, method_name, descriptor, loaded_classes)?;

        Ok(CallSite {
            target: resolved.handle,
            name: method_name.to_string(),
            method_type: descriptor.to_string(),
            kind: CallSiteKind::Constant,
        })
    }

    /// Resolve a method call within a closure context
    ///
    /// Implements Groovy's delegate resolution strategy:
    /// - DELEGATE_FIRST (Gradle default): Try delegate, then owner, then closure
    /// - OWNER_FIRST (Groovy default): Try owner, then delegate, then closure
    /// - DELEGATE_ONLY: Only try delegate
    /// - OWNER_ONLY: Only try owner
    /// - TO_SELF: Only try closure itself
    ///
    /// This is the key mechanism that enables Gradle DSL patterns like:
    /// ```groovy
    /// dependencies {
    ///     implementation "com.google.guava:guava:32.1.2"  // Resolves to DependencyHandler
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// - `heap`: For reading closure delegate/owner/resolveStrategy fields
    /// - `closure_id`: HeapId of the executing closure
    /// - `method_name`: Method to resolve
    /// - `descriptor`: Method descriptor (e.g., "(Ljava/lang/String;)V")
    /// - `loaded_classes`: Available classes for resolution
    ///
    /// # Returns
    ///
    /// Returns `(ResolvedMethod, ObjectRef)` where ObjectRef is the receiver to invoke on.
    pub fn resolve_closure_method(
        &mut self,
        heap: &super::heap::Heap,
        closure_id: super::value::HeapId,
        method_name: &str,
        descriptor: &str,
        loaded_classes: &HashMap<String, LoadedClass>,
    ) -> Result<(ResolvedMethod, super::value::ObjectRef), VMError> {
        use super::closure_runtime::{ClosureRuntime, ResolveStrategy};
        use super::value::ObjectRef;

        // Get closure resolution strategy and delegate/owner
        let strategy = ClosureRuntime::get_resolve_strategy(heap, closure_id)?;
        let delegate = ClosureRuntime::get_delegate(heap, closure_id)?;
        let owner = ClosureRuntime::get_owner(heap, closure_id)?;
        // Build resolution targets based on strategy
        let targets: Vec<(ObjectRef, &str)> = match strategy {
            ResolveStrategy::DelegateFirst => {
                vec![(delegate, "delegate"), (owner, "owner")]
            }
            ResolveStrategy::OwnerFirst => {
                vec![(owner, "owner"), (delegate, "delegate")]
            }
            ResolveStrategy::DelegateOnly => {
                vec![(delegate, "delegate")]
            }
            ResolveStrategy::OwnerOnly => {
                vec![(owner, "owner")]
            }
            ResolveStrategy::ToSelf => {
                // Try closure itself (uncommon case)
                vec![]
            }
        };

        // Try each target in order
        let has_targets = !targets.is_empty();
        for (target, _label) in targets {
            if let ObjectRef::Object(target_id) = target {
                let class_name = match heap.get_object_class_name(target_id) {
                    Ok(name) => name,
                    Err(_) => continue, // Invalid reference, skip
                };

                // Try to resolve method on this target
                if let Ok(resolved) =
                    self.resolve_method(&class_name, method_name, descriptor, loaded_classes)
                {
                    // Successfully resolved via delegate/owner
                    return Ok((resolved, target));
                }
            }
        }

        // If TO_SELF strategy or no delegate/owner resolution worked, try closure itself
        if matches!(strategy, ResolveStrategy::ToSelf) || has_targets {
            let closure_class = heap.get_object_class_name(closure_id)?;
            if let Ok(resolved) =
                self.resolve_method(&closure_class, method_name, descriptor, loaded_classes)
            {
                // Successfully resolved on closure itself
                return Ok((resolved, ObjectRef::Object(closure_id)));
            }
        }

        // No resolution found
        Err(VMError::NoSuchMethod(format!(
            "Method '{}{}' not found in closure context (strategy: {:?})",
            method_name, descriptor, strategy
        )))
    }

    /// Clear method cache (for testing or class redefinition)
    pub fn clear_cache(&mut self) {
        self.method_cache.clear();
    }
}

impl Default for RuntimeMOP {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::method_area::{ClassInitState, FieldLayout, VTableEntry};
    use std::sync::Arc;

    fn create_test_class(name: &str, methods: Vec<&str>) -> LoadedClass {
        LoadedClass {
            name: name.to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            vtable: methods
                .iter()
                .map(|m| {
                    VTableEntry::new(
                        m.to_string(),
                        "()V".to_string(),
                        name.to_string(),
                        0x0001,
                        false,
                    )
                })
                .collect(),
            resolved_virtual_methods: Vec::new(),
            non_virtual_methods: vec![],
            itable: vec![],
            field_layout: FieldLayout::new(),
            init_state: ClassInitState::Initialized,
            declared_fields: Arc::from([]),
            constant_field_values: Arc::from([]),
            modifiers: 0x0001, // PUBLIC
            is_interface: false,
            is_native_stub: false,
            default_conflict_count: 0,
            source_file: None,
            nest: None,
            bootstrap_methods: vec![],
        }
    }

    #[test]
    fn test_resolve_direct_method() {
        let mut mop = RuntimeMOP::new();
        let mut classes = HashMap::new();
        classes.insert(
            "MyClass".to_string(),
            create_test_class("MyClass", vec!["foo", "bar"]),
        );

        let result = mop.resolve_method("MyClass", "foo", "()V", &classes);
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(!resolved.is_extension);
        assert!(!resolved.is_missing_handler);
    }

    #[test]
    fn resolves_slash_named_receiver_through_canonical_superclass_registry() {
        let mut mop = RuntimeMOP::new();
        let mut classes = HashMap::new();
        classes.insert(
            "example.Base".to_string(),
            create_test_class("example.Base", vec!["inherited"]),
        );
        let mut child = create_test_class("example.Child", vec![]);
        child.superclass = Some("example/Base".to_string());
        classes.insert("example.Child".to_string(), child);

        assert!(mop
            .resolve_method("example/Child", "inherited", "()V", &classes)
            .is_ok());
    }

    #[test]
    fn test_method_not_found() {
        let mut mop = RuntimeMOP::new();
        let classes = HashMap::new();

        let result = mop.resolve_method("MyClass", "nonexistent", "()V", &classes);
        assert!(result.is_err());
    }

    #[test]
    fn test_method_missing_fallback() {
        let mut mop = RuntimeMOP::new();
        let mut classes = HashMap::new();
        classes.insert(
            "DynamicClass".to_string(),
            create_test_class("DynamicClass", vec!["methodMissing"]),
        );

        let result = mop.resolve_method("DynamicClass", "anyMethod", "()V", &classes);
        assert!(result.is_ok());
        let resolved = result.unwrap();
        assert!(resolved.is_missing_handler);
    }

    #[test]
    fn test_property_get() {
        let mut mop = RuntimeMOP::new();
        let mut classes = HashMap::new();
        classes.insert(
            "User".to_string(),
            create_test_class("User", vec!["getName", "getAge"]),
        );

        let result = mop.resolve_property_get("User", "name", &classes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_caching() {
        let mut mop = RuntimeMOP::new();
        let mut classes = HashMap::new();
        classes.insert(
            "MyClass".to_string(),
            create_test_class("MyClass", vec!["test"]),
        );

        // First call
        let _ = mop.resolve_method("MyClass", "test", "()V", &classes);
        assert_eq!(mop.method_cache.len(), 1);

        // Second call should hit cache
        let _ = mop.resolve_method("MyClass", "test", "()V", &classes);
        assert_eq!(mop.method_cache.len(), 1);
    }

    #[test]
    fn test_empty_property_name_get() {
        let mut mop = RuntimeMOP::new();
        let classes = HashMap::new();

        // Empty property name should return error, not panic
        let result = mop.resolve_property_get("SomeClass", "", &classes);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty property name"));
    }

    #[test]
    fn test_empty_property_name_set() {
        let mut mop = RuntimeMOP::new();
        let classes = HashMap::new();

        // Empty property name should return error, not panic
        let result = mop.resolve_property_set("SomeClass", "", &classes);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty property name"));
    }
}
