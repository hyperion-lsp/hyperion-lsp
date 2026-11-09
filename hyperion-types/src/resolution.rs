// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type structure name resolution

use crate::{ResolutionContext, SymbolLookup, TypeStructure};

/// Return whether a name follows Java's conventional single-letter type-parameter style.
///
/// This is only a fallback signal. Callers must prefer explicit lexical scope and
/// concrete symbol/import evidence because legal classes may also be named `T`.
pub fn is_conventional_type_parameter_name(name: &str) -> bool {
    !name.contains('.') && name.len() == 1 && name.chars().next().is_some_and(char::is_uppercase)
}

/// Resolve type structure WITH workspace lookup (preferred)
///
/// Uses workspace-aware resolution to correctly handle ambiguous cases
/// like "Builder" (could be nested or same-package).
pub fn resolve_type_structure_with_lookup(
    type_structure: &TypeStructure,
    context: &ResolutionContext,
    lookup: &dyn SymbolLookup,
) -> TypeStructure {
    // Don't resolve primitive types
    if type_structure.is_primitive {
        return type_structure.clone();
    }

    // Resolve base type name with workspace awareness
    let is_wildcard = matches!(type_structure.base.as_str(), "?" | "? extends" | "? super");
    let candidate = context.resolve_identifier_with_lookup(&type_structure.base, lookup);
    let resolved_base = if is_wildcard
        || context.is_type_parameter(&type_structure.base)
        || (is_conventional_type_parameter_name(&type_structure.base)
            && !context.has_definite_binding(&type_structure.base)
            && !lookup.symbol_exists(&candidate))
    {
        type_structure.base.clone()
    } else {
        candidate
    };

    // Recursively resolve type arguments
    let resolved_type_args = type_structure
        .type_args
        .iter()
        .map(|arg| resolve_type_structure_with_lookup(arg, context, lookup))
        .collect();

    TypeStructure {
        base: resolved_base,
        type_args: resolved_type_args,
        dimensions: type_structure.dimensions,
        is_primitive: type_structure.is_primitive,
        is_diamond: type_structure.is_diamond,
    }
}

/// Resolve a type structure with no workspace evidence — the degraded mode.
///
/// See [`ResolutionContext::resolve_identifier_without_lookup`]: only the
/// indexer, which runs before there is anything to look up, may use this.
///
/// Recursively resolves:
/// - Base type name
/// - Generic type arguments
///
/// Array dimensions are preserved as-is.
///
/// # Example
///
/// ```ignore
/// let unresolved = TypeStructure::class("List".to_string())
///     .with_type_arg(TypeStructure::class("User".to_string()));
///
/// let context = ResolutionContext::new(imports, package, None);
/// let resolved = resolve_type_structure_without_lookup(&unresolved, &context);
///
/// // resolved.base == "java.util.List"
/// // resolved.type_args[0].base == "com.example.User"
/// ```
pub fn resolve_type_structure_without_lookup(
    type_structure: &TypeStructure,
    context: &ResolutionContext,
) -> TypeStructure {
    // Don't resolve primitive types
    if type_structure.is_primitive {
        return type_structure.clone();
    }

    // Resolve base type name
    let is_wildcard = matches!(type_structure.base.as_str(), "?" | "? extends" | "? super");
    let resolved_base = if is_wildcard
        || context.is_type_parameter(&type_structure.base)
        || (is_conventional_type_parameter_name(&type_structure.base)
            && !context.has_definite_binding(&type_structure.base))
    {
        type_structure.base.clone()
    } else {
        context.resolve_identifier_without_lookup(&type_structure.base)
    };

    // Recursively resolve type arguments
    let resolved_type_args = type_structure
        .type_args
        .iter()
        .map(|arg| resolve_type_structure_without_lookup(arg, context))
        .collect();

    TypeStructure {
        base: resolved_base,
        type_args: resolved_type_args,
        dimensions: type_structure.dimensions,
        is_primitive: type_structure.is_primitive,
        is_diamond: type_structure.is_diamond,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct KnownSymbols(HashSet<String>);

    impl SymbolLookup for KnownSymbols {
        fn symbol_exists(&self, fqn: &str) -> bool {
            self.0.contains(fqn)
        }
    }

    #[test]
    fn test_resolve_simple_class() {
        let mut imports = HashMap::new();
        imports.insert("User".to_string(), "com.example.User".to_string());

        let context = ResolutionContext::new(imports, None, None);

        let unresolved = TypeStructure::class("User".to_string());
        let resolved = resolve_type_structure_without_lookup(&unresolved, &context);

        assert_eq!(resolved.base, "com.example.User");
    }

    #[test]
    fn test_resolve_generic_type() {
        let mut imports = HashMap::new();
        imports.insert("List".to_string(), "java.util.List".to_string());
        imports.insert("User".to_string(), "com.example.User".to_string());

        let context = ResolutionContext::new(imports, None, None);

        let unresolved = TypeStructure::class("List".to_string())
            .with_type_arg(TypeStructure::class("User".to_string()));

        let resolved = resolve_type_structure_without_lookup(&unresolved, &context);

        assert_eq!(resolved.base, "java.util.List");
        assert_eq!(resolved.type_args.len(), 1);
        assert_eq!(resolved.type_args[0].base, "com.example.User");
    }

    #[test]
    fn test_resolve_nested_generics() {
        let mut imports = HashMap::new();
        imports.insert("Map".to_string(), "java.util.Map".to_string());
        imports.insert("List".to_string(), "java.util.List".to_string());
        imports.insert("User".to_string(), "com.example.User".to_string());

        let context = ResolutionContext::new(imports, None, None);

        let user_list = TypeStructure::class("List".to_string())
            .with_type_arg(TypeStructure::class("User".to_string()));

        let unresolved = TypeStructure::class("Map".to_string())
            .with_type_args(vec![TypeStructure::class("String".to_string()), user_list]);

        let resolved = resolve_type_structure_without_lookup(&unresolved, &context);

        assert_eq!(resolved.base, "java.util.Map");
        assert_eq!(resolved.type_args[0].base, "java.lang.String"); // String is in java.lang
        assert_eq!(resolved.type_args[1].base, "java.util.List");
        assert_eq!(resolved.type_args[1].type_args[0].base, "com.example.User");
    }

    #[test]
    fn test_preserve_array_dimensions() {
        let mut imports = HashMap::new();
        imports.insert("User".to_string(), "com.example.User".to_string());

        let context = ResolutionContext::new(imports, None, None);

        let unresolved = TypeStructure::class("User".to_string()).with_dimensions(2);

        let resolved = resolve_type_structure_without_lookup(&unresolved, &context);

        assert_eq!(resolved.base, "com.example.User");
        assert_eq!(resolved.dimensions, 2);
    }

    #[test]
    fn test_dont_resolve_primitives() {
        let context = ResolutionContext::new(HashMap::new(), None, None);

        let unresolved = TypeStructure::primitive("int").with_dimensions(1);

        let resolved = resolve_type_structure_without_lookup(&unresolved, &context);

        assert_eq!(resolved.base, "int");
        assert_eq!(resolved.dimensions, 1);
        assert!(resolved.is_primitive);
    }

    #[test]
    fn explicit_import_beats_type_parameter_naming_convention() {
        let imports = HashMap::from([("S".to_string(), "com.example.S".to_string())]);
        let context = ResolutionContext::new(imports, None, None);

        let resolved =
            resolve_type_structure_without_lookup(&TypeStructure::class("S".to_string()), &context);

        assert_eq!(resolved.base, "com.example.S");
    }

    #[test]
    fn indexed_same_package_class_beats_type_parameter_naming_convention() {
        let context = ResolutionContext::new(HashMap::new(), Some("com.example".to_string()), None);
        let symbols = KnownSymbols(HashSet::from(["com.example.T".to_string()]));

        let resolved = resolve_type_structure_with_lookup(
            &TypeStructure::class("T".to_string()),
            &context,
            &symbols,
        );

        assert_eq!(resolved.base, "com.example.T");
    }

    #[test]
    fn indexed_default_package_type_beats_a_missing_nested_candidate() {
        let context = ResolutionContext::new(HashMap::new(), None, Some("Test".to_string()));
        let symbols = KnownSymbols(HashSet::from(["User".to_string()]));

        let resolved = resolve_type_structure_with_lookup(
            &TypeStructure::class("User".to_string()),
            &context,
            &symbols,
        );

        assert_eq!(resolved.base, "User");
    }

    #[test]
    fn indexed_nested_type_still_shadows_a_default_package_type() {
        let context = ResolutionContext::new(HashMap::new(), None, Some("Test".to_string()));
        let symbols = KnownSymbols(HashSet::from(["User".to_string(), "Test.User".to_string()]));

        let resolved = resolve_type_structure_with_lookup(
            &TypeStructure::class("User".to_string()),
            &context,
            &symbols,
        );

        assert_eq!(resolved.base, "Test.User");
    }

    #[test]
    fn declared_multi_letter_type_parameter_is_preserved() {
        let context = ResolutionContext::new(HashMap::new(), Some("com.example".to_string()), None)
            .with_type_parameters(["Element"]);

        let resolved = resolve_type_structure_without_lookup(
            &TypeStructure::class("Element".to_string()),
            &context,
        );

        assert_eq!(resolved.base, "Element");
    }

    #[test]
    fn unresolved_conventional_type_parameter_remains_a_fallback() {
        let context = ResolutionContext::new(HashMap::new(), Some("com.example".to_string()), None);
        let symbols = KnownSymbols(HashSet::new());

        let resolved = resolve_type_structure_with_lookup(
            &TypeStructure::class("T".to_string()),
            &context,
            &symbols,
        );

        assert_eq!(resolved.base, "T");
    }

    #[test]
    fn preserved_type_parameter_does_not_skip_nested_argument_resolution() {
        let imports = HashMap::from([("User".to_string(), "com.example.User".to_string())]);
        let context = ResolutionContext::new(imports, None, None).with_type_parameters(["T"]);
        let unresolved = TypeStructure::class("T".to_string())
            .with_type_arg(TypeStructure::class("User".to_string()));

        let resolved = resolve_type_structure_without_lookup(&unresolved, &context);

        assert_eq!(resolved.base, "T");
        assert_eq!(resolved.type_args[0].base, "com.example.User");
    }
}
