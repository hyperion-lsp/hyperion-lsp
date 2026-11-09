// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Language detection and dispatch
//!
//! This module provides language detection based on file extensions
//! and utilities for dispatching to language-specific implementations.

use std::path::Path;

use lsp_types::SymbolKind;

/// Supported languages in Hyperion LSP
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// Java (.java files)
    Java,
    /// Kotlin (.kt, .kts files)
    Kotlin,
    /// Groovy (.groovy, .gradle files)
    Groovy,
    /// Scala (.scala files)
    Scala,
}

/// Return the JVM declaration FQN used to index a Kotlin built-in or JVM
/// typealias.
///
/// Kotlin source names such as `kotlin.collections.List` and `ArrayList`
/// compile to Java collection declarations. Keeping this table in the
/// workspace language layer lets hierarchy, JAR metadata conversion, and LSP
/// consumers share one identity instead of growing incompatible local maps.
pub fn kotlin_builtin_jvm_fqn(kotlin_fqn: &str) -> Option<&'static str> {
    Some(match kotlin_fqn {
        "kotlin.Any" => "java.lang.Object",
        "kotlin.Nothing" => "java.lang.Void",
        "kotlin.String" => "java.lang.String",
        "kotlin.CharSequence" => "java.lang.CharSequence",
        "kotlin.Throwable" => "java.lang.Throwable",
        "kotlin.Number" => "java.lang.Number",
        "kotlin.Comparable" => "java.lang.Comparable",
        "kotlin.Enum" => "java.lang.Enum",
        "kotlin.Annotation" => "java.lang.annotation.Annotation",
        "kotlin.collections.Iterable" | "kotlin.collections.MutableIterable" => {
            "java.lang.Iterable"
        }
        "kotlin.collections.Iterator" | "kotlin.collections.MutableIterator" => {
            "java.util.Iterator"
        }
        "kotlin.collections.Collection" | "kotlin.collections.MutableCollection" => {
            "java.util.Collection"
        }
        "kotlin.collections.List" | "kotlin.collections.MutableList" => "java.util.List",
        "kotlin.collections.Set" | "kotlin.collections.MutableSet" => "java.util.Set",
        "kotlin.collections.Map" | "kotlin.collections.MutableMap" => "java.util.Map",
        "kotlin.collections.Map.Entry" | "kotlin.collections.MutableMap.MutableEntry" => {
            "java.util.Map.Entry"
        }
        "kotlin.collections.ListIterator" | "kotlin.collections.MutableListIterator" => {
            "java.util.ListIterator"
        }
        "kotlin.collections.ArrayList" => "java.util.ArrayList",
        "kotlin.collections.HashMap" => "java.util.HashMap",
        "kotlin.collections.HashSet" => "java.util.HashSet",
        "kotlin.collections.LinkedHashMap" => "java.util.LinkedHashMap",
        "kotlin.collections.LinkedHashSet" => "java.util.LinkedHashSet",
        "kotlin.collections.RandomAccess" => "java.util.RandomAccess",
        _ => return None,
    })
}

/// Implicit on-demand type-import packages for one source language.
///
/// This intentionally models language rules rather than treating every JVM
/// source file as Java. Kotlin mapped built-ins are canonicalized separately
/// through [`kotlin_builtin_jvm_fqn`].
pub fn implicit_type_import_packages(language: Option<Language>) -> &'static [&'static str] {
    match language {
        Some(Language::Kotlin) => &[
            "kotlin",
            "kotlin.annotation",
            "kotlin.collections",
            "kotlin.comparisons",
            "kotlin.io",
            "kotlin.math",
            "kotlin.ranges",
            "kotlin.sequences",
            "kotlin.text",
            "java.lang",
            "kotlin.jvm",
        ],
        Some(Language::Groovy) => &[
            "java.lang",
            "java.util",
            "java.io",
            "java.net",
            "java.time",
            "groovy.lang",
            "groovy.util",
        ],
        // Scala root imports shadow from left to right here: Predef is the
        // nearest root context, followed by scala, then java.lang.
        Some(Language::Scala) => &["scala.Predef", "scala", "java.lang"],
        Some(Language::Java) | None => &["java.lang"],
    }
}

/// Implicit single-type import for one source language.
///
/// Groovy imports these two declarations, not the entire `java.math`
/// package. Keeping them separate from on-demand packages prevents unrelated
/// `java.math` types from becoming visible without an explicit import.
pub fn implicit_single_type_import(
    language: Option<Language>,
    simple_name: &str,
) -> Option<&'static str> {
    match (language, simple_name) {
        (Some(Language::Groovy), "BigInteger") => Some("java.math.BigInteger"),
        (Some(Language::Groovy), "BigDecimal") => Some("java.math.BigDecimal"),
        _ => None,
    }
}

/// Exact JVM supertype synthesized by a source language rather than written
/// in its supertype list.
///
/// Kotlin bare header entries are role-neutral until their targets are known.
/// `bare_supertypes_are_interfaces` therefore gates Object synthesis: a bare
/// class target is the real superclass, and an unresolved target must remain
/// conservative instead of temporarily publishing Object as a direct edge.
pub(crate) fn implicit_jvm_supertype(
    language: Option<Language>,
    kind: SymbolKind,
    fqn: &str,
    parent_class: Option<&str>,
    bare_supertypes_are_interfaces: bool,
) -> Option<&'static str> {
    let target = match (language, kind) {
        (Some(Language::Java), SymbolKind::CLASS) if parent_class.is_none() => "java.lang.Object",
        (Some(Language::Java), SymbolKind::ENUM) => "java.lang.Enum",
        (Some(Language::Java), SymbolKind::STRUCT) => "java.lang.Record",
        (Some(Language::Java), SymbolKind::INTERFACE)
            if parent_class == Some("java.lang.annotation.Annotation") =>
        {
            "java.lang.annotation.Annotation"
        }
        (Some(Language::Kotlin | Language::Groovy | Language::Scala), SymbolKind::ENUM) => {
            "java.lang.Enum"
        }
        (Some(Language::Kotlin), SymbolKind::INTERFACE)
            if parent_class == Some("java.lang.annotation.Annotation") =>
        {
            "java.lang.annotation.Annotation"
        }
        (Some(Language::Kotlin), SymbolKind::CLASS | SymbolKind::OBJECT | SymbolKind::STRUCT)
            if parent_class.is_none() && bare_supertypes_are_interfaces =>
        {
            "java.lang.Object"
        }
        (
            Some(Language::Groovy | Language::Scala),
            SymbolKind::CLASS | SymbolKind::OBJECT | SymbolKind::STRUCT,
        ) if parent_class.is_none() => "java.lang.Object",
        _ => return None,
    };
    (target != fqn).then_some(target)
}

impl Language {
    /// Detect language from file path based on extension
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use hyperion_workspace::Language;
    /// use std::path::Path;
    ///
    /// assert_eq!(Language::from_path(Path::new("Foo.java")), Some(Language::Java));
    /// assert_eq!(Language::from_path(Path::new("Bar.kt")), Some(Language::Kotlin));
    /// assert_eq!(Language::from_path(Path::new("script.py")), None);
    /// ```
    pub fn from_path(path: &Path) -> Option<Language> {
        let ext = path.extension()?.to_str()?;
        match ext {
            "java" => Some(Language::Java),
            "kt" | "kts" => Some(Language::Kotlin),
            "groovy" | "gradle" => Some(Language::Groovy),
            "scala" => Some(Language::Scala),
            _ => None,
        }
    }

    /// Get file extensions for this language
    ///
    /// Returns a slice of file extensions (without the dot) that
    /// correspond to this language.
    pub fn extensions(&self) -> &[&str] {
        match self {
            Language::Java => &["java"],
            Language::Kotlin => &["kt", "kts"],
            Language::Groovy => &["groovy", "gradle"],
            Language::Scala => &["scala"],
        }
    }

    /// Get the display name for this language
    pub fn name(&self) -> &str {
        match self {
            Language::Java => "Java",
            Language::Kotlin => "Kotlin",
            Language::Groovy => "Groovy",
            Language::Scala => "Scala",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_path_java() {
        assert_eq!(
            Language::from_path(Path::new("Foo.java")),
            Some(Language::Java)
        );
        assert_eq!(
            Language::from_path(Path::new("com/example/Bar.java")),
            Some(Language::Java)
        );
    }

    #[test]
    fn test_from_path_kotlin() {
        assert_eq!(
            Language::from_path(Path::new("Foo.kt")),
            Some(Language::Kotlin)
        );
        assert_eq!(
            Language::from_path(Path::new("com/example/Bar.kt")),
            Some(Language::Kotlin)
        );
    }

    #[test]
    fn test_from_path_kotlin_script() {
        assert_eq!(
            Language::from_path(Path::new("build.gradle.kts")),
            Some(Language::Kotlin)
        );
        assert_eq!(
            Language::from_path(Path::new("settings.gradle.kts")),
            Some(Language::Kotlin)
        );
        assert_eq!(
            Language::from_path(Path::new("script.kts")),
            Some(Language::Kotlin)
        );
    }

    #[test]
    fn test_from_path_groovy() {
        assert_eq!(
            Language::from_path(Path::new("Foo.groovy")),
            Some(Language::Groovy)
        );
        assert_eq!(
            Language::from_path(Path::new("build.gradle")),
            Some(Language::Groovy)
        );
        assert_eq!(
            Language::from_path(Path::new("settings.gradle")),
            Some(Language::Groovy)
        );
    }

    #[test]
    fn test_from_path_unknown() {
        assert_eq!(Language::from_path(Path::new("script.py")), None);
        assert_eq!(Language::from_path(Path::new("README.md")), None);
        assert_eq!(Language::from_path(Path::new("Makefile")), None);
    }

    #[test]
    fn test_extensions() {
        assert_eq!(Language::Java.extensions(), &["java"]);
        assert_eq!(Language::Kotlin.extensions(), &["kt", "kts"]);
        assert_eq!(Language::Groovy.extensions(), &["groovy", "gradle"]);
    }

    #[test]
    fn test_name() {
        assert_eq!(Language::Java.name(), "Java");
        assert_eq!(Language::Kotlin.name(), "Kotlin");
        assert_eq!(Language::Groovy.name(), "Groovy");
    }
}
