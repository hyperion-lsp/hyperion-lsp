// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Annotation recognition for nullability analysis

use hyperion_parser::syntax::SyntaxNode;

/// Extract a whitespace-normalized annotation name without `@` or arguments.
pub(crate) fn annotation_name(node: &SyntaxNode) -> Option<String> {
    let text = node.text().to_string();
    let text = text.trim();
    let name = text.strip_prefix('@').unwrap_or(text);
    let name = name.split('(').next().unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.replace(char::is_whitespace, ""))
}

/// Non-null annotation names (simple and fully qualified)
pub const NONNULL_ANNOTATIONS: &[&str] = &[
    // Simple names
    "NonNull",
    "NotNull",
    "Nonnull",
    // JSR-305
    "javax.annotation.Nonnull",
    // JetBrains
    "org.jetbrains.annotations.NotNull",
    // Android
    "androidx.annotation.NonNull",
    "android.support.annotation.NonNull",
    // Eclipse
    "org.eclipse.jdt.annotation.NonNull",
    // Spring
    "org.springframework.lang.NonNull",
    // Lombok
    "lombok.NonNull",
    // Jakarta (Java EE successor)
    "jakarta.annotation.Nonnull",
    // FindBugs/SpotBugs
    "edu.umd.cs.findbugs.annotations.NonNull",
    // Checker Framework
    "org.checkerframework.checker.nullness.qual.NonNull",
];

/// Nullable annotation names (simple and fully qualified)
pub const NULLABLE_ANNOTATIONS: &[&str] = &[
    // Simple names
    "Nullable",
    // JSR-305
    "javax.annotation.Nullable",
    // JetBrains
    "org.jetbrains.annotations.Nullable",
    // Android
    "androidx.annotation.Nullable",
    "android.support.annotation.Nullable",
    // Eclipse
    "org.eclipse.jdt.annotation.Nullable",
    // Spring
    "org.springframework.lang.Nullable",
    // Jakarta
    "jakarta.annotation.Nullable",
    // FindBugs/SpotBugs
    "edu.umd.cs.findbugs.annotations.Nullable",
    // Checker Framework
    "org.checkerframework.checker.nullness.qual.Nullable",
];

/// Package-level non-null annotations
pub const PACKAGE_NONNULL_ANNOTATIONS: &[&str] = &[
    // Spring
    "NonNullApi",
    "NonNullFields",
    "org.springframework.lang.NonNullApi",
    "org.springframework.lang.NonNullFields",
    // JSR-305 (applied to packages via package-info.java)
    "ParametersAreNonnullByDefault",
    "javax.annotation.ParametersAreNonnullByDefault",
    // Eclipse
    "org.eclipse.jdt.annotation.NonNullByDefault",
    // Checker Framework
    "org.checkerframework.checker.nullness.qual.NonNullByDefault",
];

/// Check if any of the annotations indicate non-null
pub fn has_nonnull_annotation(annotations: &[String]) -> bool {
    annotations.iter().any(|ann| {
        NONNULL_ANNOTATIONS.iter().any(|pattern| {
            // Match simple name or full qualified name
            ann == *pattern
                || ann.ends_with(&format!(
                    ".{}",
                    pattern.split('.').next_back().unwrap_or(pattern)
                ))
        })
    })
}

/// Check if any of the annotations indicate nullable
pub fn has_nullable_annotation(annotations: &[String]) -> bool {
    annotations.iter().any(|ann| {
        NULLABLE_ANNOTATIONS.iter().any(|pattern| {
            ann == *pattern
                || ann.ends_with(&format!(
                    ".{}",
                    pattern.split('.').next_back().unwrap_or(pattern)
                ))
        })
    })
}

/// Check if annotation is a package-level non-null default
pub fn is_package_nonnull_annotation(annotation: &str) -> bool {
    PACKAGE_NONNULL_ANNOTATIONS.iter().any(|pattern| {
        annotation == *pattern
            || annotation.ends_with(&format!(
                ".{}",
                pattern.split('.').next_back().unwrap_or(pattern)
            ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_nonnull_annotation_simple() {
        assert!(has_nonnull_annotation(&["NonNull".to_string()]));
        assert!(has_nonnull_annotation(&["NotNull".to_string()]));
        assert!(has_nonnull_annotation(&["Nonnull".to_string()]));
    }

    #[test]
    fn test_has_nonnull_annotation_qualified() {
        assert!(has_nonnull_annotation(&[
            "javax.annotation.Nonnull".to_string()
        ]));
        assert!(has_nonnull_annotation(&[
            "org.jetbrains.annotations.NotNull".to_string()
        ]));
        assert!(has_nonnull_annotation(&["lombok.NonNull".to_string()]));
    }

    #[test]
    fn test_has_nullable_annotation() {
        assert!(has_nullable_annotation(&["Nullable".to_string()]));
        assert!(has_nullable_annotation(&[
            "javax.annotation.Nullable".to_string()
        ]));
        assert!(has_nullable_annotation(&[
            "org.jetbrains.annotations.Nullable".to_string()
        ]));
    }

    #[test]
    fn test_no_nullability_annotation() {
        assert!(!has_nonnull_annotation(&["Override".to_string()]));
        assert!(!has_nullable_annotation(&["Override".to_string()]));
        assert!(!has_nonnull_annotation(&[]));
        assert!(!has_nullable_annotation(&[]));
    }

    #[test]
    fn test_package_nonnull_annotation() {
        assert!(is_package_nonnull_annotation("NonNullApi"));
        assert!(is_package_nonnull_annotation(
            "org.springframework.lang.NonNullApi"
        ));
        assert!(is_package_nonnull_annotation(
            "ParametersAreNonnullByDefault"
        ));
        assert!(!is_package_nonnull_annotation("NonNull"));
    }

    #[test]
    fn test_annotation_matching_partial_fqn() {
        // Test that partially qualified names work
        assert!(has_nonnull_annotation(&["annotation.Nonnull".to_string()]));
        assert!(has_nullable_annotation(
            &["annotation.Nullable".to_string()]
        ));
    }
}
