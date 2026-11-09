// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Extension Function Registry
//!
//! Provides utilities for resolving Kotlin extension functions based on:
//! - Receiver type matching
//! - Scope (imports, same package, local definitions)
//!
//! Extension functions in Kotlin must be imported or defined locally to be callable:
//! ```kotlin
//! // Extension function definition
//! fun String.hello() = "Hello, $this!"
//!
//! // Can only call if in scope
//! val greeting = "World".hello()  // Works if hello() is imported/local
//! ```

use crate::ImportMap;

/// Result of extension function lookup
#[derive(Debug, Clone)]
pub struct ExtensionMatch {
    /// Fully qualified name of the extension function
    pub fqn: String,
    /// Simple name of the extension function
    pub name: String,
    /// Receiver type (e.g., "String" for `fun String.hello()`)
    pub receiver_type: String,
    /// Return type of the extension function
    pub return_type: Option<String>,
    /// Container package/class
    pub container: Option<String>,
}

/// Check if an extension function is in scope for the current file
///
/// An extension function is in scope if:
/// 1. It's defined in the same package as the current file
/// 2. It's explicitly imported (e.g., `import com.example.hello`)
/// 3. Its package is wildcard imported (e.g., `import com.example.*`)
/// 4. It's defined locally in the same file (handled by local scope resolution)
///
/// # Arguments
/// * `extension_fqn` - Fully qualified name of the extension function
/// * `imports` - Import map for the current file
///
/// # Returns
/// `true` if the extension function is in scope
pub fn is_extension_in_scope(extension_fqn: &str, imports: &ImportMap) -> bool {
    // Check explicit imports
    for imported_fqn in imports.explicit.values() {
        if imported_fqn == extension_fqn {
            return true;
        }
    }

    // Check wildcard imports
    // Extension FQN format: "package.functionName"
    if let Some(last_dot) = extension_fqn.rfind('.') {
        let package = &extension_fqn[..last_dot];

        // Same package as current file
        if let Some(ref current_package) = imports.current_package {
            if current_package == package {
                return true;
            }
        }

        // Wildcard import covers this package
        if imports.wildcards.contains(&package.to_string()) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_imports(
        package: Option<&str>,
        explicit: Vec<(&str, &str)>,
        wildcards: Vec<&str>,
    ) -> ImportMap {
        let mut import_map = ImportMap::new();
        import_map.current_package = package.map(String::from);
        import_map.explicit = explicit
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        import_map.wildcards = wildcards.into_iter().map(String::from).collect();
        import_map
    }

    #[test]
    fn test_extension_in_same_package() {
        let imports = make_imports(Some("com.example"), vec![], vec![]);

        assert!(is_extension_in_scope("com.example.hello", &imports));
        assert!(!is_extension_in_scope("com.other.hello", &imports));
    }

    #[test]
    fn test_extension_explicit_import() {
        let imports = make_imports(
            Some("com.example"),
            vec![("hello", "com.utils.hello")],
            vec![],
        );

        assert!(is_extension_in_scope("com.utils.hello", &imports));
        assert!(!is_extension_in_scope("com.utils.goodbye", &imports));
    }

    #[test]
    fn test_extension_wildcard_import() {
        let imports = make_imports(Some("com.example"), vec![], vec!["com.utils"]);

        assert!(is_extension_in_scope("com.utils.hello", &imports));
        assert!(is_extension_in_scope("com.utils.goodbye", &imports));
        assert!(!is_extension_in_scope("com.other.hello", &imports));
    }

    #[test]
    fn test_extension_not_in_scope() {
        let imports = make_imports(Some("com.example"), vec![], vec![]);

        assert!(!is_extension_in_scope("com.other.hello", &imports));
    }
}
