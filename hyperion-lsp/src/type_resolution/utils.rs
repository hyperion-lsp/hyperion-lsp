// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Utility functions for type resolution

/// Extract package from fully qualified name
///
/// # Examples
/// ```
/// use hyperion_lsp::type_resolution::extract_package;
///
/// assert_eq!(
///     extract_package("java.util.List"),
///     Some("java.util".to_string())
/// );
/// assert_eq!(extract_package("String"), None);
/// ```
pub fn extract_package(fqn: &str) -> Option<String> {
    fqn.rfind('.').map(|idx| fqn[..idx].to_string())
}

/// Extract simple name from fully qualified name
///
/// Handles method FQNs with parameter descriptors like:
/// `com.example.Class.methodName(java.lang.String)` -> `methodName`
///
/// # Examples
/// ```
/// use hyperion_lsp::type_resolution::extract_simple_name;
///
/// assert_eq!(extract_simple_name("java.util.List"), "List");
/// assert_eq!(extract_simple_name("String"), "String");
/// assert_eq!(extract_simple_name("com.example.Class.method(java.lang.String)"), "method");
/// ```
pub fn extract_simple_name(fqn: &str) -> String {
    // Strip off parameter descriptor if present (for method FQNs)
    let fqn_without_params = fqn.split('(').next().unwrap_or(fqn);
    fqn_without_params
        .rsplit('.')
        .next()
        .unwrap_or(fqn_without_params)
        .to_string()
}

/// Check if a name looks like a fully qualified name
///
/// # Examples
/// ```
/// use hyperion_lsp::type_resolution::is_fqn;
///
/// assert!(is_fqn("java.util.List"));
/// assert!(!is_fqn("List"));
/// ```
pub fn is_fqn(name: &str) -> bool {
    name.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package() {
        assert_eq!(
            extract_package("java.util.List"),
            Some("java.util".to_string())
        );
        assert_eq!(
            extract_package("com.example.User"),
            Some("com.example".to_string())
        );
        assert_eq!(extract_package("String"), None);
    }

    #[test]
    fn test_extract_simple_name() {
        assert_eq!(extract_simple_name("java.util.List"), "List");
        assert_eq!(extract_simple_name("com.example.User"), "User");
        assert_eq!(extract_simple_name("String"), "String");
    }

    #[test]
    fn test_is_fqn() {
        assert!(is_fqn("java.util.List"));
        assert!(is_fqn("com.example.User"));
        assert!(!is_fqn("List"));
        assert!(!is_fqn("String"));
    }
}
