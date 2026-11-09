// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Symbol extractor: parses Java files and extracts symbol information
//
// Extracts:
// - Package declarations
// - Import statements
// - Top-level type declarations (classes, interfaces, enums, records)
// - Methods, fields, and constructors

use crate::{ImportMap, Modifiers, Symbol, TextRange};
use anyhow::Result;
use hyperion_parser::{
    lexer::Lexer,
    parser::{ParseMode, Parser},
    syntax::{SyntaxKind, SyntaxNode},
};
use hyperion_types::{
    enclosing_type_parameter_names, extract_and_resolve_type_without_lookup, ResolutionContext,
};
use lsp_types::SymbolKind;
use rowan::NodeOrToken;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::warn;

/// Check if a node has a @Deprecated annotation
///
/// PERFORMANCE: Uses single-pass iteration with early exit instead of
/// multiple descendants_with_tokens() calls per annotation
pub(crate) fn has_deprecated_annotation(node: &SyntaxNode) -> bool {
    // Handle annotations represented inside the declaration itself.
    for child in node.children() {
        match child.kind() {
            SyntaxKind::ANNOTATION if is_deprecated_annotation(&child) => return true,
            SyntaxKind::MODIFIER => {
                // Check annotations within modifier node
                for annotation in child.children() {
                    if annotation.kind() == SyntaxKind::ANNOTATION
                        && is_deprecated_annotation(&annotation)
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }

    // In the parser's canonical shape, annotations and keyword modifiers live
    // in a MODIFIER_LIST sibling immediately before the declaration.
    {
        let preceding_modifier_list =
            crate::type_declaration_extractor::preceding_modifier_list(node);
        if preceding_modifier_list.is_some_and(|modifier_list| {
            modifier_list.descendants().any(|child| {
                child.kind() == SyntaxKind::ANNOTATION && is_deprecated_annotation(&child)
            })
        }) {
            return true;
        }
    }

    false
}

/// Check if an ANNOTATION node is @Deprecated (handles both simple and qualified names)
fn is_deprecated_annotation(annotation: &SyntaxNode) -> bool {
    // Annotation names are inside QUALIFIED_NAME: @Deprecated -> ANNOTATION > QUALIFIED_NAME > IDENT
    // For @java.lang.Deprecated: ANNOTATION > QUALIFIED_NAME > IDENT("java") DOT IDENT("lang") DOT IDENT("Deprecated")
    // We need the LAST IDENT to get the simple name
    for child in annotation.children() {
        if child.kind() == SyntaxKind::QUALIFIED_NAME {
            let mut last_ident: Option<String> = None;
            for token in child.children_with_tokens() {
                if let NodeOrToken::Token(t) = token {
                    if t.kind().is_identifier() {
                        last_ident = Some(t.text().to_string());
                    }
                }
            }
            if let Some(name) = last_ident {
                if name == "Deprecated" {
                    return true;
                }
            }
        }
    }
    false
}

/// Extracts symbols and imports from Java source code
pub struct SymbolExtractor;

fn is_type_declaration_syntax(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
            | SyntaxKind::ANNOTATION_DECL
    )
}

impl SymbolExtractor {
    /// Extract all symbols from a Java source file
    pub fn extract(source: &str) -> Result<ExtractionResult> {
        // Lex and parse (DeclarationsOnly mode skips method bodies for faster indexing)
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse_with_mode(tokens, ParseMode::DeclarationsOnly);
        let root = SyntaxNode::new_root(green_node);

        let package = Self::extract_package(&root);
        let imports = Self::extract_imports(&root);
        let (symbols, errors) = Self::extract_symbols(&root, &package, &imports);
        let generic_supertypes =
            Self::extract_generic_supertypes(&root, &symbols, &package, &imports);

        Ok(ExtractionResult {
            package,
            imports,
            symbols,
            generic_supertypes,
            errors,
        })
    }

    /// Extract the package declaration
    fn extract_package(root: &SyntaxNode) -> Option<String> {
        for child in root.children() {
            if child.kind() == SyntaxKind::PACKAGE_DECL {
                // Find the package name parts
                let mut parts = Vec::new();
                for token in child.descendants_with_tokens() {
                    if let NodeOrToken::Token(token) = token {
                        if token.kind().is_identifier() {
                            parts.push(token.text().to_string());
                        }
                    }
                }
                if !parts.is_empty() {
                    return Some(parts.join("."));
                }
            }
        }
        None
    }

    /// Extract import statements
    fn extract_imports(root: &SyntaxNode) -> ImportMap {
        use tracing::debug;

        let mut imports = ImportMap::new();

        debug!("Scanning for imports");
        let mut import_count = 0;
        for child in root.children() {
            if child.kind() == SyntaxKind::IMPORT_DECL {
                Self::process_import(&child, &mut imports);
                import_count += 1;
            }
        }
        debug!(
            "Found {} import declarations ({} explicit, {} wildcards)",
            import_count,
            imports.explicit.len(),
            imports.wildcards.len()
        );

        imports
    }

    /// Process a single import declaration
    fn process_import(node: &SyntaxNode, imports: &mut ImportMap) {
        let mut is_static = false;
        let mut is_wildcard = false;
        let mut path_parts = Vec::new();

        for token in node.descendants_with_tokens() {
            match token {
                NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::STATIC_KW => is_static = true,
                    SyntaxKind::STAR => is_wildcard = true,
                    kind if kind.is_identifier() => path_parts.push(token.text().to_string()),
                    _ => {}
                },
                NodeOrToken::Node(_) => {}
            }
        }

        if path_parts.is_empty() {
            return;
        }

        let full_path = path_parts.join(".");

        if is_static {
            if is_wildcard {
                // static import com.example.Utils.*
                imports.add_static_wildcard(full_path);
            } else {
                // static import com.example.Utils.helper
                if let Some(simple_name) = path_parts.last() {
                    imports.add_static_import(simple_name.clone(), full_path);
                }
            }
        } else if is_wildcard {
            // import com.example.*
            imports.add_wildcard(full_path);
        } else {
            // import com.example.MyClass
            if let Some(simple_name) = path_parts.last() {
                imports.add_import(simple_name.clone(), full_path);
            }
        }
    }

    /// Resolve a type from an AST node to its fully qualified name using imports
    ///
    /// Uses the shared hyperion-types crate for type resolution.
    ///
    /// This is an AST-based approach that properly handles:
    /// - Array types: User[] → com.example.model.User[]
    /// - Generic types: List<User> → java.util.List<com.example.model.User>
    /// - Nested generics: Map<String, User> → java.util.Map<String, com.example.model.User>
    ///
    /// Resolve a type node to its fully qualified name
    ///
    /// Directly declared nested types are resolved from the enclosing type's AST.
    /// Other ambiguous names still use the same-package fallback because extraction
    /// does not have access to the workspace index yet.
    ///
    /// Examples:
    /// - `Builder` in `com.example.User` could be:
    ///   - `com.example.User.Builder` (nested class) ← CORRECT for builder pattern
    ///   - `com.example.Builder` (same package) ← Current fallback behavior
    ///
    fn resolve_type_from_node(
        node: &SyntaxNode,
        imports: &ImportMap,
        package: &Option<String>,
        parent_class_fqn: Option<&str>,
    ) -> String {
        // Primitive types cannot be shadowed by imports or member types.
        // Avoid scanning the enclosing generated class for each primitive field.
        if node.kind() == SyntaxKind::PRIMITIVE_TYPE {
            if let Ok(ty) = hyperion_types::extract_type_structure(node) {
                return ty.to_fqn_string();
            }
        }

        // A member type declared directly in the current class is authoritative
        // and shadows same-package guesses. Add those lexical bindings to the
        // explicit map so generic type arguments receive the same treatment.
        let mut explicit = imports.explicit.clone();
        if let Some(root) = node.ancestors().last() {
            for declaration in root
                .children()
                .filter(|child| is_type_declaration_syntax(child.kind()))
            {
                if let Some(name) = Self::declaration_simple_name(&declaration) {
                    let fqn = package
                        .as_ref()
                        .map_or_else(|| name.clone(), |package| format!("{package}.{name}"));
                    explicit.insert(name, fqn);
                }
            }
        }
        if let Some(parent_fqn) = parent_class_fqn {
            let parent_simple = parent_fqn.split('.').next_back().unwrap_or(parent_fqn);
            let enclosing_type = node.ancestors().find(|ancestor| {
                is_type_declaration_syntax(ancestor.kind())
                    && Self::declaration_simple_name(ancestor).as_deref() == Some(parent_simple)
            });

            if let Some(enclosing_type) = enclosing_type {
                for nested in enclosing_type
                    .children()
                    .filter(|child| is_type_declaration_syntax(child.kind()))
                {
                    if let Some(name) = Self::declaration_simple_name(&nested) {
                        explicit.insert(name.clone(), format!("{parent_fqn}.{name}"));
                    }
                }
            }
        }

        // Create resolution context from workspace ImportMap
        let context = ResolutionContext::new(
            explicit,
            package.clone(),
            parent_class_fqn.map(String::from),
        )
        .with_type_parameters(enclosing_type_parameter_names(node));

        // Extract and resolve type structure
        // NOTE: Uses fallback resolution (same-package priority) since workspace isn't available
        match extract_and_resolve_type_without_lookup(node, &context) {
            Ok(type_structure) => type_structure.to_fqn_string(),
            Err(e) => {
                tracing::warn!(
                    "Failed to extract type from node (kind={:?}): {}. Falling back to Object.",
                    node.kind(),
                    e
                );
                "Object".to_string() // Fallback for malformed types
            }
        }
    }

    /// Collect member-type bindings visible from a nested declaration, from
    /// the nearest enclosing type outward. The syntax tree is authoritative
    /// here: lexical declarations shadow single-type imports even when the
    /// imported type was indexed first.
    fn lexical_member_type_bindings(
        node: &SyntaxNode,
        enclosing_class_fqn: &str,
    ) -> HashMap<String, String> {
        let mut bindings = HashMap::new();
        let mut owner_fqn = enclosing_class_fqn;
        for enclosing in node
            .ancestors()
            .skip(1)
            .filter(|ancestor| is_type_declaration_syntax(ancestor.kind()))
        {
            for nested in enclosing
                .children()
                .filter(|child| is_type_declaration_syntax(child.kind()))
            {
                if let Some(name) = Self::declaration_simple_name(&nested) {
                    bindings
                        .entry(name.clone())
                        .or_insert_with(|| format!("{owner_fqn}.{name}"));
                }
            }
            let Some((outer, _)) = owner_fqn.rsplit_once('.') else {
                break;
            };
            owner_fqn = outer;
        }
        bindings
    }

    fn declaration_simple_name(node: &SyntaxNode) -> Option<String> {
        node.descendants_with_tokens()
            .find_map(|element| match element {
                NodeOrToken::Token(token) if token.kind().is_type_identifier() => {
                    Some(token.text().to_string())
                }
                _ => None,
            })
    }

    fn extract_generic_supertypes(
        root: &SyntaxNode,
        symbols: &[Symbol],
        package: &Option<String>,
        imports: &ImportMap,
    ) -> Vec<crate::TypeGenericSupertypes> {
        root.descendants()
            .filter(|node| is_type_declaration_syntax(node.kind()))
            .filter_map(|node| {
                let name_range = node.descendants_with_tokens().find_map(|element| {
                    let token = element.as_token()?;
                    token
                        .kind()
                        .is_type_identifier()
                        .then_some(token.text_range())
                })?;
                let declaration = symbols.iter().find(|symbol| {
                    crate::is_type_declaration_kind(symbol.kind)
                        && symbol.range.start == u32::from(name_range.start())
                        && symbol.range.end == u32::from(name_range.end())
                })?;
                let lexical_types = declaration
                    .container
                    .as_deref()
                    .map_or_else(HashMap::new, |container| {
                        Self::lexical_member_type_bindings(&node, container)
                    });
                let edges =
                    crate::parent_interface_extractor::extract_generic_supertypes_in_context(
                        &node,
                        package,
                        imports,
                        &lexical_types,
                    );
                (!edges.is_empty()).then(|| crate::TypeGenericSupertypes {
                    type_fqn: declaration.fqn.to_string(),
                    edges,
                })
            })
            .collect()
    }

    /// Build a parameter descriptor string for method FQN disambiguation.
    /// Converts ["String name", "Map<K,V> map", "int... nums"] to "String,Map,int..."
    fn build_param_descriptor(parameters: &[String]) -> String {
        parameters
            .iter()
            .map(|p| {
                // Parameter format: "TypeName paramName" or "TypeName... paramName"
                // We need just the type name (first word, possibly with ... suffix)
                let parts: Vec<&str> = p.split_whitespace().collect();
                if parts.is_empty() {
                    return "Object".to_string();
                }

                let type_part = parts[0];

                // Handle varargs: "String..." -> "String..."
                // Handle generics: "Map<String,Value>" -> "Map"
                // Handle arrays: "String[]" -> "String[]"
                let simple_type = if let Some(angle_pos) = type_part.find('<') {
                    // Strip generic parameters but keep varargs/array suffix
                    let base = &type_part[..angle_pos];
                    // Check if there's array or varargs after the generic closing
                    if type_part.ends_with("...") {
                        format!("{}...", base)
                    } else if type_part.contains("[]") {
                        // Count array dimensions after generic
                        let array_suffix = type_part.matches("[]").count();
                        format!("{}{}", base, "[]".repeat(array_suffix))
                    } else {
                        base.to_string()
                    }
                } else {
                    type_part.to_string()
                };

                simple_type
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Extract method/constructor parameters from a declaration node.
    /// Returns a vector of parameter strings in the form "TypeName paramName"
    fn extract_method_parameters(
        node: &SyntaxNode,
        imports: &ImportMap,
        package: &Option<String>,
        parent_class_fqn: Option<&str>,
    ) -> Vec<String> {
        use hyperion_parser::syntax::{AstNode, ParamList};

        // Find the PARAM_LIST child
        let param_list = node.children().find(|n| n.kind() == SyntaxKind::PARAM_LIST);

        let Some(param_list_node) = param_list else {
            return vec![];
        };

        let Some(param_list) = ParamList::cast(param_list_node.clone()) else {
            return vec![];
        };

        param_list
            .params()
            .map(|param| {
                // Get parameter type
                // IMPORTANT: For array parameters like "Object[] args", the parser creates:
                //   PARAM
                //   ├── CLASS_TYPE ("Object")
                //   ├── ARRAY_TYPE ("[]")  <- SIBLING, not child of CLASS_TYPE!
                //   └── IDENT ("args")
                //
                // This is a non-JLS compliant structure. We need to manually handle it:
                // 1. Extract base type (CLASS_TYPE or PRIMITIVE_TYPE)
                // 2. Count array dimensions (number of ARRAY_TYPE siblings)
                // 3. Append "[]" for each dimension
                let param_syntax = param.syntax();

                // Extract base type
                let base_type = param
                    .ty()
                    .map(|ty| {
                        let ty_node = ty.syntax();
                        Self::resolve_type_from_node(ty_node, imports, package, parent_class_fqn)
                    })
                    .unwrap_or_else(|| "Object".to_string());

                // Count array dimensions by counting ARRAY_TYPE children
                let array_dimensions = param_syntax
                    .children()
                    .filter(|n| n.kind() == SyntaxKind::ARRAY_TYPE)
                    .count();

                // Build full type with array brackets
                let param_type = if array_dimensions > 0 {
                    format!("{}{}", base_type, "[]".repeat(array_dimensions))
                } else {
                    base_type
                };

                // Get parameter name
                let param_name = param
                    .name()
                    .map(|n| n.text().to_string())
                    .unwrap_or_else(|| "arg".to_string());

                // Check if varargs
                let varargs_suffix = if param.is_varargs() { "..." } else { "" };

                // Format as "Type name" (just type for now, with name available)
                format!("{}{} {}", param_type, varargs_suffix, param_name)
            })
            .collect()
    }

    /// Extract all symbols (types, methods, fields) from the AST
    fn extract_symbols(
        root: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
    ) -> (Vec<Symbol>, Vec<SymbolExtractionError>) {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let mut symbols = Vec::new();
        let mut errors = Vec::new();

        for child in root.children() {
            // Wrap extraction in catch_unwind for error recovery
            // If extraction panics for one top-level type, we continue with others
            let extraction_result = catch_unwind(AssertUnwindSafe(|| {
                match child.kind() {
                    SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::ANNOTATION_DECL => {
                        let kind = match child.kind() {
                            SyntaxKind::CLASS_DECL => SymbolKind::CLASS,
                            SyntaxKind::INTERFACE_DECL | SyntaxKind::ANNOTATION_DECL => {
                                SymbolKind::INTERFACE
                            }
                            SyntaxKind::ENUM_DECL => SymbolKind::ENUM,
                            _ => unreachable!(),
                        };

                        if let Some(type_sym) = crate::type_declaration_extractor::extract_type_decl(
                            &child, package, imports, kind,
                        ) {
                            let class_fqn = type_sym.fqn.clone();
                            let mut local_symbols = vec![type_sym];

                            // Extract methods and fields from the class body
                            Self::extract_class_members(
                                &child,
                                package,
                                imports,
                                &class_fqn,
                                &mut local_symbols,
                            );

                            local_symbols
                        } else {
                            Vec::new()
                        }
                    }
                    SyntaxKind::RECORD_DECL => {
                        if let Some(type_sym) = crate::type_declaration_extractor::extract_type_decl(
                            &child,
                            package,
                            imports,
                            SymbolKind::STRUCT,
                        ) {
                            let record_fqn = type_sym.fqn.clone();
                            let record_range = type_sym.range;
                            let mut local_symbols = vec![type_sym];

                            // Extract record synthetic methods
                            if let Some(record_info) = crate::record_support::RecordInfo::from_node(
                                &child,
                                package,
                                imports,
                                &record_fqn,
                            ) {
                                let record_methods = crate::record_support::generate_record_methods(
                                    &record_info,
                                    &record_fqn,
                                    record_range,
                                );
                                local_symbols.extend(record_methods);
                            }

                            // Also extract any custom methods defined in the record
                            Self::extract_class_members(
                                &child,
                                package,
                                imports,
                                &record_fqn,
                                &mut local_symbols,
                            );

                            local_symbols
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                }
            }));

            match extraction_result {
                Ok(extracted_symbols) => {
                    symbols.extend(extracted_symbols);
                }
                Err(panic_payload) => {
                    // Extraction panicked - log error and continue with other symbols
                    let error_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown panic during symbol extraction".to_string()
                    };

                    warn!(
                        "Symbol extraction failed for node at {:?}: {}",
                        child.text_range(),
                        error_msg
                    );

                    let rowan_range = child.text_range();
                    errors.push(SymbolExtractionError {
                        range: Some(TextRange {
                            start: rowan_range.start().into(),
                            end: rowan_range.end().into(),
                        }),
                        message: error_msg,
                    });
                }
            }
        }

        (symbols, errors)
    }

    /// Extract methods and fields from a class/interface body
    fn extract_class_members(
        class_node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        class_fqn: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        Self::extract_class_members_impl(class_node, package, imports, class_fqn, symbols, 0);
    }

    /// Internal implementation with depth limit to prevent infinite recursion
    fn extract_class_members_impl(
        class_node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        class_fqn: &str,
        symbols: &mut Vec<Symbol>,
        depth: u32,
    ) {
        // Prevent infinite recursion in case of malformed AST or deeply nested classes
        const MAX_NESTING_DEPTH: u32 = 20;
        if depth > MAX_NESTING_DEPTH {
            warn!(
                "Max class nesting depth ({}) exceeded for {}",
                MAX_NESTING_DEPTH, class_fqn
            );
            return;
        }

        // PERFORMANCE OPTIMIZATION: Use children() instead of descendants()
        // Members (fields, methods, nested classes) are DIRECT children of CLASS_DECL.
        // This avoids O(n²) traversal where n = nodes in class body.
        // For nested classes, we recursively call this function.
        for child in class_node.children() {
            match child.kind() {
                SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ANNOTATION_DECL => {
                    // Direct nested class/interface/enum - extract and recurse
                    let kind = match child.kind() {
                        SyntaxKind::CLASS_DECL => SymbolKind::CLASS,
                        SyntaxKind::INTERFACE_DECL => SymbolKind::INTERFACE,
                        SyntaxKind::ENUM_DECL => SymbolKind::ENUM,
                        SyntaxKind::RECORD_DECL => SymbolKind::STRUCT,
                        SyntaxKind::ANNOTATION_DECL => SymbolKind::INTERFACE,
                        _ => unreachable!(),
                    };

                    if let Some(nested_type) =
                        Self::extract_nested_type_decl(&child, package, imports, class_fqn, kind)
                    {
                        let nested_fqn = nested_type.fqn.clone();
                        let nested_range = nested_type.range;
                        symbols.push(nested_type);

                        // A nested record has the same implicit members as a
                        // top-level one (accessors, canonical constructor,
                        // equals/hashCode/toString); without them every
                        // `node.value()` on a nested record looks like a call
                        // to a missing method.
                        if child.kind() == SyntaxKind::RECORD_DECL {
                            if let Some(record_info) = crate::record_support::RecordInfo::from_node(
                                &child,
                                package,
                                imports,
                                &nested_fqn,
                            ) {
                                symbols.extend(crate::record_support::generate_record_methods(
                                    &record_info,
                                    &nested_fqn,
                                    nested_range,
                                ));
                            }
                        }

                        // Recursively extract members from the nested type
                        Self::extract_class_members_impl(
                            &child,
                            package,
                            imports,
                            &nested_fqn,
                            symbols,
                            depth + 1,
                        );
                    }
                }
                SyntaxKind::METHOD_DECL => {
                    if let Some(method) =
                        Self::extract_method(&child, package, imports, class_fqn, Some(class_fqn))
                    {
                        symbols.push(method);
                    }
                }
                SyntaxKind::CONSTRUCTOR_DECL => {
                    if let Some(constructor) =
                        Self::extract_constructor(&child, package, imports, class_fqn)
                    {
                        symbols.push(constructor);
                    }
                }
                SyntaxKind::FIELD_DECL => {
                    symbols.extend(Self::extract_fields(
                        &child,
                        package,
                        imports,
                        class_fqn,
                        Some(class_fqn),
                    ));
                }
                SyntaxKind::ENUM_CONSTANT => {
                    if let Some(constant) = Self::extract_enum_constant(&child, class_fqn) {
                        symbols.push(constant);
                    }
                }
                _ => {}
            }
        }

        // Extract Lombok-generated methods
        let lombok_symbols = Self::extract_lombok_methods(class_node, package, imports, class_fqn);
        symbols.extend(lombok_symbols);

        // Extract enum synthetic methods (values(), valueOf(), name(), ordinal())
        if class_node.kind() == SyntaxKind::ENUM_DECL {
            if let Some(enum_info) = crate::enum_support::EnumInfo::from_node(class_node) {
                let enum_range = TextRange::new(
                    class_node.text_range().start().into(),
                    class_node.text_range().end().into(),
                );
                let enum_methods =
                    crate::enum_support::generate_enum_methods(&enum_info, class_fqn, enum_range);
                symbols.extend(enum_methods);
            }
        }
    }

    /// Extract Lombok-generated synthetic methods
    fn extract_lombok_methods(
        class_node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        class_fqn: &str,
    ) -> Vec<Symbol> {
        use crate::lombok::{self, ClassInfo};

        // Extract class-level annotations
        let class_annotations = Self::extract_annotations_from_modifiers(class_node);

        // Extract class name from FQN
        let class_name = class_fqn.split('.').next_back().unwrap_or("").to_string();

        // Extract fields with their types and annotations
        // PERFORMANCE OPTIMIZATION: Use children() - fields are direct children of CLASS_DECL
        let mut fields = Vec::new();
        for child in class_node.children() {
            if child.kind() == SyntaxKind::FIELD_DECL {
                fields.extend(Self::extract_field_info(
                    &child, package, imports, class_fqn,
                ));
            }
        }

        let class_info = ClassInfo {
            name: class_name,
            package: class_fqn
                .rsplit_once('.')
                .map(|x| x.0)
                .map(|s| s.to_string()),
            fields,
            annotations: class_annotations,
        };

        let class_range = crate::TextRange::new(
            class_node.text_range().start().into(),
            class_node.text_range().end().into(),
        );

        // Process Lombok annotations and generate synthetic symbols
        lombok::process_class_annotations(&class_info, class_fqn, class_range)
    }

    /// Extract annotations from a class node's modifiers
    fn extract_annotations_from_modifiers(
        node: &SyntaxNode,
    ) -> Vec<crate::lombok::LombokAnnotation> {
        use crate::lombok::LombokAnnotation;

        let mut annotations = Vec::new();

        if let Some(modifiers) = crate::type_declaration_extractor::preceding_modifier_list(node) {
            for annotation_node in modifiers.children() {
                if annotation_node.kind() == SyntaxKind::ANNOTATION {
                    if let Some(lombok_annotation) =
                        LombokAnnotation::from_annotation_node(&annotation_node)
                    {
                        annotations.push(lombok_annotation);
                    }
                }
            }
        }

        // Also check direct children
        for child in node.children() {
            if child.kind() == SyntaxKind::ANNOTATION {
                if let Some(lombok_annotation) = LombokAnnotation::from_annotation_node(&child) {
                    annotations.push(lombok_annotation);
                }
            }
        }

        annotations
    }

    /// Extract field information for Lombok processing
    fn extract_field_info(
        node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        class_fqn: &str,
    ) -> Vec<crate::lombok::FieldInfo> {
        use crate::lombok::{FieldInfo, LombokAnnotation};

        // Reuse normal field extraction so synthetic accessors receive the same
        // qualified, generic-aware type metadata as source-declared members.
        let field_symbols =
            Self::extract_fields(node, package, imports, class_fqn, Some(class_fqn));

        // Extract field-level annotations
        let mut annotations = Vec::new();
        if let Some(modifiers) = crate::type_declaration_extractor::preceding_modifier_list(node) {
            for annotation_node in modifiers.children() {
                if annotation_node.kind() == SyntaxKind::ANNOTATION {
                    if let Some(lombok_annotation) =
                        LombokAnnotation::from_annotation_node(&annotation_node)
                    {
                        annotations.push(lombok_annotation);
                    }
                }
            }
        }

        field_symbols
            .into_iter()
            .map(|field_symbol| FieldInfo {
                name: field_symbol.simple_name.to_string(),
                type_name: field_symbol
                    .return_type
                    .as_deref()
                    .unwrap_or("java.lang.Object")
                    .to_string(),
                is_final: field_symbol.modifiers.is_final,
                is_static: field_symbol.modifiers.is_static,
                annotations: annotations.clone(),
                range: field_symbol.range,
            })
            .collect()
    }

    /// Extract a nested class or interface declaration
    fn extract_nested_type_decl(
        node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        enclosing_class_fqn: &str,
        kind: SymbolKind,
    ) -> Option<Symbol> {
        let mut name = None;
        let mut name_range: Option<rowan::TextRange> = None;
        let modifiers = crate::type_declaration_extractor::extract_declaration_modifiers(node);

        // Extract name from the node itself
        // Find the first IDENT token (which is the type name)
        for token in node.descendants_with_tokens() {
            if let NodeOrToken::Token(token) = token {
                if token.kind().is_type_identifier() && name.is_none() {
                    name = Some(token.text().to_string());
                    name_range = Some(token.text_range());
                }
            }
        }

        let simple_name = name?;
        let name_range = name_range?;

        // Build FQN: enclosing_class.nested_class (e.g., com.example.Outer.Inner)
        let fqn = format!("{}.{}", enclosing_class_fqn, simple_name);

        // Extract inheritance — `extends` is multi-parent for interfaces and
        // single-parent for classes/enums/records. See type_declaration_extractor
        // for the rationale.
        let lexical_types = Self::lexical_member_type_bindings(node, enclosing_class_fqn);
        let (parent_class, interfaces) = if matches!(kind, SymbolKind::INTERFACE) {
            let parents = crate::parent_interface_extractor::extract_interface_extends_in_context(
                node,
                package,
                imports,
                &lexical_types,
            );
            (None, parents)
        } else {
            let parent = crate::parent_interface_extractor::extract_parent_class_in_context(
                node,
                package,
                imports,
                &simple_name,
                &lexical_types,
            );
            let ifaces = crate::parent_interface_extractor::extract_interfaces_in_context(
                node,
                package,
                imports,
                &lexical_types,
            );
            (parent, ifaces)
        };
        let (parent_class, interfaces) =
            crate::type_declaration_extractor::complete_implicit_java_supertypes(
                node.kind(),
                parent_class,
                interfaces,
            );

        // Extract type parameters (e.g., <T, K, V>)
        let type_parameters = crate::type_declaration_extractor::extract_type_parameters(node);

        // Use the identifier's range, not the entire node range
        // This ensures goto-definition lands on the class name, not on the "class" keyword
        let range = TextRange::new(name_range.start().into(), name_range.end().into());

        Some(Symbol {
            fqn: fqn.into(),
            simple_name: simple_name.into(),
            range,
            kind,
            modifiers,
            container: Some(enclosing_class_fqn.to_string().into()),
            return_type: None,
            parent_class: parent_class.map(Arc::from),
            interfaces: interfaces.into_iter().map(Arc::from).collect(),
            documentation: crate::type_declaration_extractor::extract_declaration_documentation(
                node,
            ),
            parameters: vec![],
            type_parameters,
            is_deprecated: crate::type_declaration_extractor::is_deprecated_declaration(node),
            receiver_type: None, // Java doesn't have extension functions
            kotlin: None,
            throws: None,
        })
    }

    /// Extract a method declaration
    fn extract_method(
        node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        class_fqn: &str,
        parent_class_fqn: Option<&str>,
    ) -> Option<Symbol> {
        let mut identifiers = Vec::new();
        let mut modifiers = crate::type_declaration_extractor::extract_declaration_modifiers(node);
        let mut seen_lparen = false;

        let is_interface_member = node.parent().is_some_and(|owner| {
            matches!(
                owner.kind(),
                SyntaxKind::INTERFACE_DECL | SyntaxKind::ANNOTATION_DECL
            )
        });
        let has_body = node
            .children()
            .any(|child| child.kind() == SyntaxKind::BLOCK_STMT);
        if is_interface_member && !has_body && !modifiers.is_static && !modifiers.is_private {
            modifiers.is_public = true;
            modifiers.is_abstract = true;
        }

        // Collect identifiers until we see '('
        for token in node.descendants_with_tokens() {
            if let NodeOrToken::Token(token) = token {
                match token.kind() {
                    kind if kind.is_identifier() && !seen_lparen => {
                        identifiers.push(token.text().to_string());
                    }
                    SyntaxKind::L_PAREN => {
                        seen_lparen = true;
                    }
                    _ => {}
                }
            }
        }

        // Method name is the LAST identifier before '('
        let simple_name = identifiers.last()?.clone();

        // Extract parameters early so we can include them in the FQN
        let parameters = Self::extract_method_parameters(node, imports, package, parent_class_fqn);

        // Build FQN with parameter descriptor to distinguish overloads
        // Format: com.example.Class.methodName(String,Map,boolean)
        let param_descriptor = Self::build_param_descriptor(&parameters);
        let fqn = format!("{}.{}({})", class_fqn, simple_name, param_descriptor);

        // Extract return type from CLASS_TYPE, ARRAY_TYPE, or PRIMITIVE_TYPE node
        // Parser structure for arrays: CLASS_TYPE, L_BRACKET, ARRAY_TYPE (as siblings)
        // We need to find the base type and count array dimensions
        let return_type = {
            let children: Vec<_> = node.children_with_tokens().collect();
            let mut base_type: Option<String> = None;
            let mut dimensions = 0;
            let mut found_base = false;

            for item in children.iter() {
                match item {
                    NodeOrToken::Node(n) => {
                        if !found_base {
                            match n.kind() {
                                SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE => {
                                    base_type = Some(Self::resolve_type_from_node(
                                        n,
                                        imports,
                                        package,
                                        parent_class_fqn,
                                    ));
                                    found_base = true;
                                }
                                _ => {}
                            }
                        } else if n.kind() == SyntaxKind::ARRAY_TYPE {
                            // Each ARRAY_TYPE node represents a pair of brackets
                            dimensions += 1;
                        }
                    }
                    NodeOrToken::Token(_) => {}
                }
            }

            base_type.map(|ty| {
                if dimensions > 0 {
                    format!("{}{}", ty, "[]".repeat(dimensions))
                } else {
                    ty
                }
            })
        };

        // Find the range of the method name IDENT token specifically
        // This ensures navigation lands on the method name, not on generic type parameters
        let range = {
            let mut method_name_range = None;

            for token in node.descendants_with_tokens() {
                if let NodeOrToken::Token(token) = token {
                    if token.kind() == SyntaxKind::L_PAREN {
                        break;
                    } else if token.kind().is_identifier() && token.text() == simple_name {
                        // Found the method name IDENT token
                        method_name_range = Some(TextRange::new(
                            token.text_range().start().into(),
                            token.text_range().end().into(),
                        ));
                    }
                }
            }

            // Fall back to full node range if method name token not found
            method_name_range.unwrap_or_else(|| {
                TextRange::new(
                    node.text_range().start().into(),
                    node.text_range().end().into(),
                )
            })
        };

        Some(Symbol {
            fqn: fqn.into(),
            simple_name: simple_name.into(),
            range,
            kind: SymbolKind::METHOD,
            modifiers,
            container: Some(class_fqn.to_string().into()),
            return_type: return_type.map(Into::into),
            parent_class: None, // Only classes have parent classes
            interfaces: vec![],
            documentation: crate::type_declaration_extractor::extract_declaration_documentation(
                node,
            ),
            parameters: parameters.into_iter().map(Into::into).collect(),
            type_parameters: crate::type_declaration_extractor::extract_type_parameters(node),
            is_deprecated: crate::type_declaration_extractor::is_deprecated_declaration(node),
            receiver_type: None, // Java doesn't have extension functions
            kotlin: None,
            throws: Self::extract_throws(node),
        })
    }

    /// Extract a constructor declaration
    /// Constructors are similar to methods but:
    /// - Have the same name as the class (simple name, not FQN)
    /// - Don't have a return type
    /// - Are marked with SymbolKind::CONSTRUCTOR
    fn extract_constructor(
        node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        class_fqn: &str,
    ) -> Option<Symbol> {
        use rowan::NodeOrToken;

        let mut identifiers = Vec::new();
        let mut modifiers = Modifiers::default();
        let mut seen_lparen = false;

        // Extract modifiers and collect identifiers until we see '('
        for token in node.descendants_with_tokens() {
            if let NodeOrToken::Token(token) = token {
                match token.kind() {
                    SyntaxKind::PUBLIC_KW => modifiers.is_public = true,
                    SyntaxKind::PRIVATE_KW => modifiers.is_private = true,
                    SyntaxKind::PROTECTED_KW => modifiers.is_protected = true,
                    // Constructors can't be static/final/abstract, but parse anyway
                    kind if kind.is_identifier() && !seen_lparen => {
                        identifiers.push(token.text().to_string());
                    }
                    SyntaxKind::L_PAREN => {
                        seen_lparen = true;
                    }
                    _ => {}
                }
            }
        }

        // Constructor name is the LAST identifier before '('
        // It should match the class simple name
        let simple_name = identifiers.last()?.clone();

        // Extract parameters early so we can include them in the FQN
        let parameters = Self::extract_method_parameters(node, imports, package, Some(class_fqn));

        // Build FQN with parameter descriptor to distinguish constructor overloads
        // Format: com.example.Class.ClassName(String,int)
        let param_descriptor = Self::build_param_descriptor(&parameters);
        let fqn = format!("{}.{}({})", class_fqn, simple_name, param_descriptor);

        // Find the range of the constructor name IDENT token
        let range = {
            let mut ctor_name_range = None;

            for token in node.descendants_with_tokens() {
                if let NodeOrToken::Token(token) = token {
                    if token.kind() == SyntaxKind::L_PAREN {
                        break;
                    } else if token.kind().is_identifier() && token.text() == simple_name {
                        ctor_name_range = Some(TextRange::new(
                            token.text_range().start().into(),
                            token.text_range().end().into(),
                        ));
                    }
                }
            }

            ctor_name_range.unwrap_or_else(|| {
                TextRange::new(
                    node.text_range().start().into(),
                    node.text_range().end().into(),
                )
            })
        };

        // Constructors return the class type (for signature display purposes)
        let return_type = Some(class_fqn.to_string());

        Some(Symbol {
            fqn: fqn.into(),
            simple_name: simple_name.into(),
            range,
            kind: SymbolKind::CONSTRUCTOR,
            modifiers,
            container: Some(class_fqn.to_string().into()),
            return_type: return_type.map(Into::into),
            parent_class: None,
            interfaces: vec![],
            documentation: crate::type_declaration_extractor::extract_declaration_documentation(
                node,
            ),
            parameters: parameters.into_iter().map(Into::into).collect(),
            type_parameters: crate::type_declaration_extractor::extract_type_parameters(node),
            is_deprecated: crate::type_declaration_extractor::is_deprecated_declaration(node),
            receiver_type: None, // Java doesn't have extension functions
            kotlin: None,
            throws: Self::extract_throws(node),
        })
    }

    /// Exception types of a method's or constructor's `throws` clause, as
    /// written (`IOException`, `java.io.IOException`, `Outer.Failure`).
    ///
    /// Extraction runs before the rest of the workspace is indexed, so a bare
    /// name imported on demand (`import java.io.*;`) cannot be resolved here;
    /// the written spelling is kept and resolved against the declaring file's
    /// imports when a check needs it.
    fn extract_throws(node: &SyntaxNode) -> crate::Throws {
        let names: Vec<std::sync::Arc<str>> = node
            .children()
            .filter(|child| child.kind() == SyntaxKind::THROWS_CLAUSE)
            .flat_map(|clause| clause.children().collect::<Vec<_>>())
            .filter(|child| child.kind() == SyntaxKind::CLASS_TYPE)
            .map(|class_type| {
                let spelled: String = class_type
                    .descendants_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .filter(|token| !token.kind().is_trivia())
                    .map(|token| token.text().to_owned())
                    .collect();
                std::sync::Arc::<str>::from(spelled.split('<').next().unwrap_or(&spelled))
            })
            .collect();
        (!names.is_empty()).then(|| std::sync::Arc::new(names))
    }

    /// Extract every variable introduced by a field declaration.
    fn extract_fields(
        node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        class_fqn: &str,
        parent_class_fqn: Option<&str>,
    ) -> Vec<Symbol> {
        let modifiers = crate::type_declaration_extractor::extract_declaration_modifiers(node);
        let mut field_type = node
            .children()
            .find(|child| {
                matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE
                )
            })
            .map(|child| Self::resolve_type_from_node(&child, imports, package, parent_class_fqn));
        let dimensions = node
            .children()
            .filter(|child| child.kind() == SyntaxKind::ARRAY_TYPE)
            .count();
        if let Some(ty) = &mut field_type {
            ty.push_str(&"[]".repeat(dimensions));
        }

        hyperion_types::field_declarators(node)
            .into_iter()
            .map(|declarator| {
                let simple_name = declarator.name;
                let field_name_range = declarator.name_range;
                let field_type = field_type
                    .as_ref()
                    .map(|ty| format!("{ty}{}", "[]".repeat(declarator.extra_array_dimensions)));
                let fqn = format!("{}.{}", class_fqn, simple_name);

                // Use the field name token's range, not the entire FIELD_DECL node
                let range = TextRange::new(
                    field_name_range.start().into(),
                    field_name_range.end().into(),
                );

                Symbol {
                    fqn: fqn.into(),
                    simple_name: simple_name.into(),
                    range,
                    kind: SymbolKind::FIELD,
                    modifiers,
                    container: Some(class_fqn.to_string().into()),
                    return_type: field_type.map(Into::into), // For fields, this is the field type (e.g., "String", "User", "Profile")
                    parent_class: None,                      // Only classes have parent classes
                    interfaces: vec![],
                    documentation:
                        crate::type_declaration_extractor::extract_declaration_documentation(node),
                    parameters: vec![],
                    type_parameters: Vec::new(),
                    is_deprecated: crate::type_declaration_extractor::is_deprecated_declaration(
                        node,
                    ),
                    receiver_type: None, // Java doesn't have extension functions
                    kotlin: None,
                    throws: None,
                }
            })
            .collect()
    }

    /// Extract an enum constant (e.g., RED, GREEN, BLUE)
    fn extract_enum_constant(node: &SyntaxNode, enum_fqn: &str) -> Option<Symbol> {
        // Find the constant name - it's the first IDENT that's NOT inside a MODIFIER_LIST
        // This skips annotation names like "Deprecated" from @Deprecated
        let constant_token = node
            .children_with_tokens()
            .filter(|child| {
                // Skip MODIFIER_LIST nodes (which contain annotations)
                if let Some(node) = child.as_node() {
                    node.kind() != SyntaxKind::MODIFIER_LIST
                } else {
                    true
                }
            })
            .filter_map(|it| it.into_token())
            .find(|token| token.kind().is_identifier())?;

        let constant_name = constant_token.text().to_string();
        let constant_range = constant_token.text_range();

        let fqn = format!("{}.{}", enum_fqn, constant_name);

        // Use the identifier's range, not the entire node range
        // This ensures goto-definition lands on the constant name
        let range = TextRange::new(constant_range.start().into(), constant_range.end().into());

        // Enum constants are public static final fields
        let modifiers = Modifiers {
            is_public: true,
            is_static: true,
            is_final: true,
            ..Default::default()
        };

        // Extract simple enum name from FQN for return type
        let enum_simple_name = enum_fqn.split('.').next_back().unwrap_or("");

        Some(Symbol {
            fqn: fqn.into(),
            simple_name: constant_name.into(),
            range,
            kind: SymbolKind::ENUM_MEMBER,
            modifiers,
            container: Some(enum_fqn.to_string().into()),
            return_type: Some(enum_simple_name.to_string().into()), // Enum constant's type is the enum itself
            parent_class: None, // Only classes have parent classes
            interfaces: vec![],
            documentation: crate::type_declaration_extractor::extract_declaration_documentation(
                node,
            ),
            parameters: vec![],
            type_parameters: Vec::new(),
            is_deprecated: crate::type_declaration_extractor::is_deprecated_declaration(node),
            receiver_type: None, // Java doesn't have extension functions
            kotlin: None,
            throws: None,
        })
    }
}

/// Result of extracting symbols from a file
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub package: Option<String>,
    pub imports: ImportMap,
    pub symbols: Vec<Symbol>,
    /// Explicit generic arguments on direct supertype edges, stored only for
    /// nominal declarations that actually write them.
    pub generic_supertypes: Vec<crate::TypeGenericSupertypes>,
    /// Extraction errors for malformed or problematic top-level declarations
    ///
    /// When symbol extraction encounters a panic or error for a specific
    /// top-level type (class/interface/enum), we recover and continue extracting
    /// other symbols. This provides partial symbol tables for files with syntax
    /// errors, improving the user experience (some completions/goto-def work even
    /// in partially broken files).
    pub errors: Vec<SymbolExtractionError>,
}

/// Error encountered during symbol extraction
#[derive(Debug, Clone)]
pub struct SymbolExtractionError {
    /// Text range where the error occurred (if available)
    pub range: Option<TextRange>,
    /// Error message
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::debug;

    #[test]
    fn test_extract_package() {
        let source = "package com.example.test;\n\nclass Foo {}";
        let result = SymbolExtractor::extract(source).unwrap();
        assert_eq!(result.package.as_deref(), Some("com.example.test"));
    }

    #[test]
    fn test_extract_no_package() {
        let source = "class Foo {}";
        let result = SymbolExtractor::extract(source).unwrap();
        assert_eq!(result.package, None);
    }

    #[test]
    fn contextual_keywords_remain_identifiers_outside_their_grammar_context() {
        let source = r#"
            package contextual.open;
            import static helpers.Factory.open;

            class Example {
                int module;
                void open() {}
                void record() {}
            }

            enum State { open }
        "#;
        let result = SymbolExtractor::extract(source).expect("valid Java source");

        assert_eq!(result.package.as_deref(), Some("contextual.open"));
        assert_eq!(
            result
                .imports
                .static_imports_named("open")
                .collect::<Vec<_>>(),
            ["helpers.Factory.open"]
        );
        for (name, kind) in [
            ("module", SymbolKind::FIELD),
            ("open", SymbolKind::METHOD),
            ("record", SymbolKind::METHOD),
            ("open", SymbolKind::ENUM_MEMBER),
        ] {
            assert!(
                result
                    .symbols
                    .iter()
                    .any(|symbol| symbol.simple_name.as_ref() == name && symbol.kind == kind),
                "missing {kind:?} named {name}; extracted: {:?}",
                result
                    .symbols
                    .iter()
                    .map(|symbol| (symbol.kind, symbol.simple_name.as_ref()))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_extract_class() {
        let source = "package com.example;\n\npublic final class MyClass {}";
        let result = SymbolExtractor::extract(source).unwrap();

        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.fqn.as_ref(), "com.example.MyClass");
        assert_eq!(sym.simple_name.as_ref(), "MyClass");
        assert_eq!(sym.kind, SymbolKind::CLASS);
        assert!(sym.modifiers.is_public);
        assert!(sym.modifiers.is_final);
    }

    #[test]
    fn test_extract_interface() {
        let source = "package com.example;\n\npublic interface MyInterface {}";
        let result = SymbolExtractor::extract(source).unwrap();

        assert_eq!(result.symbols.len(), 1);
        let sym = &result.symbols[0];
        assert_eq!(sym.fqn.as_ref(), "com.example.MyInterface");
        assert_eq!(sym.kind, SymbolKind::INTERFACE);
        assert!(sym.modifiers.is_public);
    }

    #[test]
    fn test_type_modifiers_do_not_leak_between_declarations() {
        let source = r#"
public class PublicType {}
final class FinalType {}
interface PackageInterface {}
"#;
        let result = SymbolExtractor::extract(source).unwrap();

        let public_type = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "PublicType")
            .expect("PublicType symbol");
        assert!(public_type.modifiers.is_public);
        assert!(!public_type.modifiers.is_final);

        let final_type = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "FinalType")
            .expect("FinalType symbol");
        assert!(!final_type.modifiers.is_public);
        assert!(final_type.modifiers.is_final);

        let package_interface = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "PackageInterface")
            .expect("PackageInterface symbol");
        assert!(!package_interface.modifiers.is_public);
        assert!(!package_interface.modifiers.is_final);
    }

    #[test]
    fn test_nested_type_modifiers_are_extracted_without_leaking() {
        let source = r#"
class Outer {
    public static class Visible {}
    final class FinalNested {}
    interface PackageContract {}
}
"#;
        let result = SymbolExtractor::extract(source).unwrap();

        let visible = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "Visible")
            .expect("Visible symbol");
        assert!(visible.modifiers.is_public);
        assert!(visible.modifiers.is_static);
        assert!(!visible.modifiers.is_final);

        let final_nested = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "FinalNested")
            .expect("FinalNested symbol");
        assert!(!final_nested.modifiers.is_public);
        assert!(!final_nested.modifiers.is_static);
        assert!(final_nested.modifiers.is_final);

        let package_contract = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "PackageContract")
            .expect("PackageContract symbol");
        assert!(!package_contract.modifiers.is_public);
        assert!(!package_contract.modifiers.is_static);
        assert!(!package_contract.modifiers.is_final);
    }

    #[test]
    fn test_extract_generic_interface_implementation() {
        // Test that interface names are extracted without generic type parameters
        let source = r#"
package com.example;

interface MyInterface<T> {
    T getValue();
}

class MyImplementation<T> implements MyInterface<T> {
    private T value;
    public T getValue() { return value; }
}
"#;
        let result = SymbolExtractor::extract(source).unwrap();

        // Find the MyImplementation class
        let impl_class = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "MyImplementation" && s.kind == SymbolKind::CLASS)
            .expect("Should find MyImplementation class");

        // Preserve the written spelling without generic type parameters; the
        // workspace resolver canonicalizes it lazily in file context.
        assert_eq!(impl_class.interfaces.len(), 1);
        assert_eq!(impl_class.interfaces[0].as_ref(), "MyInterface");
    }

    #[test]
    fn test_extract_multiple_generic_interfaces() {
        // Test extraction of multiple interfaces with generics
        // Resolution is intentionally deferred; extraction retains spelling.
        let source = r#"
package com.example;

class MyClass implements List<String>, Comparable<MyClass>, Serializable {
}
"#;
        let result = SymbolExtractor::extract(source).unwrap();

        let my_class = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "MyClass" && s.kind == SymbolKind::CLASS)
            .expect("Should find MyClass");

        // All interfaces should be extracted without type parameters
        assert_eq!(my_class.interfaces.len(), 3);
        assert!(my_class.interfaces.contains(&"List".into()));
        assert!(my_class.interfaces.contains(&"Comparable".into()));
        assert!(my_class.interfaces.contains(&"Serializable".into()));
    }

    #[test]
    fn test_extract_enum() {
        let source = "package com.example;\n\nenum Status { ACTIVE, INACTIVE }";
        let result = SymbolExtractor::extract(source).unwrap();

        // Should extract enum + enum constants
        let enum_sym = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "Status" && s.kind == SymbolKind::ENUM)
            .expect("Should find Status enum");
        assert_eq!(enum_sym.fqn.as_ref(), "com.example.Status");
        assert_eq!(enum_sym.kind, SymbolKind::ENUM);

        // Verify enum constants are also extracted
        assert!(result
            .symbols
            .iter()
            .any(|s| s.simple_name.as_ref() == "ACTIVE" && s.kind == SymbolKind::ENUM_MEMBER));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.simple_name.as_ref() == "INACTIVE" && s.kind == SymbolKind::ENUM_MEMBER));
    }

    #[test]
    fn enum_constructor_arguments_do_not_hide_members() {
        let source = r#"
package com.example;

enum Color {
    RED(255), GREEN(0);

    private final int value;
    Color(int value) { this.value = value; }
    int getValue() { return value; }
}
"#;
        let result = SymbolExtractor::extract(source).unwrap();

        let red = result
            .symbols
            .iter()
            .find(|symbol| {
                symbol.fqn.as_ref() == "com.example.Color.RED"
                    && symbol.kind == SymbolKind::ENUM_MEMBER
            })
            .expect("RED enum member");
        assert_eq!(red.return_type.as_deref(), Some("Color"));
        assert!(
            result.symbols.iter().any(|symbol| {
                symbol.fqn.as_ref() == "com.example.Color.getValue()"
                    && symbol.kind == SymbolKind::METHOD
            }),
            "extracted symbols: {:?}",
            result
                .symbols
                .iter()
                .map(|symbol| (symbol.fqn.as_ref(), symbol.kind))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_extract_multiple_types() {
        let source = r#"
package com.example;

public class Foo {}

interface Bar {}

enum Baz { A, B }
"#;
        let result = SymbolExtractor::extract(source).unwrap();

        // Should extract 3 types + 2 enum constants
        assert!(result
            .symbols
            .iter()
            .any(|s| s.simple_name.as_ref() == "Foo" && s.kind == SymbolKind::CLASS));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.simple_name.as_ref() == "Bar" && s.kind == SymbolKind::INTERFACE));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.simple_name.as_ref() == "Baz" && s.kind == SymbolKind::ENUM));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.simple_name.as_ref() == "A" && s.kind == SymbolKind::ENUM_MEMBER));
        assert!(result
            .symbols
            .iter()
            .any(|s| s.simple_name.as_ref() == "B" && s.kind == SymbolKind::ENUM_MEMBER));
    }

    #[test]
    fn test_extract_methods() {
        let source = r#"
package com.example;

class Calculator {
    public int add(int a, int b) {
        return a + b;
    }

    private double multiply(double x, double y) {
        return x * y;
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        // Debug: print all symbols
        debug!("=== Extracted Symbols ===");
        for sym in &result.symbols {
            debug!(
                "  {:?}: {} (container: {:?})",
                sym.kind, sym.simple_name, sym.container
            );
        }

        // Should have 1 class + 2 methods = 3 symbols
        assert!(
            result.symbols.len() >= 3,
            "Expected at least 3 symbols (class + 2 methods), got {}",
            result.symbols.len()
        );

        let class_sym = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::CLASS)
            .expect("Class not found");
        assert_eq!(class_sym.fqn.as_ref(), "com.example.Calculator");

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::METHOD)
            .collect();
        assert!(
            methods.len() >= 2,
            "Expected at least 2 methods, got {}",
            methods.len()
        );

        // Check method names - should be "add" and "multiply", not "int" and "double"
        let method_names: Vec<&str> = methods.iter().map(|m| m.simple_name.as_ref()).collect();
        debug!("Method names: {:?}", method_names);
    }

    #[test]
    fn field_declarators_keep_individual_types_ranges_and_metadata() {
        let source = r"package p;
            public class Fields {
                @Deprecated public static final int FIRST = sum(1, 2), SECOND = 2;
                public String[] shared = {}, matrix[][] = {{{}}};
                @lombok.Getter private int left, right;
                static int sum(int a, int b) { return a + b; }
            }";
        let result = SymbolExtractor::extract(source).unwrap();
        let fields: Vec<_> = result
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::FIELD)
            .collect();
        assert_eq!(fields.len(), 6);
        for (name, ty) in [
            ("FIRST", "int"),
            ("SECOND", "int"),
            ("shared", "java.lang.String[]"),
            ("matrix", "java.lang.String[][][]"),
            ("left", "int"),
            ("right", "int"),
        ] {
            let field = fields
                .iter()
                .find(|symbol| symbol.simple_name.as_ref() == name)
                .unwrap();
            assert_eq!(field.return_type.as_deref(), Some(ty), "{name}");
            assert_eq!(
                &source[field.range.start as usize..field.range.end as usize],
                name
            );
            if matches!(name, "FIRST" | "SECOND") {
                assert!(
                    field.modifiers.is_public
                        && field.modifiers.is_static
                        && field.modifiers.is_final
                );
                assert!(field.is_deprecated);
            }
        }
        for name in ["getLeft", "getRight"] {
            assert!(
                result
                    .symbols
                    .iter()
                    .any(|symbol| symbol.simple_name.as_ref() == name),
                "missing {name}"
            );
        }
    }

    #[test]
    fn test_extract_fields() {
        let source = r#"
package com.example;

class Person {
    private String name;
    public int age;
    protected boolean active;
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        // Debug: print all symbols
        debug!("=== Extracted Symbols ===");
        for sym in &result.symbols {
            debug!(
                "  {:?}: {} (container: {:?})",
                sym.kind, sym.simple_name, sym.container
            );
        }

        // Should have 1 class + 3 fields = 4 symbols
        assert!(
            result.symbols.len() >= 4,
            "Expected at least 4 symbols (class + 3 fields), got {}",
            result.symbols.len()
        );

        let class_sym = result
            .symbols
            .iter()
            .find(|s| s.kind == SymbolKind::CLASS)
            .expect("Class not found");
        assert_eq!(class_sym.fqn.as_ref(), "com.example.Person");

        let fields: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::FIELD)
            .collect();
        assert!(
            fields.len() >= 3,
            "Expected at least 3 fields, got {}",
            fields.len()
        );

        // Check field names - should be "name", "age", "active", not "String", "int", "boolean"
        let field_names: Vec<&str> = fields.iter().map(|f| f.simple_name.as_ref()).collect();
        debug!("Field names: {:?}", field_names);
        assert!(field_names.contains(&"name"), "Should have 'name' field");
        assert!(field_names.contains(&"age"), "Should have 'age' field");
        assert!(
            field_names.contains(&"active"),
            "Should have 'active' field"
        );

        // Check that container is set correctly for fields
        for field in &fields {
            assert_eq!(
                field.container.as_ref().unwrap().as_ref(),
                "com.example.Person"
            );
        }
    }

    #[test]
    fn lombok_getters_preserve_qualified_field_types_and_annotation_scope() {
        let source = r#"
package com.example;

import com.model.Profile;
import lombok.Data;
import lombok.Getter;

@Data
class Fixture {
    private Profile profile;
}

class Holder {
    @Getter
    private Fixture fixture;
    private String unannotated;
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();
        let getter = |name: &str| {
            result
                .symbols
                .iter()
                .find(|symbol| symbol.kind == SymbolKind::METHOD && symbol.fqn.as_ref() == name)
                .unwrap_or_else(|| panic!("missing synthetic getter {name}"))
        };

        assert_eq!(
            getter("com.example.Fixture.getProfile()")
                .return_type
                .as_deref(),
            Some("com.model.Profile")
        );
        assert_eq!(
            getter("com.example.Holder.getFixture()")
                .return_type
                .as_deref(),
            Some("com.example.Fixture")
        );
        assert!(result
            .symbols
            .iter()
            .all(|symbol| symbol.fqn.as_ref() != "com.example.Holder.getUnannotated()"));
    }

    #[test]
    fn lombok_logger_annotations_generate_a_log_field() {
        for (annotation, import, logger) in [
            ("Slf4j", "lombok.extern.slf4j.Slf4j", "org.slf4j.Logger"),
            (
                "Log4j2",
                "lombok.extern.log4j.Log4j2",
                "org.apache.logging.log4j.Logger",
            ),
            (
                "CommonsLog",
                "lombok.extern.apachecommons.CommonsLog",
                "org.apache.commons.logging.Log",
            ),
            ("Log", "lombok.extern.java.Log", "java.util.logging.Logger"),
        ] {
            let source = format!(
                "package com.example;\nimport {import};\n@{annotation}\nclass Service {{ void run() {{ log.info(\"x\"); }} }}\nclass Plain {{}}\n"
            );
            let result = SymbolExtractor::extract(&source).unwrap();
            let log = result
                .symbols
                .iter()
                .find(|symbol| symbol.fqn.as_ref() == "com.example.Service.log")
                .unwrap_or_else(|| panic!("@{annotation} must generate a log field"));
            assert_eq!(log.kind, SymbolKind::FIELD);
            assert_eq!(log.return_type.as_deref(), Some(logger));
            assert!(log.modifiers.is_static && log.modifiers.is_final);
            assert!(result
                .symbols
                .iter()
                .all(|symbol| symbol.fqn.as_ref() != "com.example.Plain.log"));
        }
    }

    #[test]
    fn methods_bind_return_types_to_directly_declared_nested_classes() {
        let source = r#"
package com.example;

public class Order {
    public static Builder builder() { return new Builder(); }

    public static class Builder {
        public Builder lineItem(String name, int price) { return this; }
        public Order build() { return new Order(); }
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();
        let method = |fqn: &str| {
            result
                .symbols
                .iter()
                .find(|symbol| symbol.kind == SymbolKind::METHOD && symbol.fqn.as_ref() == fqn)
                .unwrap_or_else(|| panic!("missing method {fqn}"))
        };

        assert_eq!(
            method("com.example.Order.builder()").return_type.as_deref(),
            Some("com.example.Order.Builder")
        );
        assert_eq!(
            method("com.example.Order.Builder.lineItem(java.lang.String,int)")
                .return_type
                .as_deref(),
            Some("com.example.Order.Builder")
        );
        assert_eq!(
            method("com.example.Order.Builder.build()")
                .return_type
                .as_deref(),
            Some("com.example.Order")
        );
    }

    #[test]
    fn same_file_type_shadows_implicit_java_lang_return_type() {
        let result = SymbolExtractor::extract(concat!(
            "package test; class StringBuilder {} ",
            "class Factory { StringBuilder create() { return null; } }"
        ))
        .expect("same-file shadow fixture should extract");
        let method = result
            .symbols
            .iter()
            .find(|symbol| symbol.fqn.as_ref() == "test.Factory.create()")
            .expect("create method");

        assert_eq!(method.return_type.as_deref(), Some("test.StringBuilder"));
    }

    #[test]
    fn methods_and_constructors_record_their_throws_clause_as_written() {
        let source = r#"
package com.example;
import java.io.*;

public class Store {
    public Store() throws IOException, com.acme.Failure {}
    public void save(String path) throws FileNotFoundException {}
    public <E extends Exception> void generic() throws E {}
    public void quiet() {}
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();
        let throws = |name: &str| -> Vec<String> {
            result
                .symbols
                .iter()
                .find(|symbol| {
                    matches!(symbol.kind, SymbolKind::METHOD | SymbolKind::CONSTRUCTOR)
                        && symbol.simple_name.as_ref() == name
                })
                .map(|symbol| symbol.throws().iter().map(|t| t.to_string()).collect())
                .unwrap_or_default()
        };

        assert_eq!(throws("Store"), ["IOException", "com.acme.Failure"]);
        assert_eq!(throws("save"), ["FileNotFoundException"]);
        assert_eq!(throws("generic"), ["E"]);
        assert!(throws("quiet").is_empty());
    }

    #[test]
    fn nested_records_get_their_implicit_members() {
        let source = r#"
package com.example;

public class Outer {
    record Point(int x, java.util.List<String> tags) {}
    static class Deep { record Leaf(String name) {} }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();
        let has_method = |fqn: &str| {
            result
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::METHOD && symbol.fqn.as_ref() == fqn)
        };

        assert!(has_method("com.example.Outer.Point.x()"));
        assert!(has_method("com.example.Outer.Point.tags()"));
        assert!(has_method(
            "com.example.Outer.Point.equals(java.lang.Object)"
        ));
        assert!(has_method("com.example.Outer.Deep.Leaf.name()"));
        let accessors = result
            .symbols
            .iter()
            .filter(|symbol| symbol.fqn.as_ref() == "com.example.Outer.Point.x()")
            .count();
        assert_eq!(accessors, 1, "accessors must not be generated twice");
    }

    #[test]
    fn record_accessors_preserve_qualified_component_types_and_signatures() {
        let source = r#"
package com.example;

public record User(String email) {}
public record Profile(User user, java.util.List<User> friends) {}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();
        let method = |fqn: &str| {
            result
                .symbols
                .iter()
                .find(|symbol| symbol.kind == SymbolKind::METHOD && symbol.fqn.as_ref() == fqn)
                .unwrap_or_else(|| panic!("missing record method {fqn}"))
        };

        assert_eq!(
            method("com.example.User.email()").return_type.as_deref(),
            Some("java.lang.String")
        );
        assert_eq!(
            method("com.example.Profile.user()").return_type.as_deref(),
            Some("com.example.User")
        );
        assert_eq!(
            method("com.example.Profile.friends()")
                .return_type
                .as_deref(),
            Some("java.util.List<com.example.User>")
        );
        assert_eq!(
            method("com.example.User.equals(java.lang.Object)")
                .parameters
                .first()
                .map(AsRef::as_ref),
            Some("java.lang.Object other")
        );
    }

    #[test]
    fn test_extract_methods_and_fields() {
        let source = r#"
package com.example;

class User {
    private String email;
    private int id;

    public String getEmail() {
        return email;
    }

    public void setEmail(String email) {
        this.email = email;
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        // Should have: 1 class + 2 fields + 2 methods = 5 symbols
        assert!(
            result.symbols.len() >= 5,
            "Expected at least 5 symbols, got {}",
            result.symbols.len()
        );

        let class_count = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::CLASS)
            .count();
        let field_count = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::FIELD)
            .count();
        let method_count = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::METHOD)
            .count();

        assert_eq!(class_count, 1, "Expected 1 class");
        assert!(
            field_count >= 2,
            "Expected at least 2 fields, got {}",
            field_count
        );
        assert!(
            method_count >= 2,
            "Expected at least 2 methods, got {}",
            method_count
        );
    }
}

#[cfg(test)]
mod advanced_tests {
    use super::*;
    use tracing::debug;

    #[test]
    fn test_methods_with_non_primitive_return_types() {
        let source = r#"
package com.example;

class Service {
    public String getName() {
        return "test";
    }

    private List<String> getItems() {
        return null;
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        debug!("=== Extracted Symbols ===");
        for sym in &result.symbols {
            debug!("  {:?}: {} (FQN: {})", sym.kind, sym.simple_name, sym.fqn);
        }

        let methods: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::METHOD)
            .collect();
        assert!(
            methods.len() >= 2,
            "Expected at least 2 methods, got {}",
            methods.len()
        );

        let method_names: Vec<&str> = methods.iter().map(|m| m.simple_name.as_ref()).collect();
        assert!(
            method_names.contains(&"getName"),
            "Should have 'getName' method, got: {:?}",
            method_names
        );
        assert!(
            method_names.contains(&"getItems"),
            "Should have 'getItems' method, got: {:?}",
            method_names
        );
    }

    #[test]
    fn test_fields_with_generic_types() {
        let source = r#"
package com.example;

class Container {
    private List<String> items;
    private Map<String, Integer> counts;
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        debug!("=== Extracted Symbols ===");
        for sym in &result.symbols {
            debug!("  {:?}: {} (FQN: {})", sym.kind, sym.simple_name, sym.fqn);
        }

        let fields: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::FIELD)
            .collect();
        assert!(
            fields.len() >= 2,
            "Expected at least 2 fields, got {}",
            fields.len()
        );

        let field_names: Vec<&str> = fields.iter().map(|f| f.simple_name.as_ref()).collect();
        assert!(
            field_names.contains(&"items"),
            "Should have 'items' field, got: {:?}",
            field_names
        );
        assert!(
            field_names.contains(&"counts"),
            "Should have 'counts' field, got: {:?}",
            field_names
        );
    }

    #[test]
    fn test_array_field_preserves_brackets() {
        let source = r#"
package com.example;

class Main {
    private User[] users;
    private String[] names;
    private int[][] matrix;
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        let fields: Vec<_> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::FIELD)
            .collect();

        assert_eq!(fields.len(), 3, "Should have 3 array fields");

        // Debug: print all field types
        for field in &fields {
            eprintln!(
                "Field {}: return_type = {:?}",
                field.simple_name, field.return_type
            );
        }

        // Check that array brackets are preserved in return_type
        // Note: Types are resolved to FQN (User -> com.example.User, String -> java.lang.String)
        let users_field = fields
            .iter()
            .find(|f| f.simple_name.as_ref() == "users")
            .unwrap();
        assert_eq!(
            users_field.return_type.as_deref(),
            Some("com.example.User[]"),
            "User array field should preserve [] with FQN, got: {:?}",
            users_field.return_type
        );

        let names_field = fields
            .iter()
            .find(|f| f.simple_name.as_ref() == "names")
            .unwrap();
        assert_eq!(
            names_field.return_type.as_deref(),
            Some("java.lang.String[]"),
            "String array field should preserve [] with FQN, got: {:?}",
            names_field.return_type
        );

        let matrix_field = fields
            .iter()
            .find(|f| f.simple_name.as_ref() == "matrix")
            .unwrap();
        assert_eq!(
            matrix_field.return_type.as_deref(),
            Some("int[][]"),
            "2D primitive array field should preserve [][], got: {:?}",
            matrix_field.return_type
        );
    }

    #[test]
    fn test_resolve_imported_return_type() {
        let source = r#"
package com.example;
import com.example.model.User;

public class Main {
    private User getUser() {
        return new User();
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        // Find the getUser method
        let get_user = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "getUser")
            .expect("getUser method not found");

        debug!("getUser return_type: {:?}", get_user.return_type);

        // Should be "com.example.model.User", not just "User"
        assert_eq!(
            get_user.return_type,
            Some("com.example.model.User".into()),
            "Return type should be fully qualified"
        );
    }

    #[test]
    fn test_resolve_array_types_with_qualified_names() {
        let source = r#"
package com.example;
import com.example.model.User;

public class Main {
    private User[] getUsers() {
        return new User[0];
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        let get_users = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "getUsers")
            .expect("getUsers method not found");

        debug!("getUsers return_type: {:?}", get_users.return_type);

        // Should be "com.example.model.User[]", not "User[]"
        assert_eq!(
            get_users.return_type,
            Some("com.example.model.User[]".into()),
            "Array return type should have FQN for element type"
        );
    }

    #[test]
    fn test_resolve_generic_types_with_qualified_parameters() {
        let source = r#"
package com.example;
import com.example.model.User;
import java.util.List;

public class Main {
    private List<User> getUsers() {
        return null;
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        let get_users = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "getUsers")
            .expect("getUsers method not found");

        debug!("getUsers return_type: {:?}", get_users.return_type);

        // Should be "java.util.List<com.example.model.User>", not "java.util.List<User>"
        // The outer type (List) should be resolved AND the type parameter (User) should be resolved
        assert_eq!(
            get_users.return_type,
            Some("java.util.List<com.example.model.User>".into()),
            "Generic return type should have FQN for both container and type parameters"
        );
    }

    #[test]
    fn test_resolve_nested_generics() {
        let source = r#"
package com.example;
import com.example.model.User;
import java.util.Map;

public class Main {
    private Map<String, User> getUserMap() {
        return null;
    }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();

        let get_user_map = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "getUserMap")
            .expect("getUserMap method not found");

        debug!("getUserMap return_type: {:?}", get_user_map.return_type);

        // Should resolve all types to FQNs (Map, String, and User)
        assert_eq!(
            get_user_map.return_type,
            Some("java.util.Map<java.lang.String, com.example.model.User>".into()),
            "Nested generic return type should resolve all type parameters to FQNs"
        );
    }

    #[test]
    fn imported_single_letter_class_is_not_indexed_as_a_type_parameter() {
        let source = r#"
package com.example;
import com.model.S;

class Main {
    S value;
    S get() { return value; }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();
        let value = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "value")
            .expect("value field should be indexed");
        let get = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "get")
            .expect("get method should be indexed");

        assert_eq!(value.return_type.as_deref(), Some("com.model.S"));
        assert_eq!(get.return_type.as_deref(), Some("com.model.S"));
    }

    #[test]
    fn declared_multi_letter_type_parameters_survive_indexed_signatures() {
        let source = r#"
package com.example;

class Box<Element> {
    <Input> Box(Input input) {}
    Element value;
    Element get(Element fallback) { return value; }
    <Result> Result map(Result input) { return input; }
}
        "#;

        let result = SymbolExtractor::extract(source).unwrap();
        let value = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "value")
            .expect("value field should be indexed");
        let get = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "get")
            .expect("get method should be indexed");
        let map = result
            .symbols
            .iter()
            .find(|symbol| symbol.simple_name.as_ref() == "map")
            .expect("map method should be indexed");
        let constructor = result
            .symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::CONSTRUCTOR)
            .expect("generic constructor should be indexed");

        assert_eq!(value.return_type.as_deref(), Some("Element"));
        assert_eq!(get.return_type.as_deref(), Some("Element"));
        assert_eq!(
            get.parameters.first().map(AsRef::as_ref),
            Some("Element fallback")
        );
        assert_eq!(map.return_type.as_deref(), Some("Result"));
        assert_eq!(
            map.parameters.first().map(AsRef::as_ref),
            Some("Result input")
        );
        assert_eq!(map.type_parameters.len(), 1);
        assert_eq!(map.type_parameters[0].name.as_ref(), "Result");
        assert_eq!(constructor.type_parameters.len(), 1);
        assert_eq!(constructor.type_parameters[0].name.as_ref(), "Input");
    }

    #[test]
    fn test_symbol_extraction_partial_recovery() {
        // Test that symbol extraction recovers from errors in individual classes
        // File with one valid class and one malformed class (missing closing brace)
        let source = r#"
package com.example;

public class GoodClass {
    public void goodMethod() {}
}

public class MalformedClass { // Missing closing brace
    public void method1() {}
"#;

        let result = SymbolExtractor::extract(source).unwrap();

        // Should extract GoodClass even though MalformedClass is broken
        // Note: The parser might still parse MalformedClass with error recovery,
        // so we check that at least GoodClass is present
        let good_class = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "GoodClass");
        assert!(
            good_class.is_some(),
            "Should extract GoodClass despite other malformed classes"
        );

        // The good class should have its method
        let good_method = result
            .symbols
            .iter()
            .find(|s| s.simple_name.as_ref() == "goodMethod");
        assert!(
            good_method.is_some(),
            "Should extract methods from valid classes"
        );
    }

    #[test]
    fn test_symbol_extraction_no_errors_on_valid_code() {
        let source = r#"
package com.example;

public class ValidClass {
    public void method() {}
}

public interface ValidInterface {
    void interfaceMethod();
}

public enum ValidEnum {
    VALUE_ONE, VALUE_TWO
}
"#;

        let result = SymbolExtractor::extract(source).unwrap();

        // Valid code should produce no extraction errors
        assert_eq!(
            result.errors.len(),
            0,
            "Valid code should not produce extraction errors"
        );

        // Should extract all symbols
        assert!(
            result.symbols.len() >= 3,
            "Should extract class, interface, and enum"
        );
    }
}
