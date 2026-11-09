// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method summaries for common library methods
//!
//! This module provides pre-defined taint propagation behaviors for common
//! Java standard library methods. These summaries allow the scanner to track
//! taint through method calls without requiring full interprocedural analysis.
//!
//! # Example
//!
//! ```ignore
//! use hyperion_security::method_summaries::{MethodSummaryRegistry, TaintBehavior};
//!
//! let registry = MethodSummaryRegistry::new();
//!
//! // Check if String.concat propagates taint
//! if let Some(summary) = registry.get_summary("String", "concat") {
//!     match summary.taint_behavior {
//!         TaintBehavior::PropagatesReceiver => {
//!             // concat() returns tainted value if receiver is tainted
//!         }
//!         _ => {}
//!     }
//! }
//! ```

use std::collections::HashMap;

/// How a method propagates taint
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaintBehavior {
    /// Method returns taint from a specific argument index
    /// Example: String.valueOf(arg) - returns tainted if arg is tainted
    PropagatesArg { arg_index: usize },

    /// Method returns taint from receiver (this)
    /// Example: String.toLowerCase() - returns tainted if receiver is tainted
    PropagatesReceiver,

    /// Method combines multiple arguments into return value
    /// Example: String.format(fmt, args...) - returns tainted if any arg is tainted
    CombinesArgs { arg_indices: Vec<usize> },

    /// Method propagates taint from any input (receiver or any arg)
    /// Example: String.concat(str) - returns tainted if receiver OR arg is tainted
    PropagatesAll,

    /// Method always returns clean data (sanitizer)
    /// Example: Integer.parseInt(str) - returns clean int even if str is tainted
    AlwaysClean,

    /// Method always returns tainted data (deserialization sources)
    /// Example: ObjectInputStream.readObject() - returns attacker-controlled object
    /// This is used for deserialization methods where the input comes from
    /// an untrusted stream that was created earlier
    AlwaysTainted,

    // ========================================================================
    // Collection Taint Behaviors
    // ========================================================================
    /// Method taints the receiver if the argument at the given index is tainted
    /// Example: List.add(element) - if element is tainted, mark list as tainted
    /// Example: Set.add(element) - if element is tainted, mark set as tainted
    TaintsReceiver { arg_index: usize },

    /// Method taints the receiver if the value argument (at arg_index) is tainted
    /// Example: Map.put(key, value) - if value is tainted, mark map as tainted
    /// Note: We track value taint, not key taint, as values are more commonly used in sinks
    TaintsReceiverFromValue { value_arg_index: usize },
}

/// Summary of a method's taint behavior
#[derive(Debug, Clone)]
pub struct MethodSummary {
    /// Simple method name (e.g., "concat")
    pub method_name: String,

    /// Class name if method is instance method (e.g., "String")
    /// None for static methods or when class doesn't matter
    pub class_name: Option<String>,

    /// How this method propagates taint
    pub taint_behavior: TaintBehavior,
}

impl MethodSummary {
    /// Create a new method summary
    pub fn new(
        method_name: impl Into<String>,
        class_name: Option<impl Into<String>>,
        taint_behavior: TaintBehavior,
    ) -> Self {
        Self {
            method_name: method_name.into(),
            class_name: class_name.map(std::convert::Into::into),
            taint_behavior,
        }
    }

    /// Check if this summary applies to a given method call
    pub fn matches(&self, receiver_type: Option<&str>, method_name: &str) -> bool {
        // Method name must match
        if self.method_name != method_name {
            return false;
        }

        // If summary has a class constraint, receiver must match
        match (&self.class_name, receiver_type) {
            (Some(expected_class), Some(actual_class)) => {
                // Exact match or subtype match (simple string comparison for now)
                expected_class == actual_class
            }
            (Some(_), None) => false, // Summary requires a class but call has no receiver
            (None, _) => true,        // Summary doesn't care about class
        }
    }
}

/// Registry of method summaries for common library methods
pub struct MethodSummaryRegistry {
    /// Map from method name to list of summaries
    /// Multiple summaries can exist for same method name (different classes)
    summaries: HashMap<String, Vec<MethodSummary>>,
}

impl MethodSummaryRegistry {
    /// Create a new registry with default summaries for common library methods
    pub fn new() -> Self {
        let mut registry = Self {
            summaries: HashMap::new(),
        };

        registry.register_default_summaries();
        registry
    }

    /// Register a method summary
    pub fn register(&mut self, summary: MethodSummary) {
        self.summaries
            .entry(summary.method_name.clone())
            .or_default()
            .push(summary);
    }

    /// Get matching summary for a method call
    pub fn get_summary(
        &self,
        receiver_type: Option<&str>,
        method_name: &str,
    ) -> Option<&MethodSummary> {
        self.summaries
            .get(method_name)?
            .iter()
            .find(|summary| summary.matches(receiver_type, method_name))
    }

    /// Register default summaries for common Java library methods
    fn register_default_summaries(&mut self) {
        // String methods - most propagate taint from receiver
        self.register(MethodSummary::new(
            "concat",
            Some("String"),
            TaintBehavior::PropagatesAll, // Combines receiver + argument
        ));

        self.register(MethodSummary::new(
            "replace",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "replaceAll",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "replaceFirst",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "substring",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "trim",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "toLowerCase",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "toUpperCase",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "strip",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "stripLeading",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "stripTrailing",
            Some("String"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Base64 and similar decoders - propagate taint from argument
        self.register(MethodSummary::new(
            "decode",
            None::<String>,
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        // String.format - combines all arguments
        self.register(MethodSummary::new(
            "format",
            Some("String"),
            TaintBehavior::PropagatesAll,
        ));

        // String.join - combines all arguments
        self.register(MethodSummary::new(
            "join",
            Some("String"),
            TaintBehavior::PropagatesAll,
        ));

        // String.valueOf - propagates argument
        self.register(MethodSummary::new(
            "valueOf",
            Some("String"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        // StringBuilder methods
        self.register(MethodSummary::new(
            "toString",
            Some("StringBuilder"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "substring",
            Some("StringBuilder"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "append",
            Some("StringBuilder"),
            TaintBehavior::PropagatesAll, // Receiver becomes tainted if arg is tainted
        ));

        // StringBuffer methods (same as StringBuilder)
        self.register(MethodSummary::new(
            "toString",
            Some("StringBuffer"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "substring",
            Some("StringBuffer"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "append",
            Some("StringBuffer"),
            TaintBehavior::PropagatesAll,
        ));

        // ========================================================================
        // Collections Methods
        // ========================================================================

        // List retrieval - propagates taint from collection to returned element
        self.register(MethodSummary::new(
            "get",
            Some("List"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "get",
            Some("ArrayList"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "get",
            Some("LinkedList"),
            TaintBehavior::PropagatesReceiver,
        ));

        // List mutation - taints the collection if tainted element is added
        self.register(MethodSummary::new(
            "add",
            Some("List"),
            TaintBehavior::TaintsReceiver { arg_index: 0 },
        ));
        self.register(MethodSummary::new(
            "add",
            Some("ArrayList"),
            TaintBehavior::TaintsReceiver { arg_index: 0 },
        ));
        self.register(MethodSummary::new(
            "add",
            Some("LinkedList"),
            TaintBehavior::TaintsReceiver { arg_index: 0 },
        ));
        // add(int index, E element) - taint from element at index 1
        self.register(MethodSummary::new(
            "set",
            Some("List"),
            TaintBehavior::TaintsReceiver { arg_index: 1 },
        ));
        self.register(MethodSummary::new(
            "set",
            Some("ArrayList"),
            TaintBehavior::TaintsReceiver { arg_index: 1 },
        ));
        self.register(MethodSummary::new(
            "set",
            Some("LinkedList"),
            TaintBehavior::TaintsReceiver { arg_index: 1 },
        ));

        // Set mutation - taints the collection if tainted element is added
        self.register(MethodSummary::new(
            "add",
            Some("Set"),
            TaintBehavior::TaintsReceiver { arg_index: 0 },
        ));
        self.register(MethodSummary::new(
            "add",
            Some("HashSet"),
            TaintBehavior::TaintsReceiver { arg_index: 0 },
        ));
        self.register(MethodSummary::new(
            "add",
            Some("TreeSet"),
            TaintBehavior::TaintsReceiver { arg_index: 0 },
        ));
        self.register(MethodSummary::new(
            "add",
            Some("LinkedHashSet"),
            TaintBehavior::TaintsReceiver { arg_index: 0 },
        ));

        // Map retrieval - propagates taint from collection to returned value
        self.register(MethodSummary::new(
            "get",
            Some("Map"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "get",
            Some("HashMap"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "get",
            Some("TreeMap"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "get",
            Some("LinkedHashMap"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "get",
            Some("ConcurrentHashMap"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Map mutation - taints the collection if tainted value is put
        // put(K key, V value) - track value taint at arg index 1
        self.register(MethodSummary::new(
            "put",
            Some("Map"),
            TaintBehavior::TaintsReceiverFromValue { value_arg_index: 1 },
        ));
        self.register(MethodSummary::new(
            "put",
            Some("HashMap"),
            TaintBehavior::TaintsReceiverFromValue { value_arg_index: 1 },
        ));
        self.register(MethodSummary::new(
            "put",
            Some("TreeMap"),
            TaintBehavior::TaintsReceiverFromValue { value_arg_index: 1 },
        ));
        self.register(MethodSummary::new(
            "put",
            Some("LinkedHashMap"),
            TaintBehavior::TaintsReceiverFromValue { value_arg_index: 1 },
        ));
        self.register(MethodSummary::new(
            "put",
            Some("ConcurrentHashMap"),
            TaintBehavior::TaintsReceiverFromValue { value_arg_index: 1 },
        ));

        // Map getOrDefault - propagates from receiver OR default value
        self.register(MethodSummary::new(
            "getOrDefault",
            Some("Map"),
            TaintBehavior::PropagatesAll,
        ));
        self.register(MethodSummary::new(
            "getOrDefault",
            Some("HashMap"),
            TaintBehavior::PropagatesAll,
        ));

        // Collection iteration methods - propagate taint
        self.register(MethodSummary::new(
            "iterator",
            Some("Collection"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "iterator",
            Some("List"),
            TaintBehavior::PropagatesReceiver,
        ));
        self.register(MethodSummary::new(
            "iterator",
            Some("Set"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Iterator.next() - propagates from iterator (which propagates from collection)
        self.register(MethodSummary::new(
            "next",
            Some("Iterator"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Collection toString - propagates taint
        self.register(MethodSummary::new(
            "toString",
            Some("Collection"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "toString",
            Some("List"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "toString",
            Some("Set"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "toString",
            Some("Map"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Optional methods
        self.register(MethodSummary::new(
            "get",
            Some("Optional"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "orElse",
            Some("Optional"),
            TaintBehavior::PropagatesAll, // Propagates from receiver OR argument
        ));

        self.register(MethodSummary::new(
            "orElseGet",
            Some("Optional"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Object.toString - propagates from receiver
        self.register(MethodSummary::new(
            "toString",
            Some("Object"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Arrays.toString - propagates from array argument
        self.register(MethodSummary::new(
            "toString",
            Some("Arrays"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        // === JSON Parsing Libraries ===
        // Jackson ObjectMapper methods - propagate taint from input to parsed object
        self.register(MethodSummary::new(
            "readValue",
            Some("ObjectMapper"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        self.register(MethodSummary::new(
            "readTree",
            Some("ObjectMapper"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        self.register(MethodSummary::new(
            "convertValue",
            Some("ObjectMapper"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        // Jackson JsonParser streaming methods - propagate taint from parser state (receiver)
        self.register(MethodSummary::new(
            "readValueAs",
            Some("JsonParser"),
            TaintBehavior::PropagatesReceiver,
        ));

        self.register(MethodSummary::new(
            "readValueAsTree",
            Some("JsonParser"),
            TaintBehavior::PropagatesReceiver,
        ));

        // Gson methods - propagate taint from input to parsed object
        self.register(MethodSummary::new(
            "fromJson",
            Some("Gson"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        // === Deserialization Sources ===
        // ObjectInputStream methods - always return attacker-controlled data
        self.register(MethodSummary::new(
            "readObject",
            Some("ObjectInputStream"),
            TaintBehavior::AlwaysTainted,
        ));

        self.register(MethodSummary::new(
            "readUnshared",
            Some("ObjectInputStream"),
            TaintBehavior::AlwaysTainted,
        ));

        // XStream methods - propagate taint from input argument
        self.register(MethodSummary::new(
            "fromXML",
            Some("XStream"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));

        // Kryo methods - always return attacker-controlled data
        self.register(MethodSummary::new(
            "readObject",
            Some("Kryo"),
            TaintBehavior::AlwaysTainted,
        ));

        self.register(MethodSummary::new(
            "readClassAndObject",
            Some("Kryo"),
            TaintBehavior::AlwaysTainted,
        ));

        // Hessian methods
        self.register(MethodSummary::new(
            "readObject",
            Some("HessianInput"),
            TaintBehavior::AlwaysTainted,
        ));

        self.register(MethodSummary::new(
            "readObject",
            Some("Hessian2Input"),
            TaintBehavior::AlwaysTainted,
        ));

        // Integer/Long/etc parsing methods - these are sanitizers (always clean)
        for class_name in &["Integer", "Long", "Short", "Byte", "Float", "Double"] {
            self.register(MethodSummary::new(
                "parseInt",
                Some(*class_name),
                TaintBehavior::AlwaysClean,
            ));
            self.register(MethodSummary::new(
                "parseLong",
                Some(*class_name),
                TaintBehavior::AlwaysClean,
            ));
            self.register(MethodSummary::new(
                "parseShort",
                Some(*class_name),
                TaintBehavior::AlwaysClean,
            ));
            self.register(MethodSummary::new(
                "parseByte",
                Some(*class_name),
                TaintBehavior::AlwaysClean,
            ));
            self.register(MethodSummary::new(
                "parseFloat",
                Some(*class_name),
                TaintBehavior::AlwaysClean,
            ));
            self.register(MethodSummary::new(
                "parseDouble",
                Some(*class_name),
                TaintBehavior::AlwaysClean,
            ));
        }

        // === URL/URI methods for SSRF tracking ===
        // These methods propagate taint from their receiver (the URL/URI object)
        // If a URL is constructed from tainted data, these methods propagate that taint

        // URL.openStream() - opens a connection and returns an InputStream
        self.register(MethodSummary::new(
            "openStream",
            Some("URL"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URL.openConnection() - opens a connection and returns a URLConnection
        self.register(MethodSummary::new(
            "openConnection",
            Some("URL"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URL.getContent() - returns the content of the URL
        self.register(MethodSummary::new(
            "getContent",
            Some("URL"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URL.toString() - propagates taint through string representation
        self.register(MethodSummary::new(
            "toString",
            Some("URL"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URL.toExternalForm() - returns external form string
        self.register(MethodSummary::new(
            "toExternalForm",
            Some("URL"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URI.toURL() - converts URI to URL, propagating taint
        self.register(MethodSummary::new(
            "toURL",
            Some("URI"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URI.toString() - propagates taint through string representation
        self.register(MethodSummary::new(
            "toString",
            Some("URI"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URI.toASCIIString() - returns ASCII string representation
        self.register(MethodSummary::new(
            "toASCIIString",
            Some("URI"),
            TaintBehavior::PropagatesReceiver,
        ));

        // URI.create() - static method that creates URI from string
        // The taint comes from the argument, not the receiver
        self.register(MethodSummary::new(
            "create",
            Some("URI"),
            TaintBehavior::PropagatesArg { arg_index: 0 },
        ));
    }
}

impl Default for MethodSummaryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = MethodSummaryRegistry::new();
        assert!(!registry.summaries.is_empty());
    }

    #[test]
    fn test_string_concat_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("String"), "concat");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesAll
        );
    }

    #[test]
    fn test_string_tolowercase_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("String"), "toLowerCase");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesReceiver
        );
    }

    #[test]
    fn test_stringbuilder_tostring_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("StringBuilder"), "toString");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesReceiver
        );
    }

    #[test]
    fn test_optional_get_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("Optional"), "get");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesReceiver
        );
    }

    #[test]
    fn test_integer_parseint_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("Integer"), "parseInt");
        assert!(summary.is_some());
        assert_eq!(summary.unwrap().taint_behavior, TaintBehavior::AlwaysClean);
    }

    #[test]
    fn test_method_not_found() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("String"), "nonExistentMethod");
        assert!(summary.is_none());
    }

    #[test]
    fn test_wrong_class() {
        let registry = MethodSummaryRegistry::new();
        // concat exists for String but not for Integer
        let summary = registry.get_summary(Some("Integer"), "concat");
        assert!(summary.is_none());
    }

    #[test]
    fn test_matches_method() {
        let summary =
            MethodSummary::new("concat", Some("String"), TaintBehavior::PropagatesReceiver);

        // Matches correct class and method
        assert!(summary.matches(Some("String"), "concat"));

        // Doesn't match wrong method
        assert!(!summary.matches(Some("String"), "toLowerCase"));

        // Doesn't match wrong class
        assert!(!summary.matches(Some("Integer"), "concat"));

        // Doesn't match when class is required but not provided
        assert!(!summary.matches(None, "concat"));
    }

    #[test]
    fn test_matches_no_class_constraint() {
        let summary = MethodSummary::new(
            "toString",
            None::<String>,
            TaintBehavior::PropagatesReceiver,
        );

        // Matches any class when no constraint
        assert!(summary.matches(Some("String"), "toString"));
        assert!(summary.matches(Some("Integer"), "toString"));
        assert!(summary.matches(None, "toString"));

        // Still must match method name
        assert!(!summary.matches(Some("String"), "concat"));
    }

    #[test]
    fn test_custom_summary_registration() {
        let mut registry = MethodSummaryRegistry::new();

        // Register custom summary
        registry.register(MethodSummary::new(
            "customMethod",
            Some("CustomClass"),
            TaintBehavior::PropagatesArg { arg_index: 1 },
        ));

        let summary = registry.get_summary(Some("CustomClass"), "customMethod");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesArg { arg_index: 1 }
        );
    }

    // === JSON Parsing Library Tests ===

    #[test]
    fn test_objectmapper_readvalue_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("ObjectMapper"), "readValue");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesArg { arg_index: 0 }
        );
    }

    #[test]
    fn test_objectmapper_readtree_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("ObjectMapper"), "readTree");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesArg { arg_index: 0 }
        );
    }

    #[test]
    fn test_objectmapper_convertvalue_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("ObjectMapper"), "convertValue");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesArg { arg_index: 0 }
        );
    }

    #[test]
    fn test_jsonparser_readvalueas_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("JsonParser"), "readValueAs");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesReceiver
        );
    }

    #[test]
    fn test_jsonparser_readvalueastree_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("JsonParser"), "readValueAsTree");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesReceiver
        );
    }

    #[test]
    fn test_gson_fromjson_summary() {
        let registry = MethodSummaryRegistry::new();
        let summary = registry.get_summary(Some("Gson"), "fromJson");
        assert!(summary.is_some());
        assert_eq!(
            summary.unwrap().taint_behavior,
            TaintBehavior::PropagatesArg { arg_index: 0 }
        );
    }
}
