// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Package-level nullability defaults from package-info.java files

use super::Nullability;
use std::collections::HashMap;

/// What elements are non-null by default in a package
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PackageNullabilityDefaults {
    /// Parameters are non-null by default (@ParametersAreNonnullByDefault, @NonNullApi)
    pub parameters_nonnull: bool,
    /// Return values are non-null by default (@NonNullApi)
    pub returns_nonnull: bool,
    /// Fields are non-null by default (@NonNullFields)
    pub fields_nonnull: bool,
}

impl PackageNullabilityDefaults {
    /// No defaults (everything is platform type / Unknown)
    pub fn none() -> Self {
        Self::default()
    }

    /// Spring @NonNullApi - parameters and returns are non-null
    pub const fn nonnull_api() -> Self {
        Self {
            parameters_nonnull: true,
            returns_nonnull: true,
            fields_nonnull: false,
        }
    }

    /// Spring @NonNullFields - fields are non-null
    pub const fn nonnull_fields() -> Self {
        Self {
            parameters_nonnull: false,
            returns_nonnull: false,
            fields_nonnull: true,
        }
    }

    /// Full @NonNullApi + @NonNullFields
    pub const fn full() -> Self {
        Self {
            parameters_nonnull: true,
            returns_nonnull: true,
            fields_nonnull: true,
        }
    }

    /// Get default nullability for a parameter
    pub const fn parameter_default(&self) -> Nullability {
        if self.parameters_nonnull {
            Nullability::NonNull
        } else {
            Nullability::Unknown
        }
    }

    /// Get default nullability for a return type
    pub const fn return_default(&self) -> Nullability {
        if self.returns_nonnull {
            Nullability::NonNull
        } else {
            Nullability::Unknown
        }
    }

    /// Get default nullability for a field
    pub const fn field_default(&self) -> Nullability {
        if self.fields_nonnull {
            Nullability::NonNull
        } else {
            Nullability::Unknown
        }
    }

    /// Combine with another set of defaults (OR semantics)
    pub const fn merge(&self, other: &Self) -> Self {
        Self {
            parameters_nonnull: self.parameters_nonnull || other.parameters_nonnull,
            returns_nonnull: self.returns_nonnull || other.returns_nonnull,
            fields_nonnull: self.fields_nonnull || other.fields_nonnull,
        }
    }
}

/// Package-level annotation names that affect nullability defaults
pub const NONNULL_API_ANNOTATIONS: &[&str] = &[
    "NonNullApi",
    "org.springframework.lang.NonNullApi",
    "org.eclipse.jdt.annotation.NonNullByDefault",
];

pub const NONNULL_FIELDS_ANNOTATIONS: &[&str] =
    &["NonNullFields", "org.springframework.lang.NonNullFields"];

pub const PARAMETERS_NONNULL_ANNOTATIONS: &[&str] = &[
    "ParametersAreNonnullByDefault",
    "javax.annotation.ParametersAreNonnullByDefault",
    "org.checkerframework.checker.nullness.qual.NonNullByDefault",
];

/// Extract package defaults from a package-info.java source file
pub fn extract_package_defaults(source: &str) -> PackageNullabilityDefaults {
    let mut defaults = PackageNullabilityDefaults::none();

    // Simple text-based detection (works before full parsing)
    for line in source.lines() {
        let line = line.trim();

        // Check for @NonNullApi style annotations
        for ann in NONNULL_API_ANNOTATIONS {
            if line.contains(&format!("@{ann}"))
                || line.contains(&format!("@{}", ann.split('.').next_back().unwrap_or(ann)))
            {
                defaults.parameters_nonnull = true;
                defaults.returns_nonnull = true;
            }
        }

        // Check for @NonNullFields
        for ann in NONNULL_FIELDS_ANNOTATIONS {
            if line.contains(&format!("@{ann}"))
                || line.contains(&format!("@{}", ann.split('.').next_back().unwrap_or(ann)))
            {
                defaults.fields_nonnull = true;
            }
        }

        // Check for @ParametersAreNonnullByDefault
        for ann in PARAMETERS_NONNULL_ANNOTATIONS {
            if line.contains(&format!("@{ann}"))
                || line.contains(&format!("@{}", ann.split('.').next_back().unwrap_or(ann)))
            {
                defaults.parameters_nonnull = true;
            }
        }
    }

    defaults
}

/// Extract package name from package-info.java source
pub fn extract_package_name(source: &str) -> Option<String> {
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with("package ") {
            // Extract package name, strip trailing semicolon
            let name = line.strip_prefix("package ")?.trim();
            let name = name.strip_suffix(';').unwrap_or(name).trim();
            return Some(name.to_string());
        }
    }
    None
}

/// Registry of package defaults for a workspace
#[derive(Debug, Clone, Default)]
pub struct PackageDefaultsRegistry {
    /// Map from package name to defaults
    packages: HashMap<String, PackageNullabilityDefaults>,
}

impl PackageDefaultsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register defaults for a package from its package-info.java content
    pub fn register_package_info(&mut self, source: &str) {
        if let Some(package_name) = extract_package_name(source) {
            let defaults = extract_package_defaults(source);
            if defaults != PackageNullabilityDefaults::none() {
                tracing::debug!(
                    "Registered nullability defaults for package {}: {:?}",
                    package_name,
                    defaults
                );
                self.packages.insert(package_name, defaults);
            }
        }
    }

    /// Get defaults for a package (checks parent packages too)
    pub fn get_defaults(&self, package_name: &str) -> PackageNullabilityDefaults {
        // Check exact match first
        if let Some(defaults) = self.packages.get(package_name) {
            return *defaults;
        }

        // Check parent packages (com.example.foo inherits from com.example)
        let mut current = package_name;
        while let Some(dot_pos) = current.rfind('.') {
            current = &current[..dot_pos];
            if let Some(defaults) = self.packages.get(current) {
                return *defaults;
            }
        }

        PackageNullabilityDefaults::none()
    }

    /// Clear all registered defaults
    pub fn clear(&mut self) {
        self.packages.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_defaults_spring() {
        let source = r#"
@NonNullApi
@NonNullFields
package com.example;

import org.springframework.lang.NonNullApi;
import org.springframework.lang.NonNullFields;
"#;

        let defaults = extract_package_defaults(source);
        assert!(defaults.parameters_nonnull);
        assert!(defaults.returns_nonnull);
        assert!(defaults.fields_nonnull);
    }

    #[test]
    fn test_extract_package_defaults_jsr305() {
        let source = r#"
@ParametersAreNonnullByDefault
package com.example;

import javax.annotation.ParametersAreNonnullByDefault;
"#;

        let defaults = extract_package_defaults(source);
        assert!(defaults.parameters_nonnull);
        assert!(!defaults.returns_nonnull);
        assert!(!defaults.fields_nonnull);
    }

    #[test]
    fn test_extract_package_name() {
        let source = "package com.example.mypackage;";
        assert_eq!(
            extract_package_name(source),
            Some("com.example.mypackage".to_string())
        );
    }

    #[test]
    fn test_registry_inheritance() {
        let mut registry = PackageDefaultsRegistry::new();
        registry.register_package_info(
            r#"
@NonNullApi
package com.example;
"#,
        );

        // Child packages inherit
        let defaults = registry.get_defaults("com.example.subpackage");
        assert!(defaults.parameters_nonnull);
        assert!(defaults.returns_nonnull);
    }

    #[test]
    fn test_registry_no_inheritance_across_trees() {
        let mut registry = PackageDefaultsRegistry::new();
        registry.register_package_info(
            r#"
@NonNullApi
package com.example;
"#,
        );

        // Unrelated package doesn't inherit
        let defaults = registry.get_defaults("org.other");
        assert!(!defaults.parameters_nonnull);
    }

    #[test]
    fn test_merge_defaults() {
        let api = PackageNullabilityDefaults::nonnull_api();
        let fields = PackageNullabilityDefaults::nonnull_fields();
        let merged = api.merge(&fields);

        assert_eq!(merged, PackageNullabilityDefaults::full());
    }

    #[test]
    fn test_default_constructors() {
        let none = PackageNullabilityDefaults::none();
        assert!(!none.parameters_nonnull);
        assert!(!none.returns_nonnull);
        assert!(!none.fields_nonnull);

        let api = PackageNullabilityDefaults::nonnull_api();
        assert!(api.parameters_nonnull);
        assert!(api.returns_nonnull);
        assert!(!api.fields_nonnull);

        let fields = PackageNullabilityDefaults::nonnull_fields();
        assert!(!fields.parameters_nonnull);
        assert!(!fields.returns_nonnull);
        assert!(fields.fields_nonnull);
    }

    #[test]
    fn test_default_methods() {
        let full = PackageNullabilityDefaults::full();
        assert_eq!(full.parameter_default(), Nullability::NonNull);
        assert_eq!(full.return_default(), Nullability::NonNull);
        assert_eq!(full.field_default(), Nullability::NonNull);

        let none = PackageNullabilityDefaults::none();
        assert_eq!(none.parameter_default(), Nullability::Unknown);
        assert_eq!(none.return_default(), Nullability::Unknown);
        assert_eq!(none.field_default(), Nullability::Unknown);
    }

    #[test]
    fn test_extract_package_defaults_qualified_annotations() {
        let source = r#"
@org.springframework.lang.NonNullApi
package com.example;

import org.springframework.lang.NonNullApi;
"#;

        let defaults = extract_package_defaults(source);
        assert!(defaults.parameters_nonnull);
        assert!(defaults.returns_nonnull);
    }

    #[test]
    fn test_extract_package_name_multiline() {
        let source = r#"
// Copyright header
package com.example.deeply.nested.package;

import java.util.*;
"#;
        assert_eq!(
            extract_package_name(source),
            Some("com.example.deeply.nested.package".to_string())
        );
    }

    #[test]
    fn test_registry_multiple_packages() {
        let mut registry = PackageDefaultsRegistry::new();

        registry.register_package_info(
            r#"
@NonNullApi
package com.example;
"#,
        );

        registry.register_package_info(
            r#"
@NonNullFields
package org.other;
"#,
        );

        let example_defaults = registry.get_defaults("com.example");
        assert!(example_defaults.parameters_nonnull);
        assert!(!example_defaults.fields_nonnull);

        let other_defaults = registry.get_defaults("org.other");
        assert!(!other_defaults.parameters_nonnull);
        assert!(other_defaults.fields_nonnull);
    }

    #[test]
    fn test_registry_clear() {
        let mut registry = PackageDefaultsRegistry::new();
        registry.register_package_info(
            r#"
@NonNullApi
package com.example;
"#,
        );

        assert!(registry.get_defaults("com.example").parameters_nonnull);

        registry.clear();
        assert!(!registry.get_defaults("com.example").parameters_nonnull);
    }
}
