// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Framework registry for managing enabled frameworks and building unified registries

use super::Framework;
use crate::sanitizers::SanitizerRegistry;
use crate::sinks::SinkRegistry;
use crate::sources::SourceRegistry;
use std::collections::HashMap;

/// Registry of frameworks and their security patterns
///
/// Manages which frameworks are enabled and builds unified source/sink/sanitizer
/// registries by combining patterns from all enabled frameworks.
pub struct FrameworkRegistry {
    /// All registered frameworks, keyed by their ID
    frameworks: HashMap<String, Box<dyn Framework>>,

    /// IDs of currently enabled frameworks
    enabled: Vec<String>,

    /// Cached source registry (rebuilt when frameworks change)
    source_registry: Option<SourceRegistry>,

    /// Cached sink registry (rebuilt when frameworks change)
    sink_registry: Option<SinkRegistry>,

    /// Cached sanitizer registry (rebuilt when frameworks change)
    sanitizer_registry: Option<SanitizerRegistry>,
}

impl Default for FrameworkRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameworkRegistry {
    /// Create an empty framework registry
    ///
    /// You'll need to manually register frameworks via `register()` and enable them
    /// via `enable_frameworks()`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::frameworks::FrameworkRegistry;
    ///
    /// let mut registry = FrameworkRegistry::new();
    /// registry.register(Box::new(MyCustomFramework));
    /// registry.enable_frameworks(&["my-custom"]);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            frameworks: HashMap::new(),
            enabled: Vec::new(),
            source_registry: None,
            sink_registry: None,
            sanitizer_registry: None,
        }
    }

    /// Create a registry with all built-in frameworks registered
    ///
    /// All frameworks are enabled by default. Use `enable_frameworks()` to
    /// selectively enable specific frameworks.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::frameworks::FrameworkRegistry;
    ///
    /// // All built-in frameworks enabled
    /// let registry = FrameworkRegistry::with_builtin();
    ///
    /// // Only enable Spring
    /// let mut registry = FrameworkRegistry::with_builtin();
    /// registry.enable_frameworks(&["spring"]);
    /// ```
    #[must_use]
    pub fn with_builtin() -> Self {
        let mut registry = Self::new();

        // Register built-in frameworks
        registry.register(Box::new(super::servlet::ServletFramework));
        registry.register(Box::new(super::spring::SpringFramework));
        registry.register(Box::new(super::jaxrs::JaxRsFramework));
        registry.register(Box::new(super::dropwizard::DropwizardFramework));
        registry.register(Box::new(super::micronaut::MicronautFramework));
        registry.register(Box::new(super::quarkus::QuarkusFramework));
        registry.register(Box::new(super::vertx::VertxFramework));

        // Enable all registered frameworks by default
        registry.enabled = registry.frameworks.keys().cloned().collect();

        registry
    }

    /// Register a new framework
    ///
    /// The framework is not automatically enabled. Call `enable_frameworks()` to activate it.
    ///
    /// # Arguments
    /// * `framework` - Boxed framework implementation
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::frameworks::FrameworkRegistry;
    ///
    /// let mut registry = FrameworkRegistry::new();
    /// registry.register(Box::new(MyCustomFramework));
    /// registry.enable_frameworks(&["my-custom"]);
    /// ```
    pub fn register(&mut self, framework: Box<dyn Framework>) {
        let id = framework.id().to_string();
        self.frameworks.insert(id, framework);
        self.invalidate_caches();
    }

    /// Enable specific frameworks by ID
    ///
    /// Only enabled frameworks will have their patterns included in the unified registries.
    /// This replaces the current set of enabled frameworks.
    ///
    /// # Arguments
    /// * `framework_ids` - Slice of framework IDs to enable
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::frameworks::FrameworkRegistry;
    ///
    /// let mut registry = FrameworkRegistry::with_builtin();
    /// registry.enable_frameworks(&["spring", "jaxrs"]);
    /// ```
    pub fn enable_frameworks(&mut self, framework_ids: &[&str]) {
        self.enabled = framework_ids.iter().map(|s| (*s).to_string()).collect();
        self.invalidate_caches();
    }

    /// Enable all registered frameworks
    pub fn enable_all(&mut self) {
        self.enabled = self.frameworks.keys().cloned().collect();
        self.invalidate_caches();
    }

    /// Get the list of currently enabled framework IDs
    #[must_use]
    pub fn enabled_frameworks(&self) -> &[String] {
        &self.enabled
    }

    /// Check if a framework is registered
    #[must_use]
    pub fn has_framework(&self, id: &str) -> bool {
        self.frameworks.contains_key(id)
    }

    /// Check if a framework is enabled
    #[must_use]
    pub fn is_enabled(&self, id: &str) -> bool {
        self.enabled.contains(&id.to_string())
    }

    /// Rebuild source/sink/sanitizer registries from enabled frameworks
    ///
    /// This creates new empty registries and asks each enabled framework to populate
    /// them with patterns. The registries are cached until frameworks change.
    ///
    /// # Returns
    /// Tuple of `(SourceRegistry, SinkRegistry, SanitizerRegistry)`
    pub fn rebuild_registries(&mut self) -> (&SourceRegistry, &SinkRegistry, &SanitizerRegistry) {
        // Rebuild any invalidated cache before handing out references.
        let cache_valid = self.source_registry.is_some()
            && self.sink_registry.is_some()
            && self.sanitizer_registry.is_some();
        if !cache_valid {
            // Build new registries starting with core (non-framework) patterns.
            // Frameworks then extend these registries with web/framework-specific
            // sources/sinks/sanitizers.
            let mut sources = SourceRegistry::empty();
            sources.register_core_sources();
            let mut sinks = SinkRegistry::empty();
            sinks.register_core_sinks();
            let mut sanitizers = SanitizerRegistry::empty();
            sanitizers.register_core_sanitizers();

            // Ask each enabled framework to register its patterns
            for framework_id in &self.enabled {
                if let Some(framework) = self.frameworks.get(framework_id) {
                    framework.register_sources(&mut sources);
                    framework.register_sinks(&mut sinks);
                    framework.register_sanitizers(&mut sanitizers);
                }
            }

            self.source_registry = Some(sources);
            self.sink_registry = Some(sinks);
            self.sanitizer_registry = Some(sanitizers);
        }

        (
            self.source_registry
                .as_ref()
                .expect("source_registry set above"),
            self.sink_registry
                .as_ref()
                .expect("sink_registry set above"),
            self.sanitizer_registry
                .as_ref()
                .expect("sanitizer_registry set above"),
        )
    }

    /// Get the unified source registry
    ///
    /// Rebuilds if necessary.
    #[must_use]
    pub fn source_registry(&mut self) -> &SourceRegistry {
        self.rebuild_registries().0
    }

    /// Get the unified sink registry
    ///
    /// Rebuilds if necessary.
    #[must_use]
    pub fn sink_registry(&mut self) -> &SinkRegistry {
        self.rebuild_registries().1
    }

    /// Get the unified sanitizer registry
    ///
    /// Rebuilds if necessary.
    #[must_use]
    pub fn sanitizer_registry(&mut self) -> &SanitizerRegistry {
        self.rebuild_registries().2
    }

    /// Check if an annotation indicates a taint source in any enabled framework
    ///
    /// # Arguments
    /// * `annotation_name` - The simple name of the annotation (without package)
    ///
    /// # Returns
    /// `true` if any enabled framework recognizes this as a source annotation
    #[must_use]
    pub fn is_source_annotation(&self, annotation_name: &str) -> bool {
        self.enabled.iter().any(|framework_id| {
            self.frameworks
                .get(framework_id)
                .is_some_and(|f| f.is_source_annotation(annotation_name))
        })
    }

    /// Get all frameworks (regardless of enabled status)
    #[must_use]
    pub fn all_frameworks(&self) -> Vec<&dyn Framework> {
        self.frameworks
            .values()
            .map(std::convert::AsRef::as_ref)
            .collect()
    }

    /// Build and consume registries, returning owned instances
    ///
    /// This is useful when you need owned registries rather than references.
    /// After calling this method, the registry caches are invalidated.
    ///
    /// # Returns
    /// Tuple of owned `(SourceRegistry, SinkRegistry, SanitizerRegistry)`
    pub fn into_registries(mut self) -> (SourceRegistry, SinkRegistry, SanitizerRegistry) {
        self.rebuild_registries();

        (
            self.source_registry
                .take()
                .expect("registries rebuilt above"),
            self.sink_registry.take().expect("registries rebuilt above"),
            self.sanitizer_registry
                .take()
                .expect("registries rebuilt above"),
        )
    }

    /// Automatically detect and enable frameworks in a project directory
    ///
    /// This scans the project for build files and source code to detect which frameworks
    /// are in use. Frameworks that meet the confidence threshold (0.5) are automatically enabled.
    ///
    /// # Arguments
    /// * `project_root` - Root directory of the project to scan
    ///
    /// # Returns
    /// Detection results for all registered frameworks, sorted by confidence
    ///
    /// # Example
    ///
    /// ```ignore
    /// use hyperion_security::frameworks::FrameworkRegistry;
    ///
    /// let mut registry = FrameworkRegistry::with_builtin();
    /// let results = registry.detect_frameworks("/path/to/project");
    ///
    /// for result in results {
    ///     println!("{}: {:.2}", result.framework_name, result.confidence.value());
    /// }
    /// ```
    pub fn detect_frameworks(
        &mut self,
        project_root: &std::path::Path,
    ) -> Vec<super::detection::DetectionResult> {
        use super::detection::FrameworkDetector;

        // Collect all registered frameworks
        let frameworks: Vec<&dyn Framework> = self.all_frameworks();

        // Run detection
        let detector = FrameworkDetector::new(project_root);
        let results = detector.detect(&frameworks);

        // Auto-enable frameworks that meet the threshold
        let frameworks_to_enable: Vec<String> = results
            .iter()
            .filter(|r| r.confidence.exceeds_threshold())
            .map(|r| r.framework_id.clone())
            .collect();

        if frameworks_to_enable.is_empty() {
            tracing::info!("No frameworks detected above confidence threshold");
        } else {
            tracing::info!(
                "Auto-enabling frameworks: {}",
                frameworks_to_enable.join(", ")
            );
            let framework_ids: Vec<&str> = frameworks_to_enable
                .iter()
                .map(std::string::String::as_str)
                .collect();
            self.enable_frameworks(&framework_ids);
        }

        results
    }

    /// Invalidate cached registries
    fn invalidate_caches(&mut self) {
        self.source_registry = None;
        self.sink_registry = None;
        self.sanitizer_registry = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::VulnType;
    use crate::sources::SourceKind;
    use crate::taint::VulnTypes;
    use std::collections::HashSet;

    // Mock framework for testing
    struct MockFramework {
        id: &'static str,
        name: &'static str,
    }

    impl Framework for MockFramework {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn register_sources(&self, registry: &mut SourceRegistry) {
            let mut methods = HashSet::new();
            methods.insert("testMethod".to_string());
            let mut receivers = HashSet::new();
            receivers.insert("TestReceiver".to_string());

            registry.register_pattern(receivers, methods, SourceKind::HttpRequest);
        }

        fn register_sinks(&self, registry: &mut SinkRegistry) {
            let mut methods = HashSet::new();
            methods.insert("testSink".to_string());
            let mut receivers = HashSet::new();
            receivers.insert("TestSink".to_string());

            registry.register_pattern(receivers, methods, VulnType::SqlInjection, vec![0], false);
        }

        fn register_sanitizers(&self, registry: &mut SanitizerRegistry) {
            let mut methods = HashSet::new();
            methods.insert("testSanitize".to_string());
            let mut receivers = HashSet::new();
            receivers.insert("TestSanitizer".to_string());

            registry.register_pattern(receivers, methods, VulnTypes::SQL_INJECTION);
        }

        fn is_source_annotation(&self, annotation_name: &str) -> bool {
            annotation_name == "TestParam"
        }
    }

    #[test]
    fn test_empty_registry() {
        let mut registry = FrameworkRegistry::new();
        assert_eq!(registry.enabled_frameworks().len(), 0);

        let (sources, sinks, sanitizers) = registry.rebuild_registries();

        // Empty registries should not match anything
        assert!(sources
            .is_source(Some("TestReceiver"), "testMethod")
            .is_none());
        assert!(sinks.is_sink(Some("TestSink"), "testSink").is_none());
        assert!(sanitizers
            .is_sanitizer(Some("TestSanitizer"), "testSanitize")
            .is_none());
    }

    #[test]
    fn test_register_and_enable() {
        let mut registry = FrameworkRegistry::new();

        registry.register(Box::new(MockFramework {
            id: "test",
            name: "Test Framework",
        }));

        assert!(registry.has_framework("test"));
        assert!(!registry.is_enabled("test"));

        // Enable the framework
        registry.enable_frameworks(&["test"]);
        assert!(registry.is_enabled("test"));

        // Rebuild registries
        let (sources, sinks, sanitizers) = registry.rebuild_registries();

        // Should now match our mock patterns
        assert!(sources
            .is_source(Some("TestReceiver"), "testMethod")
            .is_some());
        assert!(sinks.is_sink(Some("TestSink"), "testSink").is_some());
        assert!(sanitizers
            .is_sanitizer(Some("TestSanitizer"), "testSanitize")
            .is_some());
    }

    #[test]
    fn test_selective_enabling() {
        let mut registry = FrameworkRegistry::new();

        registry.register(Box::new(MockFramework {
            id: "framework1",
            name: "Framework 1",
        }));

        registry.register(Box::new(MockFramework {
            id: "framework2",
            name: "Framework 2",
        }));

        // Only enable framework1
        registry.enable_frameworks(&["framework1"]);

        assert!(registry.is_enabled("framework1"));
        assert!(!registry.is_enabled("framework2"));
        assert_eq!(registry.enabled_frameworks().len(), 1);

        // Enable both
        registry.enable_frameworks(&["framework1", "framework2"]);
        assert_eq!(registry.enabled_frameworks().len(), 2);
    }

    #[test]
    fn test_is_source_annotation() {
        let mut registry = FrameworkRegistry::new();

        registry.register(Box::new(MockFramework {
            id: "test",
            name: "Test Framework",
        }));

        registry.enable_frameworks(&["test"]);

        assert!(registry.is_source_annotation("TestParam"));
        assert!(!registry.is_source_annotation("OtherParam"));
    }

    #[test]
    fn test_enable_all() {
        let mut registry = FrameworkRegistry::new();

        registry.register(Box::new(MockFramework {
            id: "f1",
            name: "Framework 1",
        }));

        registry.register(Box::new(MockFramework {
            id: "f2",
            name: "Framework 2",
        }));

        registry.enable_all();

        assert_eq!(registry.enabled_frameworks().len(), 2);
        assert!(registry.is_enabled("f1"));
        assert!(registry.is_enabled("f2"));
    }

    #[test]
    fn test_cache_invalidation() {
        let mut registry = FrameworkRegistry::new();

        registry.register(Box::new(MockFramework {
            id: "test",
            name: "Test",
        }));

        registry.enable_frameworks(&["test"]);

        // Build registries
        let (sources, _, _) = registry.rebuild_registries();
        assert!(sources
            .is_source(Some("TestReceiver"), "testMethod")
            .is_some());

        // Disable framework
        registry.enable_frameworks(&[]);

        // Registries should be rebuilt
        let (sources, _, _) = registry.rebuild_registries();
        assert!(sources
            .is_source(Some("TestReceiver"), "testMethod")
            .is_none());
    }

    #[test]
    fn test_builtin_frameworks_registered() {
        let registry = FrameworkRegistry::with_builtin();

        // Verify all 7 frameworks are registered
        assert!(registry.has_framework("servlet"));
        assert!(registry.has_framework("spring"));
        assert!(registry.has_framework("jaxrs"));
        assert!(registry.has_framework("dropwizard"));
        assert!(registry.has_framework("micronaut"));
        assert!(registry.has_framework("quarkus"));
        assert!(registry.has_framework("vertx"));

        // Verify all 7 are enabled by default
        assert_eq!(registry.enabled_frameworks().len(), 7);
        assert!(registry.is_enabled("micronaut"));
        assert!(registry.is_enabled("quarkus"));
        assert!(registry.is_enabled("vertx"));
    }
}
