// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Qualified Access Resolution
//!
//! Handles resolution of qualified member access (foo.bar.baz)

use crate::navigation::context::NavigationContext;
use crate::navigation::detect_qualified_access;
use crate::navigation::import_resolver::{
    extract_class_name, extract_package_from_root, resolve_class_fqn_via_imports,
};
use crate::navigation::lambda_inference;
use crate::navigation::type_finder::{
    find_variable_type_with_generics, find_variable_type_with_generics_and_workspace,
};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::{FileId, SemanticDatabase};
use hyperion_semantics::symbol::Type;
use hyperion_workspace::{DeclarationNamespace, WorkspaceIndex};
use rowan::{SyntaxToken, TextSize};
use std::cell::Cell;
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, SymbolKind};
use tracing::{debug, warn};

use super::NavigationProvider;

fn resolve_declared_type_name(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    owner_fqn: Option<&str>,
    written: &str,
) -> Option<String> {
    let owners = owner_fqn.into_iter().map(str::to_owned).collect::<Vec<_>>();
    match workspace_index.resolve_type_name(file_id, &owners, written) {
        hyperion_workspace::TypeNameResolution::Exact(fqn) => Some(fqn),
        hyperion_workspace::TypeNameResolution::Missing => Some(written.to_owned()),
        hyperion_workspace::TypeNameResolution::Ambiguous(_) => None,
    }
}

/// Maximum recursion depth for type parameter bound resolution
/// Prevents stack overflow on self-referential type bounds like `<T extends Comparable<T>>`
const MAX_TYPE_PARAM_DEPTH: usize = 10;

thread_local! {
    /// Track recursion depth for type parameter resolution to prevent stack overflow
    static TYPE_PARAM_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// RAII guard to automatically decrement recursion depth when dropped
struct DepthGuard;

impl Drop for DepthGuard {
    fn drop(&mut self) {
        TYPE_PARAM_DEPTH.with(|d| {
            let current = d.get();
            if current > 0 {
                d.set(current - 1);
            }
        });
    }
}

/// Helper: Resolve first segment of a chain with full workspace support for var inference
fn resolve_first_segment_with_workspace(
    workspace_index: &hyperion_workspace::WorkspaceIndex,
    file_id: FileId,
    first: &str,
    root: &SyntaxNode,
    offset: u32,
    workspace_arc: Option<Arc<WorkspaceIndex>>,
) -> Option<String> {
    // Find token at offset to use for variable lookup
    let token = root.token_at_offset(TextSize::from(offset)).right_biased();

    // Try variable type lookup first (with generics for proper resolution)
    if let Some(token) = token {
        // Use workspace-aware lookup if available (supports var inference)
        let var_type = if let Some(ref ws_arc) = workspace_arc {
            find_variable_type_with_generics_and_workspace(
                &token,
                first,
                ws_arc.clone(),
                file_id,
                root,
            )
        } else {
            find_variable_type_with_generics(&token, first)
        };

        if let Some(var_type) = var_type {
            let var_type = var_type.trim();
            debug!(
                "RESOLVE_FIRST: Found variable type for '{}': {}",
                first, var_type
            );

            // Extract base type for FQN resolution (without generics)
            let base_type = extract_base_type(var_type);

            // Resolve the variable's base type name to FQN
            match resolve_class_fqn_via_imports(workspace_index, file_id, base_type) {
                Ok(type_fqn) => {
                    // If there are type arguments, append them to the FQN
                    if let Some(type_args) = extract_type_arguments(var_type) {
                        return Some(format!("{type_fqn}{type_args}"));
                    }
                    return Some(type_fqn);
                }
                Err(_) => return Some(var_type.to_string()),
            }
        }
    }

    // Not a variable, try resolving as a class name
    resolve_class_fqn_via_imports(workspace_index, file_id, first).ok()
}

/// Extract base type from a possibly generic type string
/// "Supplier<User>" -> "Supplier"
/// "Map<String, User>" -> "Map"
fn extract_base_type(type_str: &str) -> &str {
    if let Some(idx) = type_str.find('<') {
        &type_str[..idx]
    } else {
        type_str
    }
}

/// Extract type arguments from a possibly generic type string
/// "Supplier<User>" -> Some("<User>")
/// "Map<String, User>" -> Some("<String, User>")
/// "String" -> None
fn extract_type_arguments(type_str: &str) -> Option<&str> {
    if let Some(start) = type_str.find('<') {
        Some(&type_str[start..])
    } else {
        None
    }
}

impl NavigationProvider<'_> {
    pub(crate) fn resolve_qualified_access(
        &self,
        file_id: FileId,
        receiver: &str,
        member: &str,
        member_namespace: DeclarationNamespace,
        offset: u32,
        root: &hyperion_parser::syntax::SyntaxNode,
    ) -> Result<Option<Location>, String> {
        use hyperion_semantics::resolver::ResolverBuilder;
        use hyperion_semantics::symbol::Type;

        debug!("resolve_qualified_access: resolving");
        debug!(
            "resolve_qualified_access: receiver='{}', member='{}'",
            receiver, member
        );

        // Special handling for 'super' keyword
        if receiver == "super" {
            debug!("resolve_qualified_access: Special handling for 'super' keyword");

            // Find the current class
            let package = extract_package_from_root(root);
            let class_name = root
                .children()
                .find(|n| n.kind() == SyntaxKind::CLASS_DECL)
                .and_then(|n| extract_class_name(&n));

            if let (Some(pkg), Some(cls)) = (package, class_name) {
                let current_class_fqn = format!("{pkg}.{cls}");
                debug!(
                    "resolve_qualified_access: Current class: {}",
                    current_class_fqn
                );

                // Get the parent class
                if let Some(current_class) = self.workspace_index.get_symbol(&current_class_fqn) {
                    if let Some(parent_fqn) = &current_class.parent_class {
                        debug!("resolve_qualified_access: Parent class: {}", parent_fqn);

                        // Look for the member in the parent class (with inheritance)
                        let symbol_loc = match member_namespace {
                            DeclarationNamespace::Callable => {
                                self.find_callable_with_inheritance(parent_fqn, member)
                            }
                            DeclarationNamespace::Value => {
                                self.find_value_with_inheritance(parent_fqn, member)
                            }
                            _ => None,
                        };
                        if let Some(symbol_loc) = symbol_loc {
                            debug!("resolve_qualified_access: Found member in parent class");
                            return self.symbol_location_to_lsp_location(&symbol_loc);
                        }
                    }
                }
            }

            debug!(
                "resolve_qualified_access: Could not resolve super.{}",
                member
            );
            return Ok(None);
        }

        // Special handling for 'this' keyword
        if receiver == "this" {
            debug!("resolve_qualified_access: Special handling for 'this' keyword");

            // Find the enclosing class at the cursor position (handles nested classes)
            let enclosing_class_fqn = self.find_enclosing_class_fqn_at_offset(root, offset);

            if let Some(class_fqn) = enclosing_class_fqn {
                debug!(
                    "resolve_qualified_access: Enclosing class for 'this': {}",
                    class_fqn
                );

                // Look for the member in the current class (with inheritance)
                let symbol_loc = match member_namespace {
                    DeclarationNamespace::Callable => {
                        self.find_callable_with_inheritance(&class_fqn, member)
                    }
                    DeclarationNamespace::Value => {
                        self.find_value_with_inheritance(&class_fqn, member)
                    }
                    _ => None,
                };
                if let Some(symbol_loc) = symbol_loc {
                    debug!(
                        "resolve_qualified_access: Found member '{}' via this.",
                        member
                    );
                    return self.symbol_location_to_lsp_location(&symbol_loc);
                }
            }

            debug!(
                "resolve_qualified_access: Could not resolve this.{}",
                member
            );
            return Ok(None);
        }

        // Check if receiver is a method call (ends with "()")
        let (mut receiver_name, mut is_method_call) = if receiver.ends_with("()") {
            (receiver.trim_end_matches("()"), true)
        } else {
            (receiver, false)
        };

        if !is_method_call {
            if let Some(token) = root.token_at_offset(TextSize::from(offset)).right_biased() {
                if crate::navigation::ast_queries::is_receiver_method_call(&token) {
                    is_method_call = true;
                    receiver_name = receiver_name.trim();
                }
            }
        }

        // If receiver contains dots (e.g., "User.builder"), check if the last segment is a method
        if !is_method_call && receiver_name.contains('.') {
            debug!("resolve_qualified_access: Receiver contains dots, checking if last segment is a method");

            // Extract the last segment
            if let Some(last_dot) = receiver_name.rfind('.') {
                let prefix = &receiver_name[..last_dot];
                let last_segment = &receiver_name[last_dot + 1..];

                debug!(
                    "resolve_qualified_access: Checking if '{}' is a method on '{}'",
                    last_segment, prefix
                );

                // Check if the prefix is a class name (starts with uppercase)
                if prefix.chars().next().is_some_and(char::is_uppercase) {
                    // Try to resolve the prefix as a class and check if last_segment is a method
                    let package = extract_package_from_root(root);
                    let class_name = root
                        .children()
                        .find(|n| n.kind() == SyntaxKind::CLASS_DECL)
                        .and_then(|n| extract_class_name(&n));

                    if let (Some(_pkg), Some(_cls)) = (package, class_name) {
                        // Resolve prefix to FQN
                        if let Ok(prefix_fqn) = resolve_class_fqn_via_imports(
                            self.workspace_index.as_ref(),
                            file_id,
                            prefix,
                        ) {
                            // Check if last_segment is a method in that class
                            let method_fqn = format!("{prefix_fqn}.{last_segment}");
                            debug!(
                                "resolve_qualified_access: Checking if method exists: {}",
                                method_fqn
                            );

                            if let Some(method_symbol) =
                                self.workspace_index.get_symbol(&method_fqn)
                            {
                                if method_symbol.kind == tower_lsp::lsp_types::SymbolKind::METHOD {
                                    debug!("resolve_qualified_access: Last segment is a method!");
                                    is_method_call = true;
                                    // Use full chain as receiver (receiver_name already contains it)
                                }
                            }
                        }
                    }
                }
            }
        }

        // Build symbol resolver from parse tree
        let builder = ResolverBuilder::with_workspace(file_id, self.workspace_index.clone());
        let resolver = builder.build_from_syntax(root);

        // Find the scope at the cursor position
        let scope_at_position = resolver.find_scope_at_offset(offset);
        debug!(
            "resolve_qualified_access: Scope at position (offset {}): {:?}",
            offset, scope_at_position
        );

        // If receiver is a method call, resolve it to get its return type
        let receiver_type = if is_method_call {
            debug!("resolve_qualified_access: Receiver is a method call, looking up return type");

            // Check if receiver is a chain (e.g., "User.builder" or "User.Builder")
            if receiver_name.contains('.') {
                debug!("resolve_qualified_access: Receiver contains dot: '{}', checking if it's a nested class or method chain", receiver_name);

                // First, check if this is a nested class reference (e.g., "User.Builder")
                // Try to resolve it as a class first
                if let Ok(class_fqn) = resolve_class_fqn_via_imports(
                    self.workspace_index.as_ref(),
                    file_id,
                    receiver_name,
                ) {
                    // Verify this FQN actually exists as a CLASS in the workspace
                    // (not just a method that happens to have a dot in its name)
                    if let Some(symbol) = self.workspace_index.get_symbol(&class_fqn) {
                        if symbol.kind == tower_lsp::lsp_types::SymbolKind::CLASS {
                            debug!(
                                "resolve_qualified_access: Resolved '{}' as nested class: '{}'",
                                receiver_name, class_fqn
                            );

                            Some(super::fqn_to_type(&class_fqn))
                        } else {
                            debug!("resolve_qualified_access:'{}' resolved to '{}' but it's not a CLASS (kind: {:?}), treating as method chain", receiver_name, class_fqn, symbol.kind);
                            // Not a class, treat as method chain - use helper function
                            self.resolve_dotted_chain(receiver_name, file_id, root, offset, true)
                        }
                    } else {
                        // Symbol doesn't exist in workspace, treat as method chain - use helper function
                        debug!(
                            "resolve_qualified_access:'{}' not found in workspace, treating as method chain",
                            class_fqn
                        );
                        self.resolve_dotted_chain(receiver_name, file_id, root, offset, true)
                    }
                } else {
                    // resolve_class_fqn_via_imports failed, treat as method chain - use helper function
                    debug!(
                        "resolve_qualified_access: Could not resolve '{}' via imports, treating as method chain",
                        receiver_name
                    );
                    self.resolve_dotted_chain(receiver_name, file_id, root, offset, true)
                }
            } else {
                // Simple method call - resolve it in the current class
                let package = extract_package_from_root(root);
                let class_name = root
                    .children()
                    .find(|n| n.kind() == SyntaxKind::CLASS_DECL)
                    .and_then(|n| extract_class_name(&n));

                if let (Some(pkg), Some(cls)) = (package, class_name) {
                    let method_fqn = format!("{pkg}.{cls}.{receiver_name}");
                    debug!(
                        "resolve_qualified_access: Looking for method: {}",
                        method_fqn
                    );

                    if let Some(method_symbol) = self.workspace_index.get_symbol(&method_fqn) {
                        debug!("resolve_qualified_access: Found method in workspace");

                        if let Some(return_type) = &method_symbol.return_type {
                            debug!(
                                "resolve_qualified_access: Method return type: {}",
                                return_type
                            );

                            // Resolve the return type with workspace awareness
                            let containing_class_fqn = format!("{pkg}.{cls}");
                            let Some(return_type_fqn) = resolve_declared_type_name(
                                &self.workspace_index,
                                file_id,
                                Some(&containing_class_fqn),
                                return_type,
                            ) else {
                                return Ok(None);
                            };

                            Some(super::fqn_to_type(&return_type_fqn))
                        } else {
                            debug!(
                                "resolve_qualified_access: Method has no return type information"
                            );
                            None
                        }
                    } else {
                        debug!("resolve_qualified_access: Method not found in workspace");
                        None
                    }
                } else {
                    debug!("resolve_qualified_access: Could not determine package or class name");
                    None
                }
            }
        } else {
            // Not a method call - could be a field chain like "user.profile" - use helper function
            if receiver_name.contains('.') {
                debug!(
                    "resolve_qualified_access: Non-method-call receiver contains dot: '{}', treating as field chain",
                    receiver_name
                );
                self.resolve_dotted_chain(receiver_name, file_id, root, offset, false)
            } else {
                None
            }
        };

        // If we got a type from method call resolution, use it; otherwise try variable/class resolution
        let receiver_type = if let Some(typ) = receiver_type {
            typ
        } else {
            // IMPORTANT: Check if receiver looks like a class name BEFORE trying variable resolution
            // This handles static method calls like: Utils.helper(), Math.max(), etc.
            // A class name typically starts with an uppercase letter
            let looks_like_class = receiver_name.chars().next().is_some_and(char::is_uppercase);

            if looks_like_class {
                debug!("resolve_qualified_access: Receiver '{}' starts with uppercase - checking if it's a class name for static access", receiver_name);

                // First, check if this could be an enum constant
                // Enum constants like Color.RED need to be treated as instances of the enum, not as classes
                if let Ok(resolved_fqn) = resolve_class_fqn_via_imports(
                    self.workspace_index.as_ref(),
                    file_id,
                    receiver_name,
                ) {
                    debug!(
                        "resolve_qualified_access: Resolved '{}' to FQN: '{}'",
                        receiver_name, resolved_fqn
                    );

                    // Check if this FQN is actually an enum constant in the workspace
                    if let Some(symbol) = self.workspace_index.get_symbol(&resolved_fqn) {
                        debug!(
                            "resolve_qualified_access: Found symbol with kind: {:?}",
                            symbol.kind
                        );

                        // Check if this is an enum member
                        if symbol.is_enum_member() {
                            debug!(
                                "resolve_qualified_access:'{}' is an enum constant with container: {:?}",
                                receiver_name, symbol.container
                            );

                            // Get the enum class from the container
                            if let Some(container_fqn) = &symbol.container {
                                debug!(
                                    "resolve_qualified_access: Using container class '{}' as receiver type",
                                    container_fqn
                                );

                                // Parse container FQN using safe helper
                                super::fqn_to_type(container_fqn)
                            } else {
                                warn!(
                                    "resolve_qualified_access: Enum constant '{}' (FQN: '{}') has no container - this may indicate incomplete indexing",
                                    receiver_name, resolved_fqn
                                );
                                // Fall back to class resolution
                                if let Ok(class_type) = self.resolve_class_name_for_static_access(
                                    file_id,
                                    receiver_name,
                                    root,
                                ) {
                                    class_type
                                } else {
                                    debug!("resolve_qualified_access: Could not resolve as class either");
                                    return Ok(None);
                                }
                            }
                        } else {
                            // Not an enum constant, treat as class
                            debug!(
                                "resolve_qualified_access:'{}' is not an enum constant, treating as class",
                                receiver_name
                            );
                            if let Ok(class_type) = self.resolve_class_name_for_static_access(
                                file_id,
                                receiver_name,
                                root,
                            ) {
                                debug!(
                                    "resolve_qualified_access: Resolved '{}' as class name for static access",
                                    receiver_name
                                );
                                class_type
                            } else {
                                // Not a class, fall through to variable/lambda resolution
                                if let Some(var_type) = self.resolve_receiver_as_variable(
                                    file_id,
                                    receiver_name,
                                    &resolver,
                                    scope_at_position,
                                    root,
                                    offset,
                                ) {
                                    var_type
                                } else {
                                    return Ok(None);
                                }
                            }
                        }
                    } else {
                        debug!(
                            "resolve_qualified_access: Could not find symbol for FQN: '{}'",
                            resolved_fqn
                        );
                        // Symbol not in workspace, try resolving as class normally
                        if let Ok(class_type) =
                            self.resolve_class_name_for_static_access(file_id, receiver_name, root)
                        {
                            debug!(
                                "resolve_qualified_access: Resolved '{}' as class name for static access",
                                receiver_name
                            );
                            class_type
                        } else if let Some(var_type) = self.resolve_receiver_as_variable(
                            file_id,
                            receiver_name,
                            &resolver,
                            scope_at_position,
                            root,
                            offset,
                        ) {
                            var_type
                        } else {
                            return Ok(None);
                        }
                    }
                } else {
                    debug!(
                        "resolve_qualified_access: Could not resolve '{}' via imports, trying as class",
                        receiver_name
                    );
                    // Could not resolve via imports, try as class
                    if let Ok(class_type) =
                        self.resolve_class_name_for_static_access(file_id, receiver_name, root)
                    {
                        debug!(
                            "resolve_qualified_access: Resolved '{}' as class name for static access",
                            receiver_name
                        );
                        class_type
                    } else if let Some(var_type) = self.resolve_receiver_as_variable(
                        file_id,
                        receiver_name,
                        &resolver,
                        scope_at_position,
                        root,
                        offset,
                    ) {
                        var_type
                    } else {
                        return Ok(None);
                    }
                }
            } else {
                // Lowercase start - probably a variable
                debug!(
                    "resolve_qualified_access: Receiver '{}' starts with lowercase - trying as variable first",
                    receiver_name
                );

                if let Some(var_type) = self.resolve_receiver_as_variable(
                    file_id,
                    receiver_name,
                    &resolver,
                    scope_at_position,
                    root,
                    offset,
                ) {
                    var_type
                } else {
                    // Not a variable/lambda — last resort: try as class name
                    match self.resolve_class_name_for_static_access(file_id, receiver_name, root) {
                        Ok(class_type) => class_type,
                        Err(e) => {
                            debug!("resolve_qualified_access:{}", e);
                            return Ok(None);
                        }
                    }
                }
            }
        };

        // Extract class FQN from the type
        let class_fqn = if let Type::Class(class_type) = &receiver_type {
            debug!(
                "resolve_qualified_access: Type is Class: package={:?}, name={}",
                class_type.package, class_type.name
            );

            // Check if this might be a type parameter (e.g., "T" in <T extends User>)
            // Type parameters are usually single uppercase letters or short names
            // Common patterns: T, E, K, V, TYPE, ELEMENT
            let might_be_type_param = !class_type.name.contains('.')
                && (class_type.name.len() == 1
                    || (class_type.name.len() <= 10
                        && class_type.name.chars().all(|c| c.is_ascii_uppercase())));

            if might_be_type_param {
                debug!(
                    "resolve_qualified_access:'{}' looks like a type parameter, searching for bound",
                    class_type.name
                );

                // Try to find the type parameter declaration in the method
                if let Some(bound_fqn) =
                    self.find_type_parameter_bound(root, file_id, offset, &class_type.name)
                {
                    debug!(
                        "resolve_qualified_access: Found type parameter bound: {} -> {}",
                        class_type.name, bound_fqn
                    );
                    bound_fqn
                } else {
                    // No bound found, try to resolve as normal class
                    debug!(
                        "resolve_qualified_access: No bound found for type parameter '{}', trying as class",
                        class_type.name
                    );
                    if let Some(ref pkg) = class_type.package {
                        let fqn = format!("{}.{}", pkg, class_type.name);
                        debug!(
                            "resolve_qualified_access: Constructed FQN from package: {}",
                            fqn
                        );
                        fqn
                    } else {
                        debug!(
                            "resolve_qualified_access: No package in type, resolving via imports"
                        );
                        resolve_class_fqn_via_imports(
                            self.workspace_index.as_ref(),
                            file_id,
                            &class_type.name,
                        )?
                    }
                }
            } else {
                // Not a type parameter, construct FQN normally
                if let Some(ref pkg) = class_type.package {
                    let fqn = format!("{}.{}", pkg, class_type.name);
                    debug!(
                        "resolve_qualified_access: Constructed FQN from package: {}",
                        fqn
                    );
                    fqn
                } else {
                    debug!("resolve_qualified_access: No package in type, resolving via imports");
                    // Package not resolved, try to resolve via imports
                    resolve_class_fqn_via_imports(
                        self.workspace_index.as_ref(),
                        file_id,
                        &class_type.name,
                    )?
                }
            }
        } else {
            debug!(
                "resolve_qualified_access: Receiver type is not a class: {:?}",
                receiver_type
            );
            return Ok(None);
        };

        debug!(
            "resolve_qualified_access: Resolved class FQN: {}",
            class_fqn
        );

        // Query workspace for the member (with inheritance support)
        let symbol_loc = match member_namespace {
            DeclarationNamespace::Callable => {
                self.find_callable_with_inheritance(&class_fqn, member)
            }
            DeclarationNamespace::Value => self.find_value_with_inheritance(&class_fqn, member),
            _ => None,
        };
        if let Some(symbol_loc) = symbol_loc {
            debug!(
                "resolve_qualified_access: Found member '{}' on class '{}'",
                member, class_fqn
            );
            return self.symbol_location_to_lsp_location(&symbol_loc);
        }

        // Enums implicitly inherit from java.lang.Enum, but workspace symbols for
        // source enums may not always carry an explicit parent_class edge.
        if let Some(class_symbol) = self.workspace_index.get_symbol(&class_fqn) {
            if class_symbol.kind == SymbolKind::ENUM {
                let symbol_loc = match member_namespace {
                    DeclarationNamespace::Callable => {
                        self.find_callable_with_inheritance("java.lang.Enum", member)
                    }
                    DeclarationNamespace::Value => {
                        self.find_value_with_inheritance("java.lang.Enum", member)
                    }
                    _ => None,
                };
                if let Some(symbol_loc) = symbol_loc {
                    debug!(
                        "resolve_qualified_access: Found enum inherited member '{}' on java.lang.Enum",
                        member
                    );
                    return self.symbol_location_to_lsp_location(&symbol_loc);
                }
            }
        }

        debug!(
            "resolve_qualified_access: Member '{}' not found on class '{}'",
            member, class_fqn
        );
        Ok(None)
    }

    /// Resolve a method chain by iteratively resolving each step
    ///
    /// For example, for `user.getProfile().getName()`:
    /// 1. Resolve `user` to get its type (e.g., `User`)
    /// 2. Look up `getProfile()` on `User`, get return type (e.g., `Profile`)
    /// 3. Look up `getName()` on `Profile` - this is our target
    ///
    /// Resolve a method reference (e.g., String::valueOf, list::add, ArrayList::new)
    pub(crate) fn resolve_qualified_member_access(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        offset: u32,
    ) -> Result<Option<Location>, String> {
        if let Some((receiver, member)) = detect_qualified_access(token) {
            debug!("Detected qualified access: {}.{}", receiver, member);

            let member_namespace = if crate::navigation::ast_queries::is_followed_by_paren(token)
                || token
                    .parent_ancestors()
                    .any(|node| node.kind() == SyntaxKind::METHOD_REF_EXPR)
            {
                DeclarationNamespace::Callable
            } else {
                DeclarationNamespace::Value
            };

            match self.resolve_qualified_access(
                ctx.file_id,
                &receiver,
                &member,
                member_namespace,
                offset,
                &ctx.parse_tree.syntax(),
            ) {
                Ok(Some(location)) => {
                    debug!("Resolved qualified access to: {:?}", location);
                    return Ok(Some(location));
                }
                Ok(None) => {
                    #[cfg(debug_assertions)]
                    debug!("NAV: resolve_qualified_access returned None");
                }
                Err(_e) => {
                    #[cfg(debug_assertions)]
                    debug!("NAV: resolve_qualified_access error: {}", _e);
                }
            }
        }

        if let Some(location) = self.resolve_member_via_receiver_expression(ctx, token, offset)? {
            return Ok(Some(location));
        }

        Ok(None)
    }

    /// Fallback resolver for complex qualified calls where textual receiver extraction is brittle.
    ///
    /// Examples:
    /// - `switch (...) { ... }.toUpperCase()`
    /// - `Day.MONDAY.ordinal()`
    /// - `user.getName()` where `user` is a `var` inferred from a complex initializer
    ///
    /// `pub(super)` so the chain entry point can consult it for a call whose
    /// receiver is an expression the chain parser produced no segment for.
    pub(super) fn resolve_member_via_receiver_expression(
        &self,
        ctx: &NavigationContext,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        offset: u32,
    ) -> Result<Option<Location>, String> {
        let Some(method_call) = token
            .parent_ancestors()
            .find(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
        else {
            return Ok(None);
        };

        // Only for qualified access: receiver . member(...)
        let has_dot = method_call
            .children_with_tokens()
            .any(|child| child.kind() == SyntaxKind::DOT);
        if !has_dot {
            return Ok(None);
        }

        // Receiver is the expression node before the first dot.
        let receiver_expr = method_call.children().find(|child| {
            Self::is_receiver_expression_kind(child.kind())
                && child.text_range().start() <= token.text_range().start()
        });
        let Some(receiver_expr) = receiver_expr else {
            return Ok(None);
        };

        let receiver_type = self
            .cached_expression_type(ctx.file_id, &receiver_expr)
            .or_else(|| self.infer_expression_type_with_engine(ctx, &receiver_expr, offset));

        let Some(receiver_type) = receiver_type else {
            return Ok(None);
        };

        if matches!(receiver_type, Type::Unknown) {
            return Ok(None);
        }

        self.resolve_method_on_inferred_type(&receiver_type, token.text(), ctx.file_id)
    }

    fn cached_expression_type(&self, file_id: FileId, expr: &SyntaxNode) -> Option<Type> {
        let file_sem = self.txn.file_semantics(file_id);
        file_sem.expression_type_at(expr.text_range()).cloned()
    }

    fn infer_expression_type_with_engine(
        &self,
        ctx: &NavigationContext,
        expr: &SyntaxNode,
        _offset: u32,
    ) -> Option<Type> {
        let builder = hyperion_semantics::resolver::ResolverBuilder::with_workspace(
            ctx.file_id,
            self.workspace_index.clone(),
        );
        let resolver = builder.build_from_syntax(&ctx.parse_tree.syntax());
        let engine = self.create_type_inference_engine(ctx.file_id);
        let inferred = engine.infer_expression(expr, &resolver, None);
        if matches!(inferred, Type::Unknown) {
            None
        } else {
            Some(inferred)
        }
    }

    const fn is_receiver_expression_kind(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::NAME_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::NEW_EXPR
                | SyntaxKind::PAREN_EXPR
                | SyntaxKind::THIS_EXPR
                | SyntaxKind::SUPER_EXPR
                | SyntaxKind::CAST_EXPR
                | SyntaxKind::ARRAY_ACCESS_EXPR
                | SyntaxKind::TERNARY_EXPR
                | SyntaxKind::SWITCH_EXPR
                | SyntaxKind::LITERAL_EXPR
                | SyntaxKind::INSTANCEOF_EXPR
                | SyntaxKind::ASSIGNMENT_EXPR
        )
    }

    pub(crate) fn resolve_inner_class_lookup(
        &self,
        ctx: &NavigationContext,
        qualified_name: &str,
        token: &SyntaxToken<hyperion_parser::JavaLanguage>,
        workspace_file_id: FileId,
    ) -> Result<Option<Location>, String> {
        if qualified_name.contains('.') {
            let preceded_by_dot = {
                let mut found_dot = false;
                if let Some(parent) = token.parent() {
                    for child in parent.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = child {
                            if t.text_range() == token.text_range() {
                                break;
                            }
                            if t.kind() == SyntaxKind::DOT {
                                found_dot = true;
                            } else if t.kind() == SyntaxKind::IDENT
                                || t.kind() == SyntaxKind::SUPER_KW
                            {
                                // reset for new token sequences
                                found_dot = false;
                            }
                        }
                    }
                }
                found_dot
            };

            if preceded_by_dot {
                return Ok(None);
            }

            debug!(
                "NAV: Qualified name '{}' might be inner class reference",
                qualified_name
            );

            let lexical_owners = self.lexical_type_owners_at_offset(
                &ctx.parse_tree.syntax(),
                token.text_range().start(),
            );
            let hyperion_workspace::TypeNameResolution::Exact(candidate) = self
                .workspace_index
                .resolve_type_name(workspace_file_id, &lexical_owners, qualified_name)
            else {
                return Ok(None);
            };
            if let Some(definition) =
                self.workspace_index
                    .find_symbol_definition(&hyperion_workspace::SymbolTarget::new(
                        candidate,
                        DeclarationNamespace::Type,
                    ))
            {
                return self.symbol_location_to_lsp_location(&definition.location);
            }
        } else if let Some(outer_class_name) = self.extract_outer_class_from_new_expr(token) {
            debug!(
                "NAV: Detected qualified inner class constructor: {}.{}",
                outer_class_name, qualified_name
            );

            match resolve_class_fqn_via_imports(
                self.workspace_index.as_ref(),
                workspace_file_id,
                &outer_class_name,
            ) {
                Ok(outer_class_fqn) => {
                    debug!("NAV: Resolved outer class to: {}", outer_class_fqn);

                    let inner_class_fqn = format!("{outer_class_fqn}.{qualified_name}");
                    debug!(
                        "NAV: Trying qualified inner class lookup: {}",
                        inner_class_fqn
                    );
                    if let Some(definition) = self.workspace_index.find_symbol_definition(
                        &hyperion_workspace::SymbolTarget::new(
                            inner_class_fqn,
                            DeclarationNamespace::Type,
                        ),
                    ) {
                        return self.symbol_location_to_lsp_location(&definition.location);
                    }
                }
                Err(e) => {
                    debug!(
                        "NAV: Could not resolve outer class '{}': {}",
                        outer_class_name, e
                    );
                }
            }
        }

        Ok(None)
    }

    /// Resolve a receiver name by trying the standard cascade:
    /// 1. Variable in scope → get its type
    /// 2. If type is Unknown → try lambda inference
    /// 3. Pattern variable (Java 16+ instanceof)
    /// 4. Lambda parameter inference
    ///
    /// Returns `Some(Type)` if any strategy succeeds, `None` if all fail.
    fn resolve_receiver_as_variable(
        &self,
        file_id: FileId,
        receiver_name: &str,
        resolver: &hyperion_semantics::resolver::SymbolResolver,
        scope_at_position: Option<hyperion_semantics::symbol::ScopeId>,
        root: &SyntaxNode,
        offset: u32,
    ) -> Option<Type> {
        // Step 1: Resolve the receiver from the exact lexical position.
        let receiver_symbol = scope_at_position.and_then(|scope_id| {
            resolver.resolve_in_scope_at_offset(receiver_name, scope_id, offset)
        });

        if let Some(receiver_sym) = receiver_symbol {
            let symbol_data = resolver.symbol_table().get_symbol(receiver_sym.symbol_id)?;
            debug!(
                "resolve_receiver_as_variable: '{}' resolved as variable, type: {:?}",
                receiver_name, symbol_data.type_info
            );

            return match &symbol_data.type_info {
                Type::Unknown => {
                    // Step 2: Variable found but type is Unknown → try lambda inference
                    lambda_inference::infer_lambda_parameter_type_with_workspace_and_file(
                        root,
                        receiver_name,
                        offset,
                        Some(self.workspace_index.clone()),
                        file_id,
                    )
                }
                other_type => Some(other_type.clone()),
            };
        }

        // Step 3: Try pattern variable (Java 16+ instanceof)
        if let Some(pattern_type) = resolver.get_pattern_variable_type(receiver_name) {
            debug!(
                "resolve_receiver_as_variable: '{}' found as pattern variable, type: {:?}",
                receiver_name, pattern_type
            );
            return Some(pattern_type.clone());
        }

        // Step 4: Try lambda parameter inference
        if let Some(lambda_type) =
            lambda_inference::infer_lambda_parameter_type_with_workspace_and_file(
                root,
                receiver_name,
                offset,
                Some(self.workspace_index.clone()),
                file_id,
            )
        {
            debug!(
                "resolve_receiver_as_variable: '{}' inferred as lambda param, type: {:?}",
                receiver_name, lambda_type
            );
            return Some(lambda_type);
        }

        None
    }

    /// Resolve a dotted chain like "User.builder" or "user.profile.email"
    /// Returns the final type after walking the chain, or None if any segment fails
    fn resolve_dotted_chain(
        &self,
        receiver_name: &str,
        file_id: FileId,
        root: &SyntaxNode,
        offset: u32,
        prefer_methods: bool,
    ) -> Option<Type> {
        let chain_parts: Vec<&str> = receiver_name.split('.').collect();
        debug!("resolve_qualified_access: Chain parts: {:?}", chain_parts);

        if chain_parts.len() < 2 {
            return None;
        }

        let first = chain_parts[0];
        let rest = &chain_parts[1..];

        // Resolve the first part (variable or class) - pass workspace Arc for var inference
        let first_fqn = resolve_first_segment_with_workspace(
            self.workspace_index.as_ref(),
            file_id,
            first,
            root,
            offset,
            Some(self.workspace_index.clone()),
        )?;

        debug!(
            "resolve_qualified_access: Resolved first part '{}' to '{}'",
            first, first_fqn
        );

        // Build initial type from FQN using safe parser
        let mut current_type = super::fqn_to_type(&first_fqn);

        // Walk through the rest of the chain
        for (i, segment) in rest.iter().enumerate() {
            debug!(
                "resolve_qualified_access: Chain step {}: resolving '{}' on type {:?}",
                i, segment, current_type
            );

            // Try method/field based on preference
            current_type = if prefer_methods {
                // Try method first, then field

                self.get_method_return_type_for_chain(&current_type, segment, file_id)
                    .or_else(|| self.get_field_type_for_chain(&current_type, segment, file_id))?
            } else {
                // Try field first, then method

                self.get_field_type_for_chain(&current_type, segment, file_id)
                    .or_else(|| {
                        self.get_method_return_type_for_chain(&current_type, segment, file_id)
                    })?
            };

            debug!(
                "resolve_qualified_access: Resolved '{}' to type: {:?}",
                segment, current_type
            );
        }

        Some(current_type)
    }

    /// Get the return type of a method on a given type (for chain resolution)
    /// Similar to chain_resolver's get_method_return_type but with workspace-aware resolution
    fn get_method_return_type_for_chain(
        &self,
        current_type: &Type,
        method_name: &str,
        file_id: FileId,
    ) -> Option<Type> {
        self.projected_name_only_method_return_type(current_type, file_id, method_name)
    }

    /// Get the type of a field for chain resolution (e.g., user.profile.email)
    ///
    /// Similar to get_method_return_type_for_chain but for fields.
    /// Fields store their type in the `return_type` field of Symbol.
    fn get_field_type_for_chain(
        &self,
        current_type: &Type,
        field_name: &str,
        file_id: FileId,
    ) -> Option<Type> {
        let (owner, arguments) = self.chain_receiver_identity(current_type, file_id)?;
        let field = self
            .workspace_index
            .find_projected_effective_field_by_name(&owner, &arguments, field_name)?;
        hyperion_semantics::type_parser::parse_type_string(field.projected_type.as_deref()?)
    }

    /// Find the bound for a type parameter by searching the enclosing method's type parameter list
    ///
    /// For example, in `<T extends User>`, this returns "User" (as FQN) when looking up "T"
    ///
    /// `pub(super)` so `chain_resolver` can apply the same JLS 4.4 substitution:
    /// the receiver of `value.compareTo(..)` where `value : T` reaches the chain
    /// resolver, not this module.
    pub(super) fn find_type_parameter_bound(
        &self,
        root: &hyperion_parser::syntax::SyntaxNode,
        file_id: FileId,
        offset: u32,
        type_param_name: &str,
    ) -> Option<String> {
        use hyperion_parser::syntax::SyntaxKind;
        use rowan::NodeOrToken;

        // Check recursion depth to prevent stack overflow on self-referential bounds
        let _depth_guard = TYPE_PARAM_DEPTH.with(|d| {
            let current = d.get();
            if current >= MAX_TYPE_PARAM_DEPTH {
                debug!(
                    "Type parameter recursion depth limit reached ({}), breaking cycle for '{}'",
                    MAX_TYPE_PARAM_DEPTH, type_param_name
                );
                return None;
            }
            d.set(current + 1);
            Some(DepthGuard)
        })?;

        // Type parameters can be declared by any enclosing scope: the method
        // (or constructor) AND every enclosing class/interface/record. Search
        // innermost-first so a method-level `<T>` shadows a class-level one
        // (previously only the enclosing method was searched, so class-level
        // parameters like `Box<T extends Comparable<T>>` were never found —
        let token = root
            .token_at_offset(rowan::TextSize::from(offset))
            .left_biased()?;

        for ancestor in token.parent_ancestors() {
            if !matches!(
                ancestor.kind(),
                SyntaxKind::METHOD_DECL
                    | SyntaxKind::CONSTRUCTOR_DECL
                    | SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::RECORD_DECL
            ) {
                continue;
            }

            for child in ancestor.children() {
                if child.kind() != SyntaxKind::TYPE_PARAM_LIST {
                    continue;
                }
                // Search for the type parameter with matching name
                for type_param in child.children() {
                    if type_param.kind() != SyntaxKind::TYPE_PARAM {
                        continue;
                    }
                    // Extract the type parameter name and bound
                    let mut param_name: Option<String> = None;
                    let mut bound_type: Option<String> = None;

                    for item in type_param.children_with_tokens() {
                        match item {
                            NodeOrToken::Token(t) => {
                                if t.kind() == SyntaxKind::IDENT && param_name.is_none() {
                                    param_name = Some(t.text().to_string());
                                }
                            }
                            NodeOrToken::Node(n) => {
                                // Check for TYPE_BOUND node which contains "extends Type"
                                if n.kind() == SyntaxKind::TYPE_BOUND && bound_type.is_none() {
                                    // Look for CLASS_TYPE inside TYPE_BOUND
                                    for bound_child in n.children() {
                                        if bound_child.kind() == SyntaxKind::CLASS_TYPE {
                                            bound_type =
                                                self.extract_simple_type_name(&bound_child);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // A scope that declares `type_param_name` ends the search
                    // even without a usable bound: inner declarations shadow
                    // outer ones, so an unbounded inner `<T>` must not fall
                    // through to an outer `<T extends X>`.
                    if param_name.as_deref() == Some(type_param_name) {
                        let bound = bound_type?;
                        // Resolve the bound type via imports to get FQN
                        return resolve_class_fqn_via_imports(
                            self.workspace_index.as_ref(),
                            file_id,
                            &bound,
                        )
                        .ok();
                    }
                }
            }
        }

        None
    }

    /// Extract a simple type name from a CLASS_TYPE node
    fn extract_simple_type_name(
        &self,
        node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Option<String> {
        use hyperion_parser::syntax::SyntaxKind;
        use rowan::NodeOrToken;

        // For CLASS_TYPE, look for IDENT tokens (might be nested in QUALIFIED_NAME)
        for item in node.children_with_tokens() {
            match item {
                NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::IDENT {
                        return Some(t.text().to_string());
                    }
                }
                NodeOrToken::Node(n) => {
                    // Check if it's a QUALIFIED_NAME node
                    if n.kind() == SyntaxKind::QUALIFIED_NAME {
                        // For qualified names like java.util.ArrayList, get the LAST IDENT (simple name)
                        let mut last_ident: Option<String> = None;
                        for qualified_item in n.children_with_tokens() {
                            if let NodeOrToken::Token(t) = qualified_item {
                                if t.kind() == SyntaxKind::IDENT {
                                    last_ident = Some(t.text().to_string());
                                }
                            }
                        }
                        if last_ident.is_some() {
                            return last_ident;
                        }
                    }
                }
            }
        }

        None
    }
}
