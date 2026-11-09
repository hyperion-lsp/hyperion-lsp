// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Name resolution context

use std::collections::HashMap;

/// Trait for workspace symbol lookup
///
/// Allows ResolutionContext to check if a symbol exists without
/// directly depending on WorkspaceIndex.
pub trait SymbolLookup {
    /// Check if a symbol with the given FQN exists in the workspace
    fn symbol_exists(&self, fqn: &str) -> bool;
}

/// Import map for resolving simple names to fully qualified names
pub type ImportMap = HashMap<String, String>;

/// Context for resolving type names to fully qualified names
///
/// Provides the necessary context to resolve simple type names like "User"
/// to fully qualified names like "com.example.User".
#[derive(Debug, Clone)]
pub struct ResolutionContext {
    /// Import map (simple name → FQN)
    ///
    /// Example: {"User" → "com.example.model.User"}
    pub imports: ImportMap,

    /// Wildcard import packages (e.g., `["java.util", "com.example.model"]`)
    ///
    /// Used to resolve types like "ArrayList" when "import java.util.*;" is present
    pub wildcard_imports: Vec<String>,

    /// Current package (e.g., "com.example")
    pub package: Option<String>,

    /// Parent class FQN for nested class resolution
    ///
    /// Example: "com.example.User" allows resolving "Builder" to "com.example.User.Builder"
    pub parent_class_fqn: Option<String>,

    /// Type parameters declared by the enclosing class or method.
    ///
    /// Keeping these names in the resolution context avoids guessing from naming
    /// conventions. Java permits both multi-letter type parameters and concrete
    /// classes with one-letter names.
    type_parameters: Vec<String>,
}

impl ResolutionContext {
    /// Create a new resolution context
    pub const fn new(
        imports: ImportMap,
        package: Option<String>,
        parent_class_fqn: Option<String>,
    ) -> Self {
        Self {
            imports,
            wildcard_imports: Vec::new(),
            package,
            parent_class_fqn,
            type_parameters: Vec::new(),
        }
    }

    /// Create a new resolution context with wildcard imports
    pub const fn with_wildcards(
        imports: ImportMap,
        wildcard_imports: Vec<String>,
        package: Option<String>,
        parent_class_fqn: Option<String>,
    ) -> Self {
        Self {
            imports,
            wildcard_imports,
            package,
            parent_class_fqn,
            type_parameters: Vec::new(),
        }
    }

    /// Add the type parameters declared in the current lexical scope.
    #[must_use]
    pub fn with_type_parameters<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.type_parameters
            .extend(names.into_iter().map(Into::into));
        self
    }

    /// Return whether `name` is an explicitly declared type parameter.
    pub fn is_type_parameter(&self, name: &str) -> bool {
        self.type_parameters
            .iter()
            .any(|parameter| parameter == name)
    }

    /// Return whether fallback resolution has authoritative evidence for `name`.
    ///
    /// Same-package and nested-class fallbacks are intentionally excluded: without
    /// a symbol lookup they are only guesses. Explicit imports and a containing
    /// class self-reference are known bindings.
    pub(crate) fn has_definite_binding(&self, name: &str) -> bool {
        name.contains('.')
            || is_primitive(name)
            || crate::is_core_java_lang_type(name)
            || self.imports.contains_key(name)
            || self
                .parent_class_fqn
                .as_ref()
                .is_some_and(|parent| parent.split('.').next_back().unwrap_or(parent) == name)
    }

    /// Resolve an identifier against a workspace lookup.
    ///
    /// This is the syntax-only counterpart of the workspace type-name engine
    /// (`hyperion_workspace::type_name_resolution`) and walks the same JLS
    /// tiers, verifying each one against `lookup` before taking it:
    /// 1. already qualified (contains `.`)
    /// 2. primitive types
    /// 3. the enclosing type's member types, when they exist — a member type
    ///    shadows a single-type import (JLS §6.5.5.1)
    /// 4. explicit single-type imports (JLS §7.5.1)
    /// 5. the current package, when the type exists — package members shadow
    ///    import-on-demand including java.lang (JLS §6.5.5)
    /// 6. java.lang: the core table that binds without an indexed JDK
    ///    ([`crate::CORE_JAVA_LANG_TYPES`]), then a lookup-verified check for
    ///    the rest of java.lang
    /// 7. on-demand imports whose candidate exists
    /// 8. fallback: the current package (possibly a forward reference), then
    ///    the enclosing type
    ///
    /// Two documented degradations follow from returning a `String` rather
    /// than a resolution: two on-demand imports that both match are a compile
    /// error in Java but yield the first one here, and step 8 returns a
    /// spelling for a name that does not resolve at all. Callers that need
    /// those distinguished must use `ImportResolver::resolve_type_identity`.
    pub fn resolve_identifier_with_lookup(&self, name: &str, lookup: &dyn SymbolLookup) -> String {
        // Handle qualified names (e.g., "Outer.Inner")
        if name.contains('.') {
            // Split on first dot to get outer class and nested parts
            if let Some(dot_pos) = name.find('.') {
                let (outer, nested) = name.split_at(dot_pos);
                let nested = &nested[1..]; // Skip the dot

                // Resolve the outer class first
                let resolved_outer = self.resolve_identifier_with_lookup(outer, lookup);

                // Build potential FQNs
                let full_fqn = format!("{}.{}", resolved_outer, nested);

                // Check if the full FQN exists
                if lookup.symbol_exists(&full_fqn) {
                    return full_fqn;
                }

                // Fallback: return as-is (might be already fully qualified)
                return name.to_string();
            }

            // No dot found? Return as-is
            return name.to_string();
        }

        // Primitive types
        if is_primitive(name) {
            return name.to_string();
        }

        // A member type of the enclosing declaration is the innermost scope,
        // so it shadows a single-type import (JLS §6.5.5.1) — but only when it
        // actually exists, since without evidence `Enclosing.Name` is a guess.
        let nested_fqn = self
            .parent_class_fqn
            .as_ref()
            .map(|p| format!("{}.{}", p, name));
        if let Some(nested) = nested_fqn
            .as_ref()
            .filter(|nested| lookup.symbol_exists(nested))
        {
            return nested.clone();
        }

        // Explicit imports (single-type imports win over everything but a
        // closer lexical scope, JLS §7.5.1)
        if let Some(fqn) = self.imports.get(name) {
            return fqn.clone();
        }

        // The compilation unit's own package shadows implicit import-on-demand
        // (JLS §6.5.5), again only when the candidate exists.
        // The unnamed package still has a concrete top-level candidate: the
        // bare source name. Omitting it made workspace-aware lookup fabricate
        // `Enclosing.Name` whenever lexical owner context was present, even
        // when an indexed top-level `Name` existed in the default package.
        let package_fqn = Some(
            self.package
                .as_ref()
                .map_or_else(|| name.to_owned(), |pkg| format!("{pkg}.{name}")),
        );
        if let Some(pkg) = package_fqn.as_ref().filter(|pkg| lookup.symbol_exists(pkg)) {
            return pkg.clone();
        }

        // java.lang.* implicit imports: hardcoded well-known types work even
        // without an indexed JDK; the lookup-verified check covers the rest of
        // java.lang (Comparable, Iterable, CharSequence, ...).
        if crate::is_core_java_lang_type(name) {
            return format!("java.lang.{}", name);
        }
        let java_lang_fqn = format!("java.lang.{}", name);
        if lookup.symbol_exists(&java_lang_fqn) {
            return java_lang_fqn;
        }

        // Wildcard imports - try each wildcard package
        for wildcard_pkg in &self.wildcard_imports {
            let candidate = format!("{}.{}", wildcard_pkg, name);
            if lookup.symbol_exists(&candidate) {
                return candidate;
            }
        }

        // Nothing exists — fall back to same package (may be a forward
        // reference not yet indexed), then nested, then the bare name.
        match (nested_fqn, package_fqn) {
            (_, Some(pkg)) => pkg,
            (Some(nested), None) => nested,
            (None, None) => name.to_string(),
        }
    }

    /// Resolve an identifier with no workspace evidence — the degraded mode.
    ///
    /// Only the indexer may use this: it runs while the index is being built,
    /// so there is nothing to look up. Everything else must call
    /// [`Self::resolve_identifier_with_lookup`], which verifies each JLS tier
    /// instead of guessing at it.
    ///
    /// Without a lookup, the two tiers that JLS makes conditional on a type
    /// *existing* — the enclosing type's member types and the current package
    /// — cannot be evaluated, so they become guesses and must be ordered by
    /// likelihood rather than by the spec:
    /// 1. already qualified (contains `.`)
    /// 2. primitive types
    /// 3. explicit imports and syntax-proven lexical bindings
    /// 4. self-reference (the name of the containing class)
    /// 5. java.lang types that bind unconditionally
    ///    ([`crate::CORE_JAVA_LANG_TYPES`]) — this precedes the current
    ///    package only because a same-package class shadowing a core JDK name
    ///    is rarer than an ordinary java.lang reference; with a lookup the
    ///    spec order applies
    /// 6. the current package
    /// 7. a member type of the enclosing declaration
    pub fn resolve_identifier_without_lookup(&self, name: &str) -> String {
        // Already qualified
        if name.contains('.') {
            return name.to_string();
        }

        // Primitive types
        if is_primitive(name) {
            return name.to_string();
        }

        // Single-type imports and syntax-proven lexical bindings shadow every
        // implicit import, including java.lang (JLS 6.5.5.1, 7.5.1).
        if let Some(fqn) = self.imports.get(name) {
            return fqn.clone();
        }

        // Self-reference: if name matches the containing class, return the parent FQN
        // This prevents "Builder" from becoming "Builder.Builder" when used as a return type
        if let Some(parent_fqn) = &self.parent_class_fqn {
            let parent_class_name = parent_fqn.split('.').next_back().unwrap_or(parent_fqn);
            if name == parent_class_name {
                return parent_fqn.clone();
            }
        }

        // java.lang.* implicit imports
        if crate::is_core_java_lang_type(name) {
            return format!("java.lang.{}", name);
        }

        // Same package (types in same package don't need imports)
        // Priority over nested class assumption since we can't verify nested class exists
        // without workspace access. Most unqualified types are same-package, not nested.
        if let Some(pkg) = &self.package {
            return format!("{}.{}", pkg, name);
        }

        // In the default package, an unqualified name is still a top-level-name
        // candidate. Assuming `Parent.Name` here fabricates a nested type without
        // symbol evidence and corrupts method return/parameter descriptors. Preserve
        // the legacy nested fallback only for qualified enclosing types; callers that
        // can distinguish either case must use `resolve_identifier_with_lookup`.
        if let Some(parent_fqn) = &self.parent_class_fqn {
            if parent_fqn.contains('.') {
                return format!("{parent_fqn}.{name}");
            }
        }

        name.to_string()
    }
}

/// Check if a name is a primitive type
fn is_primitive(name: &str) -> bool {
    matches!(
        name,
        "boolean" | "byte" | "short" | "int" | "long" | "char" | "float" | "double" | "void"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_types() {
        let context = ResolutionContext::new(HashMap::new(), None, None);

        assert_eq!(context.resolve_identifier_without_lookup("int"), "int");
        assert_eq!(
            context.resolve_identifier_without_lookup("boolean"),
            "boolean"
        );
        assert_eq!(context.resolve_identifier_without_lookup("void"), "void");
    }

    #[test]
    fn test_java_lang_types() {
        let context = ResolutionContext::new(HashMap::new(), None, None);

        assert_eq!(
            context.resolve_identifier_without_lookup("String"),
            "java.lang.String"
        );
        assert_eq!(
            context.resolve_identifier_without_lookup("Object"),
            "java.lang.Object"
        );
        assert_eq!(
            context.resolve_identifier_without_lookup("Integer"),
            "java.lang.Integer"
        );
    }

    #[test]
    fn test_explicit_imports() {
        let mut imports = HashMap::new();
        imports.insert("User".to_string(), "com.example.model.User".to_string());

        let context = ResolutionContext::new(imports, None, None);

        assert_eq!(
            context.resolve_identifier_without_lookup("User"),
            "com.example.model.User"
        );
    }

    #[test]
    fn explicit_binding_shadows_implicit_java_lang_type() {
        let mut imports = HashMap::new();
        imports.insert(
            "StringBuilder".to_string(),
            "custom.StringBuilder".to_string(),
        );
        let context = ResolutionContext::new(imports, None, None);

        assert_eq!(
            context.resolve_identifier_without_lookup("StringBuilder"),
            "custom.StringBuilder"
        );
    }

    #[test]
    fn default_package_does_not_fabricate_nested_type() {
        let context = ResolutionContext::new(HashMap::new(), None, Some("Factory".to_string()));

        assert_eq!(
            context.resolve_identifier_without_lookup("WidenedChoice"),
            "WidenedChoice"
        );
        assert_eq!(
            context.resolve_identifier_without_lookup("Factory"),
            "Factory"
        );
    }

    #[test]
    fn test_nested_class() {
        let context =
            ResolutionContext::new(HashMap::new(), None, Some("com.example.User".to_string()));

        assert_eq!(
            context.resolve_identifier_without_lookup("Builder"),
            "com.example.User.Builder"
        );
    }

    #[test]
    fn test_same_package() {
        let context = ResolutionContext::new(HashMap::new(), Some("com.example".to_string()), None);

        assert_eq!(
            context.resolve_identifier_without_lookup("User"),
            "com.example.User"
        );
    }

    #[test]
    fn test_already_qualified() {
        let context = ResolutionContext::new(HashMap::new(), None, None);

        assert_eq!(
            context.resolve_identifier_without_lookup("java.util.List"),
            "java.util.List"
        );
    }

    struct Indexed(&'static [&'static str]);

    impl SymbolLookup for Indexed {
        fn symbol_exists(&self, fqn: &str) -> bool {
            self.0.contains(&fqn)
        }
    }

    // JLS 6.5.5.1: the enclosing type's member types are the
    // innermost scope, so an existing nested class shadows a single-type
    // import of the same simple name. The lookup-less mode cannot verify the
    // nested candidate and therefore still prefers the import.
    #[test]
    fn an_existing_member_type_shadows_a_single_type_import() {
        let mut imports = HashMap::new();
        imports.insert("Builder".to_string(), "other.Builder".to_string());
        let context = ResolutionContext::new(
            imports,
            Some("com.example".to_string()),
            Some("com.example.User".to_string()),
        );
        let lookup = Indexed(&["com.example.User.Builder", "other.Builder"]);

        assert_eq!(
            context.resolve_identifier_with_lookup("Builder", &lookup),
            "com.example.User.Builder"
        );
        assert_eq!(
            context.resolve_identifier_without_lookup("Builder"),
            "other.Builder",
            "the degraded mode has no evidence the member type exists"
        );
    }

    // An absent member type must not shadow anything.
    #[test]
    fn a_missing_member_type_leaves_the_import_binding_intact() {
        let mut imports = HashMap::new();
        imports.insert("Builder".to_string(), "other.Builder".to_string());
        let context = ResolutionContext::new(
            imports,
            Some("com.example".to_string()),
            Some("com.example.User".to_string()),
        );
        let lookup = Indexed(&["other.Builder"]);

        assert_eq!(
            context.resolve_identifier_with_lookup("Builder", &lookup),
            "other.Builder"
        );
    }

    // The java.lang table is shared with the workspace
    // resolver, so interfaces resolve without an indexed JDK instead of being
    // fabricated as same-package types.
    #[test]
    fn core_java_lang_interfaces_resolve_without_an_indexed_jdk() {
        let context = ResolutionContext::new(HashMap::new(), Some("com.example".to_string()), None);
        let empty = Indexed(&[]);

        for name in ["Comparable", "Iterable", "CharSequence"] {
            assert_eq!(
                context.resolve_identifier_with_lookup(name, &empty),
                format!("java.lang.{name}")
            );
            assert_eq!(
                context.resolve_identifier_without_lookup(name),
                format!("java.lang.{name}")
            );
        }
    }

    // JLS 6.5.5: an existing same-package type still shadows java.lang.
    #[test]
    fn an_existing_same_package_type_shadows_a_core_java_lang_name() {
        let context = ResolutionContext::new(HashMap::new(), Some("com.example".to_string()), None);
        let lookup = Indexed(&["com.example.Record"]);

        assert_eq!(
            context.resolve_identifier_with_lookup("Record", &lookup),
            "com.example.Record"
        );
    }

    #[test]
    fn test_resolution_priority() {
        let mut imports = HashMap::new();
        imports.insert("Builder".to_string(), "com.util.Builder".to_string());

        // Import should take priority over nested class
        let context = ResolutionContext::new(
            imports,
            Some("com.example".to_string()),
            Some("com.example.User".to_string()),
        );

        assert_eq!(
            context.resolve_identifier_without_lookup("Builder"),
            "com.util.Builder"
        );
    }
}
