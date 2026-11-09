// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Parser-independent declaration facts shared by language extractors and the workspace index.

use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

/// Language-neutral declaration modifier facts retained by the workspace index.
///
/// The model is the union of modifiers currently extracted from Java, Kotlin,
/// and Groovy. A language leaves unsupported fields false; language-specific
/// semantics remain the responsibility of that language's analysis layer.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclarationModifiers {
    pub is_public: bool,
    pub is_private: bool,
    pub is_protected: bool,
    pub is_internal: bool,
    pub is_static: bool,
    pub is_final: bool,
    pub is_open: bool,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_data: bool,
    pub is_suspend: bool,
    pub is_inline: bool,
    pub is_operator: bool,
    pub is_infix: bool,
    /// Declared as a reassignable binding (Scala `var`). Scala extraction sets this;
    /// other languages leave it false. The index is bincode, which is positional and
    /// ignores `serde(default)`, so the cache version is bumped with this field.
    #[serde(default)]
    pub is_mutable: bool,
}

/// Imports visible to declarations in one source file.
///
/// Languages without static imports leave the static collections empty. The
/// add methods deduplicate imports while preserving source order.
///
/// Explicit static imports are a sequence rather than a simple-name map. Java
/// permits several single-static-import declarations with the same spelling
/// when they denote fields or methods; overload resolution and ambiguity must
/// see every declaration rather than whichever one happened to be parsed last.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclarationImports {
    pub current_package: Option<String>,
    pub explicit: HashMap<String, String>,
    pub wildcards: Vec<String>,
    pub static_imports: Vec<StaticImport>,
    pub static_wildcards: Vec<String>,
}

/// One explicit static import.
///
/// Java uses the member's natural spelling, so the common case stores only the
/// FQN; Groovy/Kotlin aliases pay for the additional string only when the
/// imported spelling actually differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaticImport {
    pub fqn: Box<str>,
    pub alias: Option<Box<str>>,
}

impl StaticImport {
    pub fn imported_name(&self) -> &str {
        self.alias
            .as_deref()
            .or_else(|| self.fqn.rsplit('.').next())
            .unwrap_or(&self.fqn)
    }
}

impl DeclarationImports {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_import(&mut self, simple_name: String, fqn: String) {
        self.explicit.insert(simple_name, fqn);
    }

    pub fn add_wildcard(&mut self, package: String) {
        if !self.wildcards.contains(&package) {
            self.wildcards.push(package);
        }
    }

    pub fn add_static_import(&mut self, simple_name: String, fqn: String) {
        let alias = (fqn.rsplit('.').next() != Some(simple_name.as_str()))
            .then(|| simple_name.into_boxed_str());
        let import = StaticImport {
            fqn: fqn.into_boxed_str(),
            alias,
        };
        if !self.static_imports.contains(&import) {
            self.static_imports.push(import);
        }
    }

    pub fn add_static_wildcard(&mut self, type_fqn: String) {
        if !self.static_wildcards.contains(&type_fqn) {
            self.static_wildcards.push(type_fqn);
        }
    }

    /// Iterate over every explicit static import with this simple name in
    /// source order. More than one binding is semantically meaningful.
    pub fn static_imports_named<'a>(
        &'a self,
        simple_name: &'a str,
    ) -> impl Iterator<Item = &'a str> + 'a {
        self.static_imports
            .iter()
            .filter(move |binding| binding.imported_name() == simple_name)
            .map(|binding| binding.fqn.as_ref())
    }

    pub fn static_import_fqns(&self) -> impl Iterator<Item = &str> {
        self.static_imports
            .iter()
            .map(|binding| binding.fqn.as_ref())
    }
}

/// One declared class, interface, or method type parameter.
///
/// For `Map<K, V>`, `K` has position 0 and `V` has position 1. Bounds retain
/// their source-level type spelling so the semantic layer can resolve them in
/// the declaration's package/import context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeParameterInfo {
    /// Type parameter name (for example `T`, `Key`, or `Value`).
    #[serde(with = "arc_str_serde")]
    pub name: Arc<str>,
    /// Zero-based position in the declaration's type-parameter list.
    pub position: usize,
    /// Declared bounds, in source order.
    #[serde(with = "vec_arc_str_serde")]
    pub bounds: Vec<Arc<str>>,
}

impl TypeParameterInfo {
    /// Create a type parameter without explicit bounds.
    pub fn new(name: impl Into<Arc<str>>, position: usize) -> Self {
        Self {
            name: name.into(),
            position,
            bounds: Vec::new(),
        }
    }

    /// Create a type parameter with its source-order bounds.
    pub fn with_bounds<N, B, I>(name: N, position: usize, bounds: I) -> Self
    where
        N: Into<Arc<str>>,
        B: Into<Arc<str>>,
        I: IntoIterator<Item = B>,
    {
        Self {
            name: name.into(),
            position,
            bounds: bounds.into_iter().map(Into::into).collect(),
        }
    }
}

mod arc_str_serde {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_ref().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Into::into)
    }
}

mod vec_arc_str_serde {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &[Arc<str>], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Arc<str>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<String>::deserialize(deserializer)
            .map(|values| values.into_iter().map(Into::into).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_accept_owned_and_shared_strings() {
        let owned = TypeParameterInfo::with_bounds(
            "Value".to_string(),
            1,
            ["java.lang.Number".to_string()],
        );
        let shared = TypeParameterInfo::with_bounds(
            Arc::<str>::from("Value"),
            1,
            [Arc::<str>::from("java.lang.Number")],
        );

        assert_eq!(owned, shared);
    }

    #[test]
    fn declaration_modifiers_retain_cross_language_facts() {
        let modifiers = DeclarationModifiers {
            is_static: true,
            is_internal: true,
            is_operator: true,
            ..DeclarationModifiers::default()
        };

        assert!(modifiers.is_static);
        assert!(modifiers.is_internal);
        assert!(modifiers.is_operator);
        assert!(!modifiers.is_abstract);
    }

    #[test]
    fn declaration_imports_deduplicate_wildcards_in_source_order() {
        let mut imports = DeclarationImports::new();
        imports.add_wildcard("java.util".to_string());
        imports.add_wildcard("java.time".to_string());
        imports.add_wildcard("java.util".to_string());
        imports.add_static_wildcard("java.util.Collections".to_string());
        imports.add_static_wildcard("java.util.Collections".to_string());

        assert_eq!(imports.wildcards, ["java.util", "java.time"]);
        assert_eq!(imports.static_wildcards, ["java.util.Collections"]);
    }

    #[test]
    fn declaration_imports_preserve_same_named_static_bindings_in_source_order() {
        let mut imports = DeclarationImports::new();
        imports.add_static_import("pick".to_string(), "first.Tools.pick".to_string());
        imports.add_static_import("pick".to_string(), "second.Tools.pick".to_string());
        imports.add_static_import("pick".to_string(), "first.Tools.pick".to_string());
        imports.add_static_import("alias".to_string(), "third.Tools.pick".to_string());

        assert_eq!(
            imports.static_imports_named("pick").collect::<Vec<_>>(),
            ["first.Tools.pick", "second.Tools.pick"]
        );
        assert_eq!(
            imports.static_imports_named("alias").collect::<Vec<_>>(),
            ["third.Tools.pick"]
        );
        assert!(
            std::mem::size_of::<DeclarationImports>() <= 144,
            "lossless static imports should not make the per-file import header larger"
        );
        assert!(
            std::mem::size_of::<StaticImport>() <= 4 * std::mem::size_of::<usize>(),
            "one static binding should be at most four machine words"
        );
    }
}
