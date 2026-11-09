// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Semantic walker that pre-computes name resolution for every name node in a file.
//!
//! This walker mirrors the type checker's scope-tracking structure but only performs
//! name resolution — no type checking. The results are stored in a `FileSemantics`
//! that is cached via Salsa.

use crate::db::FileId;
use crate::file_semantics::FileSemantics;
use crate::inference::TypeInferenceEngine;
use crate::resolved_name::{NameRejection, NameRejectionKind, ResolvedName};
use crate::resolver::builder::ResolverBuilder;
use crate::semantic_error::{SemanticError, SemanticErrorKind};
use crate::symbol::{ClassType, SymbolTable, Type};
use crate::unified_resolver::{NameContext, UnifiedTypeResolver};
use crate::utils::{
    build_imports_from_syntax, extract_class_type_simple_name, extract_name_expr_ident,
    find_case_mismatch_suggestion, strip_generics,
};
use hyperion_parser::{SyntaxKind, SyntaxNode};
use hyperion_workspace::{
    DeclarationNamespace, DefinitionSourceAuthority, FieldAccessQualifier,
    ProjectedFieldAccessResolution, SymbolTarget, TypeNameResolution, WorkspaceIndex,
};
use rowan::TextRange;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Compute file semantics by walking the AST and resolving every name node.
///
/// This is the entry point called by the Salsa `file_semantics` query.
pub fn compute_file_semantics(
    root: &SyntaxNode,
    file_id: FileId,
    _symbol_table: &SymbolTable,
    workspace_index: &WorkspaceIndex,
) -> FileSemantics {
    let started = Instant::now();
    debug!(?file_id, "Starting Java semantic import extraction");
    let imports = build_imports_from_syntax(root);
    log_semantic_stage(file_id, "import extraction", started.elapsed());
    let workspace_arc = Arc::new(workspace_index.clone());
    let started = Instant::now();
    debug!(?file_id, "Starting Java semantic resolver construction");
    let symbol_resolver = ResolverBuilder::with_workspace(file_id, Arc::clone(&workspace_arc))
        .build_from_syntax(root);
    log_semantic_stage(file_id, "resolver construction", started.elapsed());

    let inference_engine = TypeInferenceEngine::new(Arc::clone(&workspace_arc), file_id);

    let mut walker = SemanticWalker {
        file_id,
        variable_names_stack: vec![HashSet::with_capacity(15)],
        type_parameters: HashSet::new(),
        local_types: HashSet::new(),
        resolved_names: HashMap::new(),
        expression_types: HashMap::new(),
        diagnostics: Vec::new(),
        field_access_cache: HashMap::new(),
        workspace_index,
        imports: &imports,
        symbol_resolver: &symbol_resolver,
        inference_engine: &inference_engine,
    };

    // Pre-populate local types and type parameters from the full AST
    let started = Instant::now();
    debug!(?file_id, "Starting Java semantic local type collection");
    walker.collect_local_types(root);
    log_semantic_stage(file_id, "local type collection", started.elapsed());

    // Walk the AST
    let started = Instant::now();
    debug!(?file_id, "Starting Java semantic AST walk");
    walker.walk_node(root);
    log_semantic_stage(file_id, "AST walk", started.elapsed());

    FileSemantics::new(
        walker.resolved_names,
        walker.expression_types,
        walker.diagnostics,
    )
}

fn log_semantic_stage(file_id: FileId, stage: &'static str, elapsed: Duration) {
    if elapsed >= Duration::from_millis(250) {
        warn!(
            ?file_id,
            stage,
            ?elapsed,
            "Slow Java semantic stage completed"
        );
    } else {
        debug!(?file_id, stage, ?elapsed, "Java semantic stage completed");
    }
}

/// Internal walker state.
struct SemanticWalker<'a> {
    /// Exact source file being analyzed.
    file_id: FileId,
    /// Stack of scopes, each containing known variable names
    variable_names_stack: Vec<HashSet<String>>,
    /// Known type parameters in scope
    type_parameters: HashSet<String>,
    /// Local type declarations (classes/interfaces/enums/records in this file)
    local_types: HashSet<String>,
    /// Accumulated resolution results
    resolved_names: HashMap<TextRange, ResolvedName>,
    /// Accumulated expression type inference results
    expression_types: HashMap<TextRange, Type>,
    /// Accumulated diagnostics
    diagnostics: Vec<SemanticError>,
    /// Canonical field election is hierarchy-aware and can be expensive. One
    /// file commonly repeats the same receiver/member/access context many
    /// times, so retain the exact result for this immutable semantic pass.
    field_access_cache: HashMap<FieldAccessCacheKey, ProjectedFieldAccessResolution>,
    /// Workspace index for cross-file resolution
    workspace_index: &'a WorkspaceIndex,
    /// Import map for this file
    imports: &'a hyperion_workspace::ImportMap,
    /// Symbol resolver for same-file resolution
    symbol_resolver: &'a crate::resolver::SymbolResolver,
    /// Type inference engine for expression types
    inference_engine: &'a TypeInferenceEngine,
}

/// Outcome of binding an unqualified `case NAME` label.
enum SwitchLabel {
    Bound,
    /// The selector's type is not a known enum, so the label proves nothing.
    SelectorUnknown,
    /// The selector is a known enum that has no such constant.
    MissingFromKnownEnum,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FieldAccessCacheKey {
    receiver_fqn: String,
    receiver_arguments: Vec<String>,
    member_name: String,
    lexical_access_owners: Vec<String>,
    is_super: bool,
}

impl SemanticWalker<'_> {
    fn enter_scope(&mut self) {
        self.variable_names_stack.push(HashSet::with_capacity(8));
    }

    fn exit_scope(&mut self) {
        if self.variable_names_stack.len() > 1 {
            self.variable_names_stack.pop();
        }
    }

    fn insert_variable(&mut self, name: String) {
        if let Some(current_scope) = self.variable_names_stack.last_mut() {
            current_scope.insert(name);
        }
    }

    fn infer_and_store(&mut self, node: &SyntaxNode) {
        let inferred = self
            .inference_engine
            .infer_expression(node, self.symbol_resolver, None);
        if !matches!(inferred, Type::Unknown) {
            self.expression_types.insert(node.text_range(), inferred);
        }
    }

    fn build_name_context<'b>(
        &'b self,
        in_switch_case: bool,
        allow_external_type: bool,
        expected_namespace: Option<hyperion_workspace::DeclarationNamespace>,
        lexical_owners: &'b [String],
    ) -> NameContext<'b> {
        NameContext {
            in_switch_case,
            type_parameters: &self.type_parameters,
            variable_scopes: &self.variable_names_stack,
            local_types: &self.local_types,
            lexical_owners,
            allow_external_type,
            expected_namespace,
        }
    }

    fn collect_local_types(&mut self, root: &SyntaxNode) {
        for node in root.descendants() {
            match node.kind() {
                SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::RECORD_DECL
                | SyntaxKind::ANNOTATION_DECL => {
                    if let Some(name) = extract_type_decl_name(&node) {
                        self.local_types.insert(name);
                    }
                }
                SyntaxKind::TYPE_PARAM => {
                    if let Some(name) = extract_type_param_name(&node) {
                        self.type_parameters.insert(name);
                    }
                }
                _ => {}
            }
        }
    }

    fn walk_node(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SyntaxKind::BLOCK_STMT
            | SyntaxKind::CLASS_DECL
            | SyntaxKind::INTERFACE_DECL
            | SyntaxKind::ENUM_DECL
            | SyntaxKind::RECORD_DECL
            | SyntaxKind::METHOD_DECL
            | SyntaxKind::CONSTRUCTOR_DECL => {
                self.enter_scope();
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.exit_scope();
            }
            SyntaxKind::LOCAL_VAR_DECL => {
                for name in crate::utils::declarator_identifiers(node) {
                    self.insert_variable(name.text().to_owned());
                }
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::PARAM => {
                if let Some(name) = extract_param_name(node) {
                    self.insert_variable(name);
                }
                // Recurse so the declared type (e.g. `Foo` in `void f(Foo x)`)
                // is resolved for navigation and checked for existence.
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::TYPE_PARAM => {
                if let Some(name) = extract_type_param_name(node) {
                    self.type_parameters.insert(name);
                }
                // Recurse into children so that types in bounds
                // (e.g. `Number` in `<T extends Number>`) get resolved.
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::FIELD_DECL => {
                for declarator in hyperion_types::field_declarators(node) {
                    self.insert_variable(declarator.name);
                }
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::IMPORT_DECL => self.check_import_decl(node),
            SyntaxKind::FIELD_ACCESS_EXPR => {
                self.infer_and_store(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
                self.check_field_access(node);
            }
            SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::SWITCH_EXPR
            | SyntaxKind::LITERAL_EXPR
            | SyntaxKind::PAREN_EXPR => {
                self.infer_and_store(node);
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
            SyntaxKind::CLASS_LITERAL_EXPR => self.check_class_literal(node),
            SyntaxKind::CLASS_TYPE => {
                self.check_unresolved_class_type(node);
                // The qualified head is resolved above (`extract_class_type_simple_name`
                // stops at `<`), so only the type arguments remain: each has its own
                // identifier range and is checked like any other type position.
                self.check_type_arguments(node);
            }
            SyntaxKind::NAME_EXPR => {
                // Store inferred expression type for NAME_EXPR so navigation's
                // FileSemantics fast path can resolve receiver types (e.g.,
                // `user.getName()` needs to know `user`'s type).
                self.infer_and_store(node);
                self.check_unresolved_name_expr(node);
            }
            _ => {
                for child in node.children() {
                    self.walk_node(&child);
                }
            }
        }
    }

    fn check_import_decl(&mut self, node: &SyntaxNode) {
        let import_text = node.text().to_string();
        let mut path = import_text.trim();
        path = path.strip_prefix("import ").unwrap_or(path);
        path = path.strip_prefix("static ").unwrap_or(path);
        path = path.trim_end_matches(';').trim();

        // Skip wildcard and static imports to avoid false positives.
        if path.ends_with(".*") || import_text.contains("static ") {
            return;
        }

        // Skip java.* imports if JDK isn't indexed
        if path.starts_with("java.") && !self.workspace_index.has_core_jdk_classes() {
            return;
        }

        // Skip external package imports if we don't have ANY classes from that package indexed.
        if let Some(last_dot) = path.rfind('.') {
            let package = &path[..last_dot];
            if !self.workspace_index.package_has_class_names(package) {
                return;
            }
        }

        debug!(
            "SEMANTIC_WALKER: import '{}' scoped={} has_class={}",
            path,
            self.workspace_index.has_source_scope(),
            self.workspace_index.has_class(path)
        );
        if !self.workspace_index.has_class(path) {
            self.diagnostics.push(SemanticError::new(
                node.text_range(),
                SemanticErrorKind::UndefinedSymbol {
                    symbol_name: path.to_string(),
                    suggested_symbol: None,
                },
            ));
        }
    }

    fn check_class_literal(&mut self, node: &SyntaxNode) {
        // The parser retains an expression-shaped operand (NAME_EXPR, qualified
        // access, or array syntax), but JLS 15.8.2 requires a type here. Resolve
        // the complete spelling once instead of walking it as values/fields.
        let tokens: Vec<_> = node
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .take_while(|token| token.kind() != SyntaxKind::CLASS_KW)
            .filter(|token| !token.kind().is_trivia())
            .collect();
        let text: String = tokens.iter().map(|token| token.text()).collect();
        let Some(operand) = text.strip_suffix('.') else {
            return;
        };
        let raw_name = strip_generics(operand.trim_end_matches("[]"));
        if matches!(
            raw_name.as_str(),
            "boolean" | "byte" | "short" | "int" | "long" | "char" | "float" | "double" | "void"
        ) {
            return;
        }
        let Some(identifier) = tokens
            .iter()
            .rev()
            .find(|token| token.kind().is_identifier())
        else {
            return;
        };
        self.check_type_reference(
            node,
            raw_name,
            identifier.text().to_string(),
            identifier.text_range(),
        );
    }

    fn check_type_arguments(&mut self, class_type: &SyntaxNode) {
        for arguments in class_type
            .children()
            .filter(|child| child.kind() == SyntaxKind::TYPE_ARGS)
        {
            for argument in arguments.children() {
                match argument.kind() {
                    SyntaxKind::CLASS_TYPE | SyntaxKind::ARRAY_TYPE => self.walk_node(&argument),
                    SyntaxKind::WILDCARD_TYPE => {
                        for bound in argument.children() {
                            self.walk_node(&bound);
                        }
                    }
                    // `List<int[]>` parses as PRIMITIVE_TYPE followed by an ARRAY_TYPE
                    // sibling and is a perfectly good reference type.
                    SyntaxKind::PRIMITIVE_TYPE
                        if argument
                            .next_sibling()
                            .is_none_or(|next| next.kind() != SyntaxKind::ARRAY_TYPE) =>
                    {
                        self.diagnostics.push(SemanticError::new(
                            argument.text_range(),
                            SemanticErrorKind::PrimitiveTypeArgument {
                                type_name: argument.text().to_string().trim().to_owned(),
                            },
                        ));
                    }
                    _ => {}
                }
            }
        }
    }

    fn check_unresolved_class_type(&mut self, node: &SyntaxNode) {
        // Skip types in import/package declarations
        if node.ancestors().any(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::IMPORT_DECL | SyntaxKind::PACKAGE_DECL
            )
        }) {
            return;
        }

        // `outer.new Inner()`: `Inner` is a member type of the type of `outer`
        // (JLS 15.9.1), not a name in the enclosing lexical scope.
        if node.parent().is_some_and(|parent| {
            parent.kind() == SyntaxKind::NEW_EXPR
                && parent
                    .children_with_tokens()
                    .take_while(|element| element.kind() != SyntaxKind::NEW_KW)
                    .any(|element| element.kind() == SyntaxKind::DOT)
        }) {
            return;
        }

        let Some((simple_name, ident_range)) = extract_class_type_simple_name(node) else {
            return;
        };

        // Trivia (`implements A, java.io.Serializable // as of 2.1`) belongs to
        // the node but not to the name: only significant tokens spell the type.
        let spelled: String = node
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| !token.kind().is_trivia())
            .map(|token| token.text().to_string())
            .collect();
        let raw_name = strip_generics(&spelled);
        self.check_type_reference(node, raw_name, simple_name, ident_range);
    }

    fn check_type_reference(
        &mut self,
        node: &SyntaxNode,
        raw_name: String,
        simple_name: String,
        ident_range: TextRange,
    ) {
        let offset = node.text_range().start().into();
        let lexical_owners = self.get_lexical_class_fqns(node);
        let name_ctx = self.build_name_context(
            false,
            true,
            Some(hyperion_workspace::DeclarationNamespace::Type),
            &lexical_owners,
        );
        let unified = UnifiedTypeResolver::new(
            self.workspace_index,
            self.imports,
            Some(self.symbol_resolver),
        );

        if let Some(resolved) = unified.resolve_name(&raw_name, offset, &name_ctx) {
            debug!(
                "SEMANTIC_WALKER: Resolved class type '{}' via resolve_name",
                raw_name
            );
            self.resolved_names.insert(ident_range, resolved);
            return;
        }

        // `some.pkg.Type` written inline: with no class of `some.pkg` indexed
        // the package comes from a dependency this workspace does not see (or
        // does not exist -- the build would say so). Imports get the same
        // benefit of the doubt in `check_import_decl`.
        if let Some((package, _)) = raw_name.rsplit_once('.') {
            if !self.workspace_index.package_has_class_names(package)
                && !self.workspace_index.has_class(package)
            {
                return;
            }
        }

        let suggested_symbol = find_case_mismatch_suggestion(&simple_name, self.imports);
        let symbol_name = if raw_name.contains('.') {
            raw_name
        } else {
            simple_name
        };

        self.diagnostics.push(SemanticError::new(
            ident_range,
            SemanticErrorKind::UndefinedSymbol {
                symbol_name,
                suggested_symbol,
            },
        ));
    }

    fn check_unresolved_name_expr(&mut self, node: &SyntaxNode) {
        // Skip if this is part of a CLASS_TYPE (handled by check_unresolved_class_type)
        if node
            .ancestors()
            .any(|ancestor| matches!(ancestor.kind(), SyntaxKind::CLASS_TYPE))
        {
            return;
        }

        // Field-access chain prefixes (e.g., `com` in `com.foo.bar.Bar.doSomething()`)
        // should still be resolved for navigation, but should not produce diagnostics
        // when unresolved. Without this, fully-qualified references produce spurious
        // "cannot find symbol: com" errors for the leftmost package segment.
        //
        // Do NOT suppress NAME_EXPR receivers in METHOD_CALL_EXPR (e.g., `missing` in
        // `missing.call()`), because those should still report undefined-symbol when
        // unresolved.
        // `Outer.this` and `Outer.super` parse as THIS_EXPR/SUPER_EXPR[NAME_EXPR ...]:
        // the qualifier names a type, exactly like a field-access chain prefix.
        let is_chain_receiver = node.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::THIS_EXPR | SyntaxKind::SUPER_EXPR
            )
        });
        let parent_method_call = node
            .parent()
            .filter(|parent| parent.kind() == SyntaxKind::METHOD_CALL_EXPR);
        let is_qualified_call_receiver = parent_method_call.as_ref().is_some_and(|call| {
            call.children_with_tokens().any(|element| {
                element
                    .into_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::DOT)
            })
        });

        // `Foo::bar` parses as METHOD_REF_EXPR[NAME_EXPR, `::`, IDENT], the same
        // qualifier position as `Foo.bar()`, so it needs the same treatment.
        let is_method_reference_qualifier = is_method_reference_qualifier(node);

        let Some((name, ident_range)) = extract_name_expr_ident(node) else {
            return;
        };

        let in_switch_label = Self::is_in_switch_label(node);
        // A method-reference qualifier may name either a type (`String::valueOf`)
        // or a value (`instance::method`), so it must not be constrained to the
        // value namespace the way a bare expression is.
        let expected_namespace =
            if is_chain_receiver || is_qualified_call_receiver || is_method_reference_qualifier {
                None
            } else if parent_method_call.is_some() {
                Some(hyperion_workspace::DeclarationNamespace::Callable)
            } else {
                Some(hyperion_workspace::DeclarationNamespace::Value)
            };

        // Per JLS 6.5.2 a call or method-reference qualifier is reclassified as a
        // TypeName once no variable of that name is in scope, so it may
        // legitimately name a type from an unindexed dependency. Only chain
        // receivers were granted that allowance, which reported every static
        // call on an imported third-party class as an unresolved symbol.
        // Unlike chain receivers this does not suppress the diagnostic, so
        // `missing.call()` is still reported when nothing matches at all.
        let offset = node.text_range().start().into();
        let lexical_owners = self.get_lexical_class_fqns(node);
        let name_ctx = self.build_name_context(
            in_switch_label,
            is_chain_receiver || is_qualified_call_receiver || is_method_reference_qualifier,
            expected_namespace,
            &lexical_owners,
        );
        let unified = UnifiedTypeResolver::new(
            self.workspace_index,
            self.imports,
            Some(self.symbol_resolver),
        );

        if let Some(resolved) = unified.resolve_name(&name, offset, &name_ctx) {
            debug!(
                "SEMANTIC_WALKER: Resolved name expr '{}' via resolve_name",
                name
            );
            self.resolved_names.insert(ident_range, resolved);
            return;
        }

        // An unqualified enum label is scoped by the switch selector's type.
        // Bind it here so every semantic consumer sees the same exact identity.
        if in_switch_label {
            match self.bind_switch_enum_constant(node, &name, ident_range) {
                SwitchLabel::Bound => return,
                // The selector's type is unknown (a library value whose type this
                // engine cannot infer, a pattern switch, ...): the label may be
                // any constant of it. Only a constant missing from a *known*
                // enum is an error.
                SwitchLabel::SelectorUnknown => return,
                SwitchLabel::MissingFromKnownEnum => {}
            }
        }

        // Don't emit diagnostics for chain receivers — the receiver might be
        // a qualified name prefix (com.foo.Bar) or an external type.
        if is_chain_receiver {
            return;
        }

        // Unqualified members are looked up through the lexical receiver.
        // Use the same effective-member election as qualified navigation: a
        // raw inheritance walk can accept private or hidden ancestor fields,
        // while omitting inherited method calls altogether.
        if let Some(class_fqn) = self.get_enclosing_class_fqn(node) {
            match expected_namespace {
                Some(DeclarationNamespace::Value) | None => {
                    if let ProjectedFieldAccessResolution::Resolved(projected) =
                        self.workspace_index.resolve_projected_field_access(
                            &class_fqn,
                            &[],
                            &name,
                            &self.get_lexical_class_fqns(node),
                            FieldAccessQualifier::Expression,
                        )
                    {
                        self.bind_projected_field(ident_range, node.text_range(), projected);
                        return;
                    }
                }
                Some(DeclarationNamespace::Callable)
                    if !self
                        .workspace_index
                        .find_effective_methods_by_name(&class_fqn, &name)
                        .is_empty() =>
                {
                    return;
                }
                _ => {}
            }
        }

        // An unqualified name can come from places the index may not see:
        // a static import of an unindexed class, or a member inherited from
        // an unresolved supertype. Only a name that provably has nowhere left
        // to hide is an error.
        if self.unqualified_lookup_is_open(node, &name) {
            debug!(
                "SEMANTIC_WALKER: not reporting '{}': lookup depends on unresolved code",
                name
            );
            return;
        }

        let suggested_symbol = find_case_mismatch_suggestion(&name, self.imports);

        self.diagnostics.push(SemanticError::new(
            ident_range,
            SemanticErrorKind::UndefinedSymbol {
                symbol_name: name,
                suggested_symbol,
            },
        ));
    }

    /// True when a simple name that failed every lookup could still be
    /// supplied by code the index does not contain.
    fn unqualified_lookup_is_open(&self, node: &SyntaxNode, name: &str) -> bool {
        use crate::compile_checks::facts::{TypeFacts, TypeFactsCache};

        // Static imports: `import static x.Y.*` or `import static x.Y.name`
        // naming a class we cannot see may provide any member.
        if self
            .imports
            .static_wildcards
            .iter()
            .any(|owner| !self.workspace_index.has_class(owner))
        {
            return true;
        }
        if self.imports.static_imports_named(name).any(|fqn| {
            fqn.rsplit_once('.')
                .is_some_and(|(owner, _)| !self.workspace_index.has_class(owner))
        }) {
            return true;
        }

        // Members inherited through an unresolved supertype of any lexically
        // enclosing class; anonymous and local classes inherit from types
        // whose members we do not model here.
        let declares_local_type = node.ancestors().any(|ancestor| {
            // An anonymous class body is inline in NEW_EXPR, after the ARG_LIST.
            ancestor.kind() == SyntaxKind::NEW_EXPR
                && ancestor
                    .children_with_tokens()
                    .any(|element| element.kind() == SyntaxKind::L_BRACE)
        });
        if declares_local_type {
            return true;
        }
        // Pattern bindings (`o instanceof T t`, `case R(var x)`) have
        // flow-sensitive scope (JLS 6.3.1) that the scope builder only models
        // for the simplest shapes: `!(o instanceof T t) || t...` and
        // `if (!(o instanceof T t)) return; t...` are valid. A name bound by a
        // pattern anywhere in the enclosing member is never reported.
        if let Some(member) = node.ancestors().find(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::METHOD_DECL
                    | SyntaxKind::CONSTRUCTOR_DECL
                    | SyntaxKind::FIELD_DECL
                    | SyntaxKind::STATIC_INIT
                    | SyntaxKind::INSTANCE_INIT
            )
        }) {
            if member
                .descendants()
                .filter(|descendant| descendant.kind() == SyntaxKind::TYPE_PATTERN)
                .any(|pattern| {
                    pattern
                        .children_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                        .any(|token| token.kind().is_identifier() && token.text() == name)
                })
            {
                return true;
            }
        }
        let cache = TypeFactsCache::default();
        let facts = TypeFacts::new(self.workspace_index, &cache);
        // A class that declares no supertype clause inherits only from Object;
        // every other lexical owner needs its whole hierarchy resolved.
        let declarations = node.ancestors().filter(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::RECORD_DECL
                    | SyntaxKind::ANNOTATION_DECL
            ) && extract_type_decl_name(ancestor).is_some()
        });
        self.get_lexical_class_fqns(node)
            .iter()
            .zip(declarations)
            .any(|(owner, declaration)| {
                let declares_supertypes = declaration.children_with_tokens().any(|element| {
                    matches!(
                        element.kind(),
                        SyntaxKind::EXTENDS_KW | SyntaxKind::IMPLEMENTS_KW
                    )
                });
                declares_supertypes && facts.supertypes(owner).is_none()
            })
    }

    fn is_in_switch_label(node: &SyntaxNode) -> bool {
        let Some(case_clause) = node
            .ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::CASE_CLAUSE)
        else {
            return false;
        };
        case_clause
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .find(|token| matches!(token.kind(), SyntaxKind::COLON | SyntaxKind::ARROW))
            .is_some_and(|delimiter| node.text_range().end() <= delimiter.text_range().start())
    }

    fn bind_switch_enum_constant(
        &mut self,
        label: &SyntaxNode,
        name: &str,
        name_range: TextRange,
    ) -> SwitchLabel {
        let Some(case_clause) = label
            .ancestors()
            .find(|ancestor| ancestor.kind() == SyntaxKind::CASE_CLAUSE)
        else {
            return SwitchLabel::SelectorUnknown;
        };
        let Some(switch_node) = case_clause.ancestors().find(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::SWITCH_STMT | SyntaxKind::SWITCH_EXPR
            )
        }) else {
            return SwitchLabel::SelectorUnknown;
        };
        let first_case_start = switch_node
            .children()
            .find(|child| child.kind() == SyntaxKind::CASE_CLAUSE)
            .map_or(case_clause.text_range().start(), |child| {
                child.text_range().start()
            });
        let Some(selector) = switch_node.children().find(|child| {
            child.text_range().end() <= first_case_start
                && Self::is_switch_selector_expression(child.kind())
        }) else {
            return SwitchLabel::SelectorUnknown;
        };
        let Type::Class(selector_type) =
            self.inference_engine
                .infer_expression(&selector, self.symbol_resolver, None)
        else {
            return SwitchLabel::SelectorUnknown;
        };
        let lexical_owners = self.get_lexical_class_fqns(&switch_node);
        let TypeNameResolution::Exact(enum_fqn) = self.workspace_index.resolve_type_name(
            self.file_id,
            &lexical_owners,
            &selector_type.qualified_name(),
        ) else {
            return SwitchLabel::SelectorUnknown;
        };
        let enum_target = SymbolTarget::new(enum_fqn.as_str(), DeclarationNamespace::Type);
        if self
            .workspace_index
            .find_symbol_definition(&enum_target)
            .is_none_or(|definition| definition.symbol.kind != lsp_types::SymbolKind::ENUM)
        {
            return SwitchLabel::SelectorUnknown;
        }
        let member_target =
            SymbolTarget::new(format!("{enum_fqn}.{name}"), DeclarationNamespace::Value);
        let Some(definition) = self
            .workspace_index
            .find_symbol_definition(&member_target)
            .filter(|definition| definition.symbol.kind == lsp_types::SymbolKind::ENUM_MEMBER)
        else {
            return SwitchLabel::MissingFromKnownEnum;
        };
        let location = match definition.source {
            DefinitionSourceAuthority::Workspace(revision) => {
                Some(crate::symbol::SourceLocation::new(
                    revision.file_id(),
                    definition.location.range.start,
                    definition.location.range.end,
                ))
            }
            DefinitionSourceAuthority::External => None,
        };
        self.resolved_names.insert(
            name_range,
            ResolvedName::EnumConstant {
                enum_fqn,
                name: name.to_owned(),
                location,
            },
        );
        SwitchLabel::Bound
    }

    const fn is_switch_selector_expression(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::NAME_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::PAREN_EXPR
                | SyntaxKind::CAST_EXPR
                | SyntaxKind::ARRAY_ACCESS_EXPR
                | SyntaxKind::TERNARY_EXPR
        )
    }

    /// Bind a qualified value selector (`receiver.field`) to the exact field
    /// elected for the receiver type. Previously `FileSemantics` described only
    /// the receiver NAME_EXPR, so dependent edits could leave an apparently
    /// identical semantic result even when the selected field disappeared.
    fn check_field_access(&mut self, node: &SyntaxNode) {
        let Some(receiver) = node.children().next() else {
            return;
        };
        let Some(selector) = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier())
            .last()
        else {
            return;
        };
        let mut receiver_type =
            self.inference_engine
                .infer_expression(&receiver, self.symbol_resolver, None);
        // `Type.FIELD`: a receiver name that resolves to a type is the type itself.
        if receiver.kind() == SyntaxKind::NAME_EXPR {
            if let Some((_, ident_range)) = extract_name_expr_ident(&receiver) {
                if let Some(ResolvedName::Type { fqn, .. }) = self.resolved_names.get(&ident_range)
                {
                    let (package, name) = fqn
                        .rsplit_once('.')
                        .map_or((None, fqn.as_str()), |(package, name)| {
                            (Some(package.to_owned()), name)
                        });
                    receiver_type = Type::Class(ClassType::new(name.to_owned(), package));
                }
            }
        }
        let Type::Class(receiver_class) = &receiver_type else {
            return;
        };
        let receiver_fqn = receiver_class.qualified_name().into_owned();
        let receiver_arguments = receiver_class
            .type_arguments
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let lexical_access_owners = self.get_lexical_class_fqns(node);
        let is_super = receiver.kind() == SyntaxKind::SUPER_EXPR;
        let qualifier = if is_super {
            FieldAccessQualifier::Super
        } else {
            FieldAccessQualifier::Expression
        };
        let cache_key = FieldAccessCacheKey {
            receiver_fqn: receiver_fqn.clone(),
            receiver_arguments,
            member_name: selector.text().to_owned(),
            lexical_access_owners,
            is_super,
        };
        let resolution = if let Some(cached) = self.field_access_cache.get(&cache_key) {
            cached.clone()
        } else {
            let resolved = self.workspace_index.resolve_projected_field_access(
                &cache_key.receiver_fqn,
                &cache_key.receiver_arguments,
                &cache_key.member_name,
                &cache_key.lexical_access_owners,
                qualifier,
            );
            self.field_access_cache.insert(cache_key, resolved.clone());
            resolved
        };
        let projected = match resolution {
            ProjectedFieldAccessResolution::Resolved(projected) => projected,
            ProjectedFieldAccessResolution::Inaccessible => {
                self.reject_field_access(&selector, &receiver_fqn, NameRejectionKind::Inaccessible);
                self.report_inaccessible_field(node, &selector, &receiver_fqn);
                return;
            }
            ProjectedFieldAccessResolution::Ambiguous => {
                self.reject_field_access(&selector, &receiver_fqn, NameRejectionKind::Ambiguous);
                return;
            }
            ProjectedFieldAccessResolution::Missing => {
                if self.bind_nested_type_selector(&receiver, &selector) {
                    return;
                }
                self.reject_field_access(&selector, &receiver_fqn, NameRejectionKind::Missing);
                self.report_missing_field(node, &receiver, &selector, &receiver_fqn);
                return;
            }
        };
        self.bind_projected_field(selector.text_range(), node.text_range(), projected);
    }

    /// `x.secret` where `secret` is private (or package-private/protected) and
    /// the use site is outside its access scope.
    fn report_inaccessible_field(
        &mut self,
        node: &SyntaxNode,
        selector: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
        receiver_fqn: &str,
    ) {
        if crate::compile_checks::lexical::is_in_local_or_anonymous_type(node)
            || !node
                .children()
                .next()
                .is_some_and(|receiver| Self::receiver_is_spelled(&receiver))
        {
            return;
        }
        let Some(definition) = self
            .workspace_index
            .find_effective_field_definition_by_name(receiver_fqn, selector.text())
        else {
            return;
        };
        let symbol = &definition.symbol;
        let access = if symbol.modifiers.is_private {
            "private"
        } else if symbol.modifiers.is_protected {
            "protected"
        } else {
            "package-private"
        };
        let owner = symbol.container.as_deref().unwrap_or(receiver_fqn);
        self.diagnostics.push(SemanticError::new(
            selector.text_range(),
            SemanticErrorKind::InaccessibleMember {
                member_kind: "variable",
                member: selector.text().to_owned(),
                owner: owner
                    .split('.')
                    .skip_while(|segment| !segment.chars().next().is_some_and(char::is_uppercase))
                    .collect::<Vec<_>>()
                    .join("."),
                access,
            },
        ));
    }

    /// A receiver whose static type is spelled in the source (a variable, a
    /// `new`, a cast, a chain of field selectors on those) rather than derived
    /// through overload selection and generic inference.
    fn receiver_is_spelled(receiver: &SyntaxNode) -> bool {
        match receiver.kind() {
            SyntaxKind::NAME_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::LITERAL_EXPR => true,
            SyntaxKind::FIELD_ACCESS_EXPR => receiver
                .children()
                .next()
                .is_some_and(|base| Self::receiver_is_spelled(&base)),
            _ => false,
        }
    }

    /// `x.nothing` where the receiver's type provably has no such field.
    fn report_missing_field(
        &mut self,
        node: &SyntaxNode,
        receiver: &SyntaxNode,
        selector: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
        receiver_fqn: &str,
    ) {
        use crate::compile_checks::facts::{TypeFacts, TypeFactsCache};

        // `Outer.this` and friends carry a qualifier whose type inference may
        // report the innermost class instead.
        if matches!(
            receiver.kind(),
            SyntaxKind::THIS_EXPR | SyntaxKind::SUPER_EXPR
        ) && receiver.children().next().is_some()
        {
            return;
        }
        if receiver_fqn == "java.lang.Object"
            || !Self::receiver_is_spelled(receiver)
            || crate::compile_checks::lexical::is_in_local_or_anonymous_type(node)
        {
            return;
        }
        let cache = TypeFactsCache::default();
        let facts = TypeFacts::new(self.workspace_index, &cache);
        let Some(supertypes) = facts.supertypes(receiver_fqn) else {
            return;
        };
        if facts.is_foreign_language_type(receiver_fqn)
            || supertypes
                .iter()
                .any(|supertype| facts.is_foreign_language_type(supertype))
        {
            return;
        }
        // `Sub.Inherited`: a member type reached through a subclass.
        let name = selector.text();
        if supertypes.iter().any(|supertype| {
            self.workspace_index
                .has_class(&format!("{supertype}.{name}"))
        }) {
            return;
        }
        if crate::compile_checks::lombok::lombok_may_generate(&facts, receiver_fqn, name) {
            return;
        }
        self.diagnostics.push(SemanticError::new(
            selector.text_range(),
            SemanticErrorKind::FieldNotFound {
                name: name.to_owned(),
                receiver_type: receiver_fqn.to_owned(),
            },
        ));
    }

    fn bind_projected_field(
        &mut self,
        name_range: TextRange,
        expression_range: TextRange,
        projected: hyperion_workspace::ProjectedEffectiveField,
    ) {
        let field_type = projected
            .projected_type
            .as_deref()
            .and_then(crate::type_parser::parse_type_string)
            .unwrap_or(Type::Unknown);
        let definition = projected.definition;
        let Some(owner_fqn) = definition.symbol.container.as_ref() else {
            return;
        };
        let location = match definition.source {
            hyperion_workspace::DefinitionSourceAuthority::Workspace(revision) => {
                Some(crate::symbol::SourceLocation::new(
                    revision.file_id(),
                    definition.location.range.start,
                    definition.location.range.end,
                ))
            }
            hyperion_workspace::DefinitionSourceAuthority::External => None,
        };
        let modifier_prefix = workspace_value_modifier_prefix(&definition.symbol);
        self.resolved_names.insert(
            name_range,
            ResolvedName::Field {
                owner_fqn: owner_fqn.to_string(),
                name: definition.symbol.simple_name.to_string(),
                field_type: field_type.clone(),
                location,
                modifier_prefix,
            },
        );
        self.expression_types.insert(expression_range, field_type);
    }

    fn bind_nested_type_selector(
        &mut self,
        receiver: &SyntaxNode,
        selector: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
    ) -> bool {
        let Some(receiver_token) = receiver
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier())
            .last()
        else {
            return false;
        };
        let Some(receiver_fqn) = self
            .resolved_names
            .get(&receiver_token.text_range())
            .and_then(|resolved| match resolved {
                ResolvedName::Type { fqn, .. } | ResolvedName::External { fqn } => Some(fqn),
                _ => None,
            })
        else {
            return false;
        };
        let nested_fqn = format!("{receiver_fqn}.{}", selector.text());
        let target = hyperion_workspace::SymbolTarget::new(
            nested_fqn.as_str(),
            hyperion_workspace::DeclarationNamespace::Type,
        );
        let Some(definition) = self.workspace_index.find_symbol_definition(&target) else {
            return false;
        };
        let location = match definition.source {
            hyperion_workspace::DefinitionSourceAuthority::Workspace(revision) => {
                Some(crate::symbol::SourceLocation::new(
                    revision.file_id(),
                    definition.location.range.start,
                    definition.location.range.end,
                ))
            }
            hyperion_workspace::DefinitionSourceAuthority::External => None,
        };
        self.resolved_names.insert(
            selector.text_range(),
            ResolvedName::Type {
                fqn: nested_fqn,
                location,
            },
        );
        true
    }

    fn reject_field_access(
        &mut self,
        selector: &rowan::SyntaxToken<hyperion_parser::JavaLanguage>,
        receiver_fqn: &str,
        kind: NameRejectionKind,
    ) {
        self.resolved_names.insert(
            selector.text_range(),
            ResolvedName::Rejected(NameRejection {
                kind,
                receiver_fqn: receiver_fqn.to_owned(),
                name: selector.text().to_owned(),
            }),
        );
    }

    fn get_enclosing_class_fqn(&self, node: &SyntaxNode) -> Option<String> {
        self.get_lexical_class_fqns(node).into_iter().next()
    }

    fn get_lexical_class_fqns(&self, node: &SyntaxNode) -> Vec<String> {
        let mut enclosing_classes = Vec::new();

        for ancestor in node.ancestors() {
            if matches!(
                ancestor.kind(),
                SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::RECORD_DECL
                    | SyntaxKind::ANNOTATION_DECL
            ) {
                if let Some(class_name) = extract_type_decl_name(&ancestor) {
                    enclosing_classes.push(class_name);
                }
            }
        }

        if enclosing_classes.is_empty() {
            return Vec::new();
        }

        // Build every outer-to-inner class prefix, then return nearest-first.
        enclosing_classes.reverse();
        let mut class_fqn = self.imports.current_package.clone().unwrap_or_default();
        let mut owners = Vec::with_capacity(enclosing_classes.len());
        for class_name in enclosing_classes {
            if !class_fqn.is_empty() {
                class_fqn.push('.');
            }
            class_fqn.push_str(&class_name);
            owners.push(class_fqn.clone());
        }
        owners.reverse();
        owners
    }
}

fn workspace_value_modifier_prefix(symbol: &hyperion_workspace::Symbol) -> String {
    let mut modifiers = Vec::new();
    if symbol.modifiers.is_public {
        modifiers.push("public");
    } else if symbol.modifiers.is_protected {
        modifiers.push("protected");
    } else if symbol.modifiers.is_private {
        modifiers.push("private");
    } else if symbol.modifiers.is_internal {
        modifiers.push("internal");
    }
    if symbol.modifiers.is_static {
        modifiers.push("static");
    }
    if symbol.modifiers.is_final {
        modifiers.push("final");
    }
    if modifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", modifiers.join(" "))
    }
}

/// Extract the name from a type declaration (class, interface, enum, record, annotation).
fn extract_type_decl_name(node: &SyntaxNode) -> Option<String> {
    for elem in node.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            if token.kind().is_type_identifier() {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

/// Extract the name from a TYPE_PARAM node.
fn extract_type_param_name(node: &SyntaxNode) -> Option<String> {
    for elem in node.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            if token.kind().is_type_identifier() {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

/// Extract the parameter name from a PARAM node.
///
/// A parameter's declared type always lives in a child *node* (CLASS_TYPE,
/// PRIMITIVE_TYPE, ARRAY_TYPE), never in a direct IDENT token, so the first
/// IDENT token child is the parameter name. Requiring a preceding type node
/// would drop inferred lambda parameters such as `x` in `x -> x.trim()`,
/// which are parsed as a PARAM holding only an identifier.
fn extract_param_name(node: &SyntaxNode) -> Option<String> {
    // The last identifier token child, not the first: modifiers and `var` can
    // precede the name (`final String value`, `var x`), while the declared type
    // always lives in a child *node* and trailing `[]` are non-identifier
    // tokens. Requiring a preceding type node, as this did, dropped inferred
    // lambda parameters such as `x` in `x -> x.trim()`, which are parsed as a
    // PARAM holding only an identifier.
    let mut last_ident = None;
    for elem in node.children_with_tokens() {
        if let Some(token) = elem.into_token() {
            if token.kind().is_identifier() {
                last_ident = Some(token.text().to_string());
            }
        }
    }
    last_ident
}

/// True when `node` is the qualifier of a method reference, i.e. the `Foo` in
/// `Foo::bar` or `Foo::new`.
///
/// The qualifier is the first child of the reference expression and is
/// followed by `::`, so this cannot mistake any other operand for it.
fn is_method_reference_qualifier(node: &SyntaxNode) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if !matches!(
        parent.kind(),
        SyntaxKind::METHOD_REF_EXPR | SyntaxKind::CONSTRUCTOR_REF_EXPR
    ) {
        return false;
    }
    if parent.first_child().as_ref() != Some(node) {
        return false;
    }
    let mut next = node.next_sibling_or_token();
    while let Some(elem) = next {
        let Some(token) = elem.as_token() else {
            return false;
        };
        if !token.kind().is_trivia() {
            return token.kind() == SyntaxKind::COLON_COLON;
        }
        next = elem.next_sibling_or_token();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    #[test]
    fn imported_constants_have_semantic_bindings() {
        for declaration in [
            "public class Constants { public static final int FIRST = 1, VALUE = 7; }",
            "public @interface Constants { int VALUE = 7; }",
        ] {
            for import in ["p.Constants.VALUE", "p.Constants.*"] {
                let directory = tempfile::tempdir().unwrap();
                let index = WorkspaceIndex::new(directory.path().to_path_buf());
                let constants = format!("package p; {declaration}");
                index
                    .index_file_with_source(&directory.path().join("Constants.java"), &constants)
                    .unwrap();
                let source = format!("package q; import static {import}; class Use {{ int value() {{ return VALUE; }} }}");
                let path = directory.path().join("Use.java");
                index.index_file_with_source(&path, &source).unwrap();
                let file_id = index.get_file_id(&path).unwrap();
                let (green, errors) = Parser::parse(Lexer::new(&source).collect());
                assert!(errors.is_empty(), "{errors:?}");
                let root = SyntaxNode::new_root(green);
                let semantics = compute_file_semantics(&root, file_id, &SymbolTable::new(), &index);
                assert!(
                    semantics.diagnostics().is_empty(),
                    "{declaration} / {import}: {:?}",
                    semantics.diagnostics()
                );
                let usage = root
                    .descendants()
                    .find(|node| node.kind() == SyntaxKind::NAME_EXPR && node.text() == "VALUE")
                    .unwrap();
                assert!(matches!(
                    semantics.resolve_at(usage.text_range()),
                    Some(ResolvedName::Field { .. })
                ));
            }
        }
    }
}
