// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_parser::syntax::SyntaxKind;
use hyperion_workspace::WorkspaceIndex;
use tower_lsp::lsp_types::{Location, Position, Range, Url};
use tracing::debug;

/// Find the scope at a given byte offset
///
/// This uses the symbol table's scope range tracking to find the deepest scope
/// that contains the given offset.
pub fn find_scope_at_position(
    _root: &hyperion_parser::syntax::SyntaxNode,
    offset: u32,
    resolver: &hyperion_semantics::resolver::SymbolResolver,
) -> Option<hyperion_semantics::symbol::ScopeId> {
    resolver.find_scope_at_offset(offset)
}

fn method_signature(fqn: &str) -> Option<(&str, Vec<&str>)> {
    let (base, parameters) = fqn.split_once('(')?;
    let parameters = parameters.strip_suffix(')')?;
    if parameters.trim().is_empty() {
        return Some((base, Vec::new()));
    }

    let mut depth = 0_u32;
    let mut start = 0;
    let mut parts = Vec::new();
    for (offset, character) in parameters.char_indices() {
        match character {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(parameters[start..offset].trim());
                start = offset + character.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(parameters[start..].trim());
    Some((base, parts))
}

fn canonical_parameter_type(parameter: &str) -> String {
    let mut canonical = String::with_capacity(parameter.len());
    let mut generic_depth = 0_u32;
    for character in parameter.trim().chars() {
        match character {
            '<' => generic_depth += 1,
            '>' => generic_depth = generic_depth.saturating_sub(1),
            '$' if generic_depth == 0 => canonical.push('.'),
            character if generic_depth == 0 && !character.is_whitespace() => {
                canonical.push(character);
            }
            _ => {}
        }
    }
    if let Some(base) = canonical.strip_suffix("...") {
        format!("{base}[]")
    } else {
        canonical
    }
}

fn array_shape(parameter: &str) -> (&str, usize) {
    let mut base = parameter;
    let mut dimensions = 0;
    while let Some(stripped) = base.strip_suffix("[]") {
        base = stripped;
        dimensions += 1;
    }
    (base, dimensions)
}

fn parameter_types_are_source_compatible(requested: &str, candidate: &str) -> bool {
    if requested == candidate {
        return true;
    }
    let (requested_base, requested_dimensions) = array_shape(requested);
    let (candidate_base, candidate_dimensions) = array_shape(candidate);
    if requested_dimensions != candidate_dimensions {
        return false;
    }

    // Source signatures commonly use imported simple names while bytecode uses
    // qualified descriptors. Ambiguous simple-name matches are rejected by the
    // caller, which considers every same-arity candidate before selecting one.
    if (!requested_base.contains('.') || !candidate_base.contains('.'))
        && requested_base.rsplit('.').next() == candidate_base.rsplit('.').next()
    {
        return true;
    }

    // The Java source extractor retains type-variable names while bytecode
    // exposes their erasure. Without bound metadata, only the JLS default bound
    // (`Object`) is provable. Never use arity alone to guess a different type.
    candidate_base.len() == 1
        && candidate_base
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase())
        && matches!(requested_base, "Object" | "java.lang.Object")
}

fn find_extracted_symbol<'a>(
    symbols: &'a [crate::source_provider::content::SourceSymbol],
    requested_fqn: &str,
) -> Option<&'a crate::source_provider::content::SourceSymbol> {
    if let Some(exact) = symbols
        .iter()
        .find(|symbol| symbol.fqn.as_ref() == requested_fqn)
    {
        return Some(exact);
    }

    let Some((requested_base, requested_parameters)) = method_signature(requested_fqn) else {
        let mut candidates = symbols.iter().filter(|symbol| {
            method_signature(&symbol.fqn)
                .is_some_and(|(candidate_base, _)| candidate_base == requested_fqn)
        });
        let candidate = candidates.next();
        return candidates.next().is_none().then_some(candidate).flatten();
    };
    let requested_parameters: Vec<_> = requested_parameters
        .into_iter()
        .map(canonical_parameter_type)
        .collect();
    let mut compatible_match = None;

    for symbol in symbols {
        let Some((candidate_base, candidate_parameters)) = method_signature(&symbol.fqn) else {
            continue;
        };
        if candidate_base != requested_base
            || candidate_parameters.len() != requested_parameters.len()
        {
            continue;
        }
        let candidate_parameters: Vec<_> = candidate_parameters
            .into_iter()
            .map(canonical_parameter_type)
            .collect();
        if candidate_parameters
            .iter()
            .zip(&requested_parameters)
            .all(|(candidate, requested)| {
                parameter_types_are_source_compatible(requested, candidate)
            })
        {
            if compatible_match.is_some() {
                return None;
            }
            compatible_match = Some(symbol);
        }
    }

    compatible_match
}

/// Convert LSP Position to byte offset
///
/// # Arguments
///
/// * `source` - The source text
/// * `position` - The LSP position (line, character in UTF-16 units)
///
/// # Returns
///
/// The byte offset corresponding to the position
///
/// # Note
///
/// This function is re-exported for backwards compatibility.
/// New code should use `crate::position_utils::position_to_offset` directly.
pub fn position_to_offset(
    source: &str,
    position: Position,
) -> Result<u32, crate::position_utils::PositionError> {
    crate::position_utils::position_to_offset_u32(source, position)
}

/// Convert byte offset to LSP Position
///
/// # Arguments
///
/// * `source` - The source text
/// * `offset` - The byte offset
///
/// # Returns
///
/// The LSP Position (line, character in UTF-16 units)
///
/// # Note
///
/// This function is re-exported for backwards compatibility.
/// New code should use `crate::position_utils::offset_to_position` directly.
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    crate::position_utils::offset_to_position(source, offset)
}

/// Extract the package declaration from Java/Kotlin source text.
///
/// Scans line-by-line for a `package ...;` declaration, stopping early
/// if imports are encountered first.
pub fn extract_package_from_source(source: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("package ") {
            let pkg = trimmed
                .strip_prefix("package ")?
                .trim_end_matches(';')
                .trim()
                .to_string();
            return Some(pkg);
        }
        if trimmed.starts_with("import ") {
            break;
        }
    }
    None
}

/// Find the simple name of the first class declaration in a syntax tree.
///
/// Searches descendants for the first CLASS_DECL node and extracts its IDENT token.
/// Returns the simple class name (e.g., "User"), not a fully qualified name.
pub fn find_first_class_name(root: &hyperion_parser::syntax::SyntaxNode) -> Option<String> {
    for node in root.descendants() {
        if node.kind() == SyntaxKind::CLASS_DECL {
            for child in node.children_with_tokens() {
                if let Some(token) = child.as_token() {
                    if token.kind() == SyntaxKind::IDENT {
                        return Some(token.text().to_string());
                    }
                }
            }
        }
    }
    None
}

/// Convert a workspace `SymbolLocation` to an LSP `Location` for workspace (non-external) files.
///
/// Converts only against the exact source revision retained by the workspace
/// index. Missing source bytes or an invalid range fail closed; a navigation
/// request must never manufacture a plausible-looking `(0, 0)` destination.
///
/// External/JDK symbols are converted only when a matching source provider is
/// supplied and the retained location still owns the elected typed definition.
pub fn symbol_location_to_lsp_location(
    workspace_index: &WorkspaceIndex,
    symbol_loc: &hyperion_workspace::SymbolLocation,
    source_providers: Option<&crate::source_provider::SourceProviderChain>,
) -> Result<Option<Location>, String> {
    if workspace_index.is_external_file(symbol_loc.file_id) {
        let Some(proof) = ExternalNavigationProof::capture(workspace_index, symbol_loc) else {
            debug!(
                "External symbol {} is no longer the elected typed definition",
                symbol_loc.fqn
            );
            return Ok(None);
        };
        return external_symbol_location_to_lsp_location(
            workspace_index,
            symbol_loc,
            &proof,
            source_providers,
        );
    }

    let Some(revisioned) = workspace_index.revisioned_location(symbol_loc) else {
        // Stored usages, deserialized locations, and locally constructed
        // locations deliberately have no query provenance. Never infer it by
        // comparing their fields to the current index.
        return Ok(None);
    };
    revisioned_symbol_location_to_lsp_location(workspace_index, &revisioned)
}

fn external_symbol_location_to_lsp_location(
    workspace_index: &WorkspaceIndex,
    symbol_loc: &hyperion_workspace::SymbolLocation,
    proof: &ExternalNavigationProof,
    source_providers: Option<&crate::source_provider::SourceProviderChain>,
) -> Result<Option<Location>, String> {
    let Some(providers) = source_providers else {
        debug!(
            "No source providers for external symbol: {}",
            symbol_loc.fqn
        );
        return Ok(None);
    };

    let provided = providers.get_source_exact_for_revision(
        proof.source_identity(),
        proof.source_revision(),
        &symbol_loc.fqn,
    );
    // No source anywhere (a JDK without src.zip, a jar without -sources): show
    // a signatures-only stub of the class file instead of nothing.
    let Some((source_content, provider_name)) = provided.or_else(|| {
        let archive = proof.source_identity().archive_path()?;
        crate::source_provider::stub_source_content(
            &providers.materialized_sources(),
            archive,
            &symbol_loc.fqn,
        )
        .map(|stub| (stub, "bytecode stub"))
    }) else {
        debug!(
            "No source available for external symbol {} in {:?}",
            symbol_loc.fqn,
            proof.source_identity()
        );
        return Ok(None);
    };

    debug!(
        "Got source for {} from {} provider",
        symbol_loc.fqn, provider_name
    );

    let uri = source_content
        .uri()
        .map_err(|e| format!("Failed to get URI for source: {e}"))?;

    let text_content = source_content.content();

    match source_content.symbols() {
        Ok(symbols) => {
            if let Some(symbol) = find_extracted_symbol(symbols, &symbol_loc.fqn) {
                let Some(range) =
                    byte_range_to_lsp_range(text_content, symbol.range.start, symbol.range.end)
                else {
                    return Ok(None);
                };

                if !proof.is_current(workspace_index) {
                    debug!(
                        "External symbol {} changed while its source was loading",
                        symbol_loc.fqn
                    );
                    return Ok(None);
                }
                return Ok(Some(Location { uri, range }));
            }
            debug!("Symbol {} not found in extracted source", symbol_loc.fqn);
        }
        Err(e) => {
            debug!(
                "Failed to parse extracted source for {}: {}",
                symbol_loc.fqn, e
            );
        }
    }
    Ok(None)
}

/// Proof that one retained external location is the currently elected
/// declaration in its semantic namespace.
///
/// Query locations carry the exact external generation that owned their range.
/// The proof combines that token with a typed, atomically resolved definition
/// and revalidates the same election after provider I/O, so a retained R1
/// location cannot silently bind to a same-shaped R2 publication.
pub(crate) struct ExternalNavigationProof {
    definition: hyperion_workspace::ResolvedDefinition,
    source_identity: hyperion_workspace::ExternalSourceIdentity,
    source_revision: hyperion_workspace::ExternalSourceRevisionToken,
}

impl ExternalNavigationProof {
    pub(crate) fn capture(
        workspace_index: &WorkspaceIndex,
        location: &hyperion_workspace::SymbolLocation,
    ) -> Option<Self> {
        let source_revision = workspace_index.external_source_revision(location)?;
        let target = hyperion_workspace::SymbolTarget::new(
            location.fqn.clone(),
            hyperion_workspace::DeclarationNamespace::for_kind(location.kind),
        );
        let definition = workspace_index.find_symbol_definition(&target)?;
        if definition.source != hyperion_workspace::DefinitionSourceAuthority::External
            || !same_symbol_location(&definition.location, location)
        {
            return None;
        }
        let source_identity =
            workspace_index.get_external_source_identity(definition.location.file_id)?;
        let proof = Self {
            definition,
            source_identity,
            source_revision,
        };
        proof.is_current(workspace_index).then_some(proof)
    }

    pub(crate) const fn source_identity(&self) -> &hyperion_workspace::ExternalSourceIdentity {
        &self.source_identity
    }

    pub(crate) const fn source_revision(&self) -> hyperion_workspace::ExternalSourceRevisionToken {
        self.source_revision
    }

    pub(crate) fn is_current(&self, workspace_index: &WorkspaceIndex) -> bool {
        let Some(current) = workspace_index.find_symbol_definition(&self.definition.target) else {
            return false;
        };
        if current.source != hyperion_workspace::DefinitionSourceAuthority::External
            || current.workspace_revision != self.definition.workspace_revision
            || workspace_index.external_source_revision(&current.location)
                != Some(self.source_revision)
            || !same_symbol_location(&current.location, &self.definition.location)
            || workspace_index
                .get_external_source_identity(current.location.file_id)
                .as_ref()
                != Some(&self.source_identity)
        {
            return false;
        }

        // The definition and identity are read under separate index locks. A
        // final revision check closes that interval and is the linearization
        // point immediately before returning a navigation result.
        workspace_index.current_workspace_revision() == self.definition.workspace_revision
    }
}

fn same_symbol_location(
    left: &hyperion_workspace::SymbolLocation,
    right: &hyperion_workspace::SymbolLocation,
) -> bool {
    left.file_id == right.file_id
        && left.range == right.range
        && left.kind == right.kind
        && left.name == right.name
        && left.fqn == right.fqn
        && left.container_name == right.container_name
}

pub(crate) fn revisioned_symbol_location_to_lsp_location(
    workspace_index: &WorkspaceIndex,
    revisioned: &hyperion_workspace::RevisionedLocation,
) -> Result<Option<Location>, String> {
    let symbol_loc = &revisioned.location;
    if workspace_index.is_external_file(symbol_loc.file_id)
        || revisioned.revision.file_id() != symbol_loc.file_id
    {
        return Ok(None);
    }

    let Some(indexed) = workspace_index.get_indexed_revision_content(revisioned.revision) else {
        return Ok(None);
    };
    let Ok(uri) = Url::from_file_path(&indexed.file_path) else {
        debug!("Failed to convert path to URI: {:?}", indexed.file_path);
        return Ok(None);
    };

    let Some(range) = byte_range_to_lsp_range(
        &indexed.content,
        symbol_loc.range.start,
        symbol_loc.range.end,
    ) else {
        debug!(
            "Invalid indexed range {}..{} for {}",
            symbol_loc.range.start, symbol_loc.range.end, symbol_loc.fqn
        );
        return Ok(None);
    };

    Ok(Some(Location { uri, range }))
}

/// Convert a half-open UTF-8 byte range without clamping invalid offsets.
pub(crate) fn byte_range_to_lsp_range(source: &str, start: u32, end: u32) -> Option<Range> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    if start > end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    Some(Range::new(
        offset_to_position(source, start),
        offset_to_position(source, end),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_provider::{
        ExternalSourceRequest, SourceContent, SourceFile, SourceProvider, SourceProviderChain,
    };
    use hyperion_workspace::{
        ExternalSourceIdentity, ExternalSourceRevisionToken, Modifiers, Symbol, TextRange,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use tower_lsp::lsp_types::{Position, SymbolKind};

    #[derive(Clone)]
    struct IdentitySourceProvider {
        source_identity: ExternalSourceIdentity,
        priority: u32,
        content: SourceContent,
    }

    #[derive(Clone)]
    struct MutatingIdentitySourceProvider {
        workspace_index: Arc<WorkspaceIndex>,
        source_identity: ExternalSourceIdentity,
        content: SourceContent,
    }

    impl SourceProvider for IdentitySourceProvider {
        fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
            (request.source_identity() == &self.source_identity && request.fqn() == "dep.Duplicate")
                .then(|| self.content.clone())
        }

        fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
            request.source_identity() == &self.source_identity && request.fqn() == "dep.Duplicate"
        }

        fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
            source_identity == &self.source_identity
        }

        fn snapshot_for_sources(
            &self,
            source_identities: &std::collections::HashSet<ExternalSourceIdentity>,
        ) -> Option<Arc<dyn SourceProvider>> {
            source_identities
                .contains(&self.source_identity)
                .then(|| Arc::new(self.clone()) as Arc<dyn SourceProvider>)
        }

        fn priority(&self) -> u32 {
            self.priority
        }

        fn name(&self) -> &'static str {
            "identity-test"
        }
    }

    impl SourceProvider for MutatingIdentitySourceProvider {
        fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
            if request.source_identity() != &self.source_identity
                || request.fqn() != "dep.Duplicate"
            {
                return None;
            }
            self.workspace_index.add_external_symbol(Symbol {
                fqn: "dep.ConcurrentRefresh".into(),
                simple_name: "ConcurrentRefresh".into(),
                range: TextRange::new(0, 0),
                kind: SymbolKind::CLASS,
                modifiers: Modifiers::default(),
                container: None,
                return_type: None,
                parameters: Vec::new(),
                parent_class: None,
                interfaces: Vec::new(),
                documentation: None,
                type_parameters: Vec::new(),
                is_deprecated: false,
                receiver_type: None,
                kotlin: None,
                throws: None,
            });
            Some(self.content.clone())
        }

        fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
            request.source_identity() == &self.source_identity && request.fqn() == "dep.Duplicate"
        }

        fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
            source_identity == &self.source_identity
        }

        fn snapshot_for_sources(
            &self,
            source_identities: &std::collections::HashSet<ExternalSourceIdentity>,
        ) -> Option<Arc<dyn SourceProvider>> {
            source_identities
                .contains(&self.source_identity)
                .then(|| Arc::new(self.clone()) as Arc<dyn SourceProvider>)
        }

        fn priority(&self) -> u32 {
            0
        }

        fn name(&self) -> &'static str {
            "mutating-identity-test"
        }
    }

    fn duplicate_symbol() -> Symbol {
        Symbol {
            fqn: "dep.Duplicate".into(),
            simple_name: "Duplicate".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        }
    }

    fn publish_revisioned_provider(
        index: &WorkspaceIndex,
        chain: &SourceProviderChain,
        owner: &str,
        priority: u32,
        source: &PathBuf,
        source_identity: &ExternalSourceIdentity,
        provider: Arc<dyn SourceProvider>,
        symbols: Vec<Symbol>,
    ) -> ExternalSourceRevisionToken {
        let ticket = index.begin_external_source_refresh(owner);
        assert!(chain.observe_group_refresh(&ticket));
        let publication = chain
            .begin_group_publication(
                &ticket,
                std::slice::from_ref(source_identity),
                Some(provider),
            )
            .expect("provider publication");
        let refresh = index
            .replace_external_sources_with_priority_for_refresh(
                &ticket,
                priority,
                std::slice::from_ref(source),
            )
            .expect("workspace publication");
        let revision = refresh
            .revision_for_identity(source_identity)
            .expect("source revision");
        assert!(index.add_external_symbols_responsive_for_revision(symbols, revision));
        assert!(chain.record_group_publication_success_for_revision(
            &publication,
            source_identity,
            revision,
        ));
        assert!(chain.finish_group_publication(&publication));
        revision
    }

    #[test]
    fn workspace_query_location_converts_without_compatibility_lookup() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Ordinary.java");
        let source = "class Ordinary {}";
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(&path, source)
            .expect("publication");
        let queried = index.find_symbol("Ordinary").expect("queried definition");

        let location = symbol_location_to_lsp_location(&index, &queried, None)
            .expect("conversion")
            .expect("ordinary workspace definition remains navigable");
        assert_eq!(location.uri, Url::from_file_path(path).expect("URI"));
        assert_eq!(location.range.start, Position::new(0, 6));
    }

    #[test]
    fn external_navigation_uses_the_workspace_selected_source_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let wrong_path = temp.path().join("wrong/Duplicate.java");
        let selected_path = temp.path().join("selected/Duplicate.java");
        std::fs::create_dir_all(wrong_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(selected_path.parent().unwrap()).unwrap();
        let wrong_source = "package dep; public class Duplicate { String owner = \"wrong\"; }";
        let selected_source =
            "package dep; public class Duplicate { String owner = \"selected\"; }";
        std::fs::write(&wrong_path, wrong_source).unwrap();
        std::fs::write(&selected_path, selected_source).unwrap();
        let providers = SourceProviderChain::new();
        providers.register(Arc::new(IdentitySourceProvider {
            source_identity: ExternalSourceIdentity::for_virtual("different-archive"),
            priority: 0,
            content: SourceContent::Stub {
                source: Arc::new(SourceFile::new(Arc::from(wrong_source))),
                temp_file: wrong_path,
            },
        }));
        let source_id = PathBuf::from(crate::source_provider::jdk::JDK_SOURCE_ID);
        let selected_identity =
            ExternalSourceIdentity::for_virtual(crate::source_provider::jdk::JDK_SOURCE_ID);
        publish_revisioned_provider(
            &index,
            &providers,
            "selected-provider",
            0,
            &source_id,
            &selected_identity,
            Arc::new(IdentitySourceProvider {
                source_identity: selected_identity.clone(),
                priority: 10,
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(selected_source))),
                    temp_file: selected_path.clone(),
                },
            }),
            vec![duplicate_symbol()],
        );
        let symbol = index.find_symbol("dep.Duplicate").expect("external symbol");
        assert_eq!(
            index.get_external_source_name(symbol.file_id).as_deref(),
            Some(crate::source_provider::jdk::JDK_SOURCE_ID)
        );

        let location = symbol_location_to_lsp_location(&index, &symbol, Some(&providers))
            .expect("conversion")
            .expect("selected attachment");
        assert_eq!(location.uri, Url::from_file_path(selected_path).unwrap());
    }

    /// A provider that owns an archive identity but has no source for it: the
    /// situation of a library jar published without a `-sources` archive.
    #[derive(Clone)]
    struct SourcelessProvider {
        source_identity: ExternalSourceIdentity,
    }

    impl SourceProvider for SourcelessProvider {
        fn owns_source_identity(&self, source_identity: &ExternalSourceIdentity) -> bool {
            source_identity == &self.source_identity
        }

        fn snapshot_for_sources(
            &self,
            _source_identities: &std::collections::HashSet<ExternalSourceIdentity>,
        ) -> Option<Arc<dyn SourceProvider>> {
            Some(Arc::new(self.clone()))
        }

        fn priority(&self) -> u32 {
            20
        }

        fn name(&self) -> &'static str {
            "sourceless-test"
        }
    }

    fn member_symbol(
        fqn: &str,
        simple_name: &str,
        kind: SymbolKind,
        container: Option<&str>,
    ) -> Symbol {
        Symbol {
            fqn: fqn.into(),
            simple_name: simple_name.into(),
            kind,
            container: container.map(Into::into),
            ..duplicate_symbol()
        }
    }

    /// Go-to-definition into a jar (or a JDK) without sources used
    /// to answer "No definition found"; it opens a signatures-only stub of the
    /// class file, at the member when one was asked for.
    #[test]
    fn library_without_sources_navigates_to_a_class_file_stub() {
        let javac_available = std::process::Command::new("javac")
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !javac_available {
            eprintln!("SKIP: no javac");
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let classes = temp.path().join("classes");
        std::fs::create_dir_all(&classes).unwrap();
        std::fs::write(
            classes.join("Widget.java"),
            "package dep;\npublic class Widget<T> {\n    public T render(int width, String label) { return null; }\n}\n",
        )
        .unwrap();
        let compiled = std::process::Command::new("javac")
            .arg("-g")
            .arg("-d")
            .arg(&classes)
            .arg(classes.join("Widget.java"))
            .output()
            .expect("javac");
        assert!(compiled.status.success());
        let jar = temp.path().join("widget.jar");
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&jar).unwrap());
            zip.start_file("dep/Widget.class", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&std::fs::read(classes.join("dep/Widget.class")).unwrap())
                .unwrap();
            zip.finish().unwrap();
        }

        let index = WorkspaceIndex::new(temp.path().join("workspace"));
        let providers = SourceProviderChain::new();
        let identity = ExternalSourceIdentity::for_archive_path(&jar);
        publish_revisioned_provider(
            &index,
            &providers,
            "test-sourceless-jar",
            0,
            &jar,
            &identity,
            Arc::new(SourcelessProvider {
                source_identity: identity.clone(),
            }),
            vec![
                member_symbol("dep.Widget", "Widget", SymbolKind::CLASS, None),
                member_symbol(
                    "dep.Widget.render(int,String)",
                    "render",
                    SymbolKind::METHOD,
                    Some("dep.Widget"),
                ),
            ],
        );

        let class = index.find_symbol("dep.Widget").expect("class symbol");
        let class_location = symbol_location_to_lsp_location(&index, &class, Some(&providers))
            .expect("conversion")
            .expect("a stub must be served for a class without sources");
        let stub = std::fs::read_to_string(class_location.uri.to_file_path().unwrap()).unwrap();
        assert!(stub.contains("public class Widget<T>"), "{stub}");

        let method = index
            .find_symbol("dep.Widget.render(int,String)")
            .expect("method symbol");
        let method_location = symbol_location_to_lsp_location(&index, &method, Some(&providers))
            .expect("conversion")
            .expect("the member is located inside the stub");
        let line = stub
            .lines()
            .nth(method_location.range.start.line as usize)
            .unwrap();
        assert!(
            line.contains("render(int width, java.lang.String label)"),
            "{line}"
        );
    }

    #[test]
    fn external_navigation_fails_closed_while_its_provider_generation_is_partial() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source_id = PathBuf::from(crate::source_provider::jdk::JDK_SOURCE_ID);
        let publication_identity =
            ExternalSourceIdentity::for_virtual(crate::source_provider::jdk::JDK_SOURCE_ID);
        let chain = SourceProviderChain::new();
        let source_path = temp.path().join("selected/Duplicate.java");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let source = "package dep; public class Duplicate {}";
        std::fs::write(&source_path, source).unwrap();

        publish_revisioned_provider(
            &index,
            &chain,
            "test-external-navigation",
            100,
            &source_id,
            &publication_identity,
            Arc::new(IdentitySourceProvider {
                source_identity: publication_identity.clone(),
                priority: 0,
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(source))),
                    temp_file: source_path,
                },
            }),
            vec![duplicate_symbol()],
        );
        let symbol = index.find_symbol("dep.Duplicate").unwrap();
        assert!(
            symbol_location_to_lsp_location(&index, &symbol, Some(&chain))
                .unwrap()
                .is_some()
        );

        let r2 = index.begin_external_source_refresh("test-external-navigation");
        assert!(chain.observe_group_refresh(&r2));
        assert!(chain
            .begin_group_publication(&r2, std::slice::from_ref(&publication_identity), None)
            .is_some());
        let _ = index
            .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source_id))
            .expect("R2 classpath commit");

        assert!(
            symbol_location_to_lsp_location(&index, &symbol, Some(&chain))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn failed_successor_finalization_restores_last_good_external_navigation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let source_id = PathBuf::from(crate::source_provider::jdk::JDK_SOURCE_ID);
        let source_identity =
            ExternalSourceIdentity::for_virtual(crate::source_provider::jdk::JDK_SOURCE_ID);
        let chain = SourceProviderChain::new();
        let source_path = temp.path().join("selected/Duplicate.java");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let source = "package dep; public class Duplicate {}";
        std::fs::write(&source_path, source).unwrap();

        let r1_revision = publish_revisioned_provider(
            &index,
            &chain,
            "failed-successor-navigation",
            100,
            &source_id,
            &source_identity,
            Arc::new(IdentitySourceProvider {
                source_identity: source_identity.clone(),
                priority: 0,
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(source))),
                    temp_file: source_path.clone(),
                },
            }),
            vec![duplicate_symbol()],
        );

        let r2 = index.begin_external_source_refresh("failed-successor-navigation");
        assert!(chain.observe_group_refresh(&r2));
        let r2_publication = chain
            .begin_group_publication(&r2, std::slice::from_ref(&source_identity), None)
            .expect("R2 provider publication");
        let r2_refresh = index
            .replace_external_sources_for_refresh(&r2, std::slice::from_ref(&source_id))
            .expect("R2 reservation");
        let r2_revision = r2_refresh
            .revision_for_identity(&source_identity)
            .expect("R2 reserved revision");
        assert_ne!(r1_revision, r2_revision);

        assert!(chain.finish_group_publication(&r2_publication));
        let last_good = index
            .find_symbol("dep.Duplicate")
            .expect("R1 remains the freshly queried last-good definition");
        assert_eq!(
            index.external_source_revision(&last_good),
            Some(r1_revision)
        );
        let location = symbol_location_to_lsp_location(&index, &last_good, Some(&chain))
            .expect("last-good conversion")
            .expect("R1 provider route remains navigable");
        assert_eq!(location.uri, Url::from_file_path(source_path).unwrap());
    }

    #[test]
    fn external_navigation_rechecks_provenance_after_provider_io() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        let source_path = temp.path().join("Duplicate.java");
        let source = "package dep; public class Duplicate {}";
        std::fs::write(&source_path, source).expect("materialize source fixture");
        let chain = SourceProviderChain::new();
        let source_id = PathBuf::from(crate::source_provider::jdk::JDK_SOURCE_ID);
        let source_identity =
            ExternalSourceIdentity::for_virtual(crate::source_provider::jdk::JDK_SOURCE_ID);
        publish_revisioned_provider(
            &index,
            &chain,
            "mutating-provider",
            100,
            &source_id,
            &source_identity,
            Arc::new(MutatingIdentitySourceProvider {
                workspace_index: Arc::clone(&index),
                source_identity: source_identity.clone(),
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(source))),
                    temp_file: source_path,
                },
            }),
            vec![duplicate_symbol()],
        );
        let symbol = index.find_symbol("dep.Duplicate").expect("external symbol");

        assert!(
            symbol_location_to_lsp_location(&index, &symbol, Some(&chain))
                .expect("conversion")
                .is_none(),
            "a source loaded across an external-index revision boundary must be discarded"
        );
    }

    #[test]
    fn retained_external_location_is_rejected_after_workspace_promotion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let external_source_path = temp.path().join("external/Duplicate.java");
        std::fs::create_dir_all(external_source_path.parent().unwrap()).unwrap();
        let external_source = "package dep; public class Duplicate {}";
        std::fs::write(&external_source_path, external_source).unwrap();
        let providers = SourceProviderChain::new();
        let source_id = PathBuf::from(crate::source_provider::jdk::JDK_SOURCE_ID);
        let source_identity =
            ExternalSourceIdentity::for_virtual(crate::source_provider::jdk::JDK_SOURCE_ID);
        publish_revisioned_provider(
            &index,
            &providers,
            "workspace-promotion",
            100,
            &source_id,
            &source_identity,
            Arc::new(IdentitySourceProvider {
                source_identity: source_identity.clone(),
                priority: 0,
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(external_source))),
                    temp_file: external_source_path,
                },
            }),
            vec![duplicate_symbol()],
        );
        let retained = index
            .find_symbol("dep.Duplicate")
            .expect("R1 external definition");
        assert!(
            symbol_location_to_lsp_location(&index, &retained, Some(&providers))
                .expect("R1 conversion")
                .is_some()
        );

        let workspace_path = temp.path().join("src/dep/Duplicate.java");
        index
            .index_file_with_source(&workspace_path, external_source)
            .expect("workspace definition");
        let current = index
            .find_symbol_definition(&hyperion_workspace::SymbolTarget::new(
                "dep.Duplicate",
                hyperion_workspace::DeclarationNamespace::Type,
            ))
            .expect("promoted workspace definition");
        assert_eq!(
            current.source,
            hyperion_workspace::DefinitionSourceAuthority::Workspace(
                current.revision().expect("workspace revision")
            )
        );

        assert!(
            symbol_location_to_lsp_location(&index, &retained, Some(&providers))
                .expect("stale conversion")
                .is_none(),
            "an R1 external range must not survive workspace-source promotion"
        );
    }

    #[test]
    fn retained_external_location_is_rejected_after_dependency_promotion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let low_path = temp.path().join("low.jar");
        let low_identity = ExternalSourceIdentity::for_archive_path(&low_path);
        let low_source_path = temp.path().join("low-source/Duplicate.java");
        std::fs::create_dir_all(low_source_path.parent().unwrap()).unwrap();
        let low_source = "package dep; public class Duplicate {}";
        std::fs::write(&low_source_path, low_source).unwrap();
        let providers = SourceProviderChain::new();
        publish_revisioned_provider(
            &index,
            &providers,
            "low-dependency",
            100,
            &low_path,
            &low_identity,
            Arc::new(IdentitySourceProvider {
                source_identity: low_identity.clone(),
                priority: 0,
                content: SourceContent::Stub {
                    source: Arc::new(SourceFile::new(Arc::from(low_source))),
                    temp_file: low_source_path,
                },
            }),
            vec![duplicate_symbol()],
        );
        let retained = index
            .find_symbol("dep.Duplicate")
            .expect("R1 low-priority definition");
        assert!(
            symbol_location_to_lsp_location(&index, &retained, Some(&providers))
                .expect("R1 conversion")
                .is_some()
        );

        let high_path = temp.path().join("high.jar");
        let high_refresh = index.replace_external_sources_with_priority_revisioned(
            "high-dependency",
            0,
            std::slice::from_ref(&high_path),
        );
        let high_revision = high_refresh
            .revision_for(&high_path)
            .expect("high dependency revision");
        assert!(index.add_external_symbols_responsive_for_revision(
            vec![Symbol {
                fqn: "dep.Duplicate".into(),
                simple_name: "Duplicate".into(),
                range: TextRange::new(0, 0),
                kind: SymbolKind::CLASS,
                modifiers: Modifiers::default(),
                container: None,
                return_type: None,
                parameters: Vec::new(),
                parent_class: None,
                interfaces: Vec::new(),
                documentation: None,
                type_parameters: Vec::new(),
                is_deprecated: false,
                receiver_type: None,
                kotlin: None,
                throws: None,
            }],
            high_revision,
        ));
        assert_ne!(
            index
                .find_symbol("dep.Duplicate")
                .expect("promoted dependency")
                .file_id,
            retained.file_id
        );

        assert!(
            symbol_location_to_lsp_location(&index, &retained, Some(&providers))
                .expect("stale conversion")
                .is_none(),
            "a higher-precedence dependency must retire the prior external navigation target"
        );
    }

    #[test]
    fn retained_external_location_is_rejected_after_same_archive_refresh() {
        let temp = tempfile::tempdir().expect("tempdir");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        let archive = temp.path().join("dependency.jar");
        let symbol = || Symbol {
            fqn: "dep.Duplicate".into(),
            simple_name: "Duplicate".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        };

        let first = index.replace_external_sources_with_priority_revisioned(
            "dependency",
            0,
            std::slice::from_ref(&archive),
        );
        let first_revision = first.revision_for(&archive).expect("first revision");
        assert!(index.add_external_symbols_responsive_for_revision(vec![symbol()], first_revision));
        let retained = index.find_symbol("dep.Duplicate").expect("R1 symbol");

        let second = index.replace_external_sources_with_priority_revisioned(
            "dependency",
            0,
            std::slice::from_ref(&archive),
        );
        let second_revision = second.revision_for(&archive).expect("second revision");
        assert_eq!(
            first_revision.file_id(),
            second_revision.file_id(),
            "same archive identity deliberately reuses its FileId"
        );
        assert_ne!(first_revision, second_revision);
        assert!(index.add_external_symbols_responsive_for_revision(vec![symbol()], second_revision));

        assert!(
            ExternalNavigationProof::capture(&index, &retained).is_none(),
            "a structurally identical R1 location must not be rebound to R2"
        );
        let current = index.find_symbol("dep.Duplicate").expect("R2 symbol");
        assert!(ExternalNavigationProof::capture(&index, &current).is_some());
    }

    #[test]
    fn retained_index_revision_is_not_replaced_by_changed_disk() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Moved.java");
        let original = "public class Moved {}";
        std::fs::write(&path, original).expect("original source");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_sync_for_test(&path)
            .expect("initial index");
        let stale = index
            .find_symbol_revisioned("Moved")
            .expect("initial symbol");

        let edited = "// shifted\npublic class Moved {}";
        std::fs::write(&path, edited).expect("edited source");
        index.clear_file_cache();

        let location = revisioned_symbol_location_to_lsp_location(&index, &stale)
            .expect("location conversion")
            .expect("retained exact indexed revision");
        assert_eq!(location.range.start.line, 0);
        assert_eq!(
            index
                .find_symbol("Moved")
                .expect("old indexed symbol")
                .range,
            stale.location.range,
            "navigation leaves the workspace revision untouched"
        );
    }

    #[test]
    fn stale_workspace_location_does_not_follow_duplicate_promotion() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first = temp.path().join("one/Dupe.java");
        let second = temp.path().join("two/Dupe.java");
        std::fs::create_dir_all(first.parent().unwrap()).expect("first parent");
        std::fs::create_dir_all(second.parent().unwrap()).expect("second parent");
        let declaration = "package p; public class Dupe {}";
        std::fs::write(&first, declaration).expect("first source");
        std::fs::write(&second, declaration).expect("second source");

        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file_sync_for_test(&first).expect("index first");
        index
            .index_file_sync_for_test(&second)
            .expect("index second");
        let stale = index
            .find_symbol_revisioned("p.Dupe")
            .expect("active duplicate");
        let active_path = index
            .get_file_path(stale.location.file_id)
            .expect("active source path");

        std::fs::write(&active_path, "package p; public class Other {}")
            .expect("remove active declaration");
        index.clear_file_cache();

        let location = revisioned_symbol_location_to_lsp_location(&index, &stale)
            .expect("location conversion")
            .expect("retained active revision");
        assert_eq!(location.range.start.line, 0);
        assert_eq!(
            index
                .find_symbol("p.Dupe")
                .expect("unchanged active symbol")
                .file_id,
            stale.location.file_id,
            "navigation does not reindex and promote a duplicate"
        );
    }

    #[test]
    fn evicted_caller_owned_buffer_fails_closed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Open.kt");
        std::fs::write(&path, "class DiskRevision").expect("disk source");
        let editor_source = "// editor-only\nclass Open";
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(&path, editor_source)
            .expect("editor revision indexes");
        let location = index.find_symbol_revisioned("Open").expect("editor symbol");
        index.clear_file_cache();

        assert!(
            revisioned_symbol_location_to_lsp_location(&index, &location)
                .expect("location conversion")
                .is_none(),
            "disk bytes must not be substituted for an evicted editor revision"
        );
    }

    #[test]
    fn revisioned_navigation_rejects_same_text_republication() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Same.java");
        let source = "class Same {}";
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(&path, source)
            .expect("R1 publication");
        let r1 = index.find_symbol_revisioned("Same").expect("R1 definition");

        index
            .index_file_with_source(&path, source)
            .expect("same-text R2 publication");

        assert!(
            revisioned_symbol_location_to_lsp_location(&index, &r1)
                .expect("conversion")
                .is_none(),
            "R1 range must not bind to identical R2 bytes"
        );
    }

    #[test]
    fn raw_workspace_location_is_never_rebound_after_same_text_republication() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Raw.java");
        let source = "class Raw {}";
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(&path, source)
            .expect("R1 publication");
        let raw_r1 = index.find_symbol("Raw").expect("raw R1 definition");

        index
            .index_file_with_source(&path, source)
            .expect("same-text R2 publication");

        assert!(
            symbol_location_to_lsp_location(&index, &raw_r1, None)
                .expect("conversion")
                .is_none(),
            "structural equality must not launder an unversioned R1 range into R2"
        );
    }

    #[test]
    fn duplicate_promotion_does_not_launder_prior_semantic_selection() {
        let temp = tempfile::tempdir().expect("tempdir");
        let first = temp.path().join("a/Dupe.java");
        let second = temp.path().join("b/Dupe.java");
        std::fs::create_dir_all(first.parent().unwrap()).expect("first parent");
        std::fs::create_dir_all(second.parent().unwrap()).expect("second parent");
        let declaration = "package p; class Dupe {}";
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(&first, declaration)
            .expect("first declaration");
        index
            .index_file_with_source(&second, declaration)
            .expect("duplicate declaration");
        let selected_r1 = index.find_symbol("p.Dupe").expect("selected R1 owner");
        let selected_path = index
            .get_file_path(selected_r1.file_id)
            .expect("selected path");

        index
            .index_file_with_source(&selected_path, "package p; class Other {}")
            .expect("retire selected owner and promote duplicate");

        assert!(
            symbol_location_to_lsp_location(&index, &selected_r1, None)
                .expect("conversion")
                .is_none(),
            "a later duplicate owner must not inherit the prior selection's range authority"
        );
        let promoted = index.find_symbol("p.Dupe").expect("promoted definition");
        assert_ne!(promoted.file_id, selected_r1.file_id);
        assert!(
            symbol_location_to_lsp_location(&index, &promoted, None)
                .expect("conversion")
                .is_some(),
            "the freshly queried promoted owner remains navigable"
        );
    }

    #[test]
    fn extracted_source_matching_distinguishes_same_arity_overloads() {
        let source = crate::source_provider::SourceFile::new(Arc::from(
            r#"
                package sample;
                class Api<T> {
                    Api(String value) {}
                    void accept(java.util.Date value) {}
                    void accept(java.sql.Date value) {}
                    void spread(String... values) {}
                    void generic(T value) {}
                    void incompatible(int value) {}
                    void qualifiedOnly(a.Foo value) {}
                }
            "#,
        ));
        let symbols = source.java_symbols().unwrap();

        let sql_date = find_extracted_symbol(symbols, "sample.Api.accept(java.sql.Date)")
            .expect("qualified overload should match");
        assert_eq!(sql_date.fqn.as_ref(), "sample.Api.accept(java.sql.Date)");

        assert!(
            find_extracted_symbol(symbols, "sample.Api.accept(Date)").is_none(),
            "an ambiguous simple-name signature must not jump to an arbitrary overload"
        );

        let varargs = find_extracted_symbol(symbols, "sample.Api.spread(java.lang.String[])")
            .expect("bytecode arrays and source varargs should have one identity");
        assert_eq!(
            varargs.fqn.as_ref(),
            "sample.Api.spread(java.lang.String...)"
        );

        let erased = find_extracted_symbol(symbols, "sample.Api.generic(java.lang.Object)")
            .expect("a sole erased candidate is unambiguous");
        assert_eq!(erased.fqn.as_ref(), "sample.Api.generic(T)");

        assert!(
            find_extracted_symbol(symbols, "sample.Api.incompatible(java.lang.String)").is_none(),
            "arity alone cannot prove that a different parameter type owns the bytecode target"
        );
        assert!(
            find_extracted_symbol(symbols, "sample.Api.qualifiedOnly(b.Foo)").is_none(),
            "two qualified types with the same simple name are not compatible"
        );
        assert!(find_extracted_symbol(symbols, "sample.Api.spread").is_some());
        assert!(
            find_extracted_symbol(symbols, "sample.Api.accept").is_none(),
            "a descriptorless overloaded target remains ambiguous"
        );

        assert!(find_extracted_symbol(symbols, "sample.Api.Api(java.lang.String)").is_some());
    }

    #[test]
    fn test_position_to_offset_simple() {
        let text = "hello world";
        assert_eq!(position_to_offset(text, Position::new(0, 0)).unwrap(), 0);
        assert_eq!(position_to_offset(text, Position::new(0, 6)).unwrap(), 6);
        assert_eq!(position_to_offset(text, Position::new(0, 11)).unwrap(), 11);
    }

    #[test]
    fn test_position_to_offset_multiline() {
        let text = "line 0\nline 1\nline 2";

        // Start of document
        assert_eq!(position_to_offset(text, Position::new(0, 0)).unwrap(), 0);

        // Middle of first line
        assert_eq!(position_to_offset(text, Position::new(0, 3)).unwrap(), 3);

        // Start of second line
        assert_eq!(position_to_offset(text, Position::new(1, 0)).unwrap(), 7);

        // Middle of second line
        assert_eq!(position_to_offset(text, Position::new(1, 3)).unwrap(), 10);

        // Start of third line
        assert_eq!(position_to_offset(text, Position::new(2, 0)).unwrap(), 14);
    }

    #[test]
    fn test_position_to_offset_unicode() {
        let text = "hello 世界";
        // ASCII part
        assert_eq!(position_to_offset(text, Position::new(0, 0)).unwrap(), 0);
        assert_eq!(position_to_offset(text, Position::new(0, 6)).unwrap(), 6);
        // '世' is 3 UTF-8 bytes, 1 UTF-16 code unit
        assert_eq!(position_to_offset(text, Position::new(0, 7)).unwrap(), 9);
        // '界' is 3 UTF-8 bytes, 1 UTF-16 code unit
        assert_eq!(position_to_offset(text, Position::new(0, 8)).unwrap(), 12);
    }

    #[test]
    fn test_offset_to_position_simple() {
        let text = "hello world";
        assert_eq!(offset_to_position(text, 0), Position::new(0, 0));
        assert_eq!(offset_to_position(text, 6), Position::new(0, 6));
        assert_eq!(offset_to_position(text, 11), Position::new(0, 11));
    }

    #[test]
    fn test_offset_to_position_multiline() {
        let text = "line 0\nline 1\nline 2";

        // Start of document
        assert_eq!(offset_to_position(text, 0), Position::new(0, 0));

        // Middle of first line
        assert_eq!(offset_to_position(text, 3), Position::new(0, 3));

        // Start of second line (after '\n')
        assert_eq!(offset_to_position(text, 7), Position::new(1, 0));

        // Middle of second line
        assert_eq!(offset_to_position(text, 10), Position::new(1, 3));

        // Start of third line
        assert_eq!(offset_to_position(text, 14), Position::new(2, 0));
    }

    #[test]
    fn test_offset_to_position_unicode() {
        let text = "hello 世界";
        assert_eq!(offset_to_position(text, 0), Position::new(0, 0));
        assert_eq!(offset_to_position(text, 6), Position::new(0, 6));
        // '世' is at byte offset 6-9, UTF-16 offset 7
        assert_eq!(offset_to_position(text, 9), Position::new(0, 7));
        // '界' is at byte offset 9-12, UTF-16 offset 8
        assert_eq!(offset_to_position(text, 12), Position::new(0, 8));
    }

    #[test]
    fn test_position_offset_roundtrip() {
        let text = "hello\nworld\n测试";

        // Test several positions
        let positions = vec![
            Position::new(0, 0),
            Position::new(0, 5),
            Position::new(1, 0),
            Position::new(1, 5),
            Position::new(2, 0),
            Position::new(2, 2),
        ];

        for pos in positions {
            let offset = position_to_offset(text, pos).unwrap();
            let back = offset_to_position(text, offset as usize);
            assert_eq!(back, pos, "Roundtrip failed for position {:?}", pos);
        }
    }
}
