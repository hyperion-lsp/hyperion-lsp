// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Query engine: high-level API for symbol resolution and navigation

use crate::{
    import_resolver::ImportResolver, index::WorkspaceIndex, position_utils::LineIndex,
    CurrentUsageLocations, CurrentUsageQueryError, DeclarationNamespace, DefinitionSourceAuthority,
    FileId, FileRevisionToken, RevisionedLocation, SymbolLocation, SymbolTarget, WorkspaceRevision,
};
use anyhow::Result;
use lsp_types::{Location, Range, SymbolInformation, Url};
use std::{collections::HashMap, sync::Arc};
use tracing::debug;

/// Query engine for workspace-wide symbol resolution
pub struct QueryEngine {
    index: WorkspaceIndex,
}

/// Coordinates prepared once per source publication for one search response.
struct WorkspaceSymbolSource {
    content: Arc<String>,
    lines: LineIndex,
    uri: Url,
}

type WorkspaceSymbolSources = HashMap<FileRevisionToken, Option<WorkspaceSymbolSource>>;

/// Definitions and a complete usage scan resolved under one workspace graph.
#[derive(Debug, Clone)]
pub struct CurrentReferences {
    pub definitions: Vec<RevisionedLocation>,
    /// Definitions owned by the current immutable classpath graph. These have
    /// no workspace file token; `workspace_revision` is their exact proof.
    pub external_definitions: Vec<SymbolLocation>,
    pub usages: CurrentUsageLocations,
}

impl CurrentReferences {
    pub fn workspace_revision(&self) -> WorkspaceRevision {
        self.usages.workspace_revision
    }

    pub fn locations(&self) -> impl Iterator<Item = &SymbolLocation> {
        self.definitions
            .iter()
            .map(|definition| &definition.location)
            .chain(self.external_definitions.iter())
            .chain(self.usages.locations())
    }

    pub fn len(&self) -> usize {
        self.definitions.len() + self.external_definitions.len() + self.usages.locations().count()
    }

    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
            && self.external_definitions.is_empty()
            && self.usages.files.is_empty()
    }
}

impl QueryEngine {
    pub fn new(index: WorkspaceIndex) -> Self {
        Self { index }
    }

    /// Resolve a symbol reference to its definition
    ///
    /// Takes a simple name (e.g., "List") and the file context, and returns
    /// all possible definition locations.
    pub fn goto_definition(
        &self,
        simple_name: &str,
        file_id: FileId,
    ) -> Result<Vec<SymbolLocation>> {
        debug!("goto_definition: {} in file {:?}", simple_name, file_id);

        // Get import map for the file
        let imports = match self.index.get_imports(file_id) {
            Some(imp) => imp,
            None => {
                debug!("No imports found for file {:?}", file_id);
                return Ok(Vec::new());
            }
        };

        // Resolve simple name to possible FQNs
        let resolver = ImportResolver::new(&imports, &self.index);
        let fqn_candidates = resolver.resolve(simple_name);

        debug!(
            "Resolved {} to candidates: {:?}",
            simple_name, fqn_candidates
        );

        // Look up each candidate in the index
        let mut locations = Vec::new();
        for fqn in fqn_candidates {
            if let Some(location) = self.index.find_symbol(&fqn) {
                locations.push(location);
            }
        }

        Ok(locations)
    }

    /// Find all references to a symbol (including definition)
    ///
    /// Returns both the definition and all usages of the symbol across the workspace.
    ///
    /// # Arguments
    /// * `fqn` - Fully qualified name of the symbol
    ///
    /// # Returns
    /// Vector containing the definition (if found) followed by all usages
    pub fn find_current_references(
        &self,
        fqn: &str,
    ) -> std::result::Result<CurrentReferences, CurrentUsageQueryError> {
        self.find_current_references_cancellable(fqn, || false)
    }

    pub fn find_current_references_cancellable<F>(
        &self,
        fqn: &str,
        is_cancelled: F,
    ) -> std::result::Result<CurrentReferences, CurrentUsageQueryError>
    where
        F: Fn() -> bool + Sync,
    {
        let mut targets = DeclarationNamespace::ALL
            .into_iter()
            .filter_map(|namespace| {
                let target = SymbolTarget::new(Arc::from(fqn), namespace);
                self.index.find_symbol_definition(&target).map(|_| target)
            });
        let target = targets.next();
        if targets.next().is_some() {
            return Err(CurrentUsageQueryError::Unavailable(format!(
                "'{fqn}' spans declaration namespaces; use find_current_references_target"
            )));
        }
        let Some(target) = target else {
            let usages = self.index.find_current_usages(fqn)?;
            return Ok(CurrentReferences {
                definitions: Vec::new(),
                external_definitions: Vec::new(),
                usages,
            });
        };
        self.find_current_references_target_cancellable(&target, is_cancelled)
    }

    /// Find references to one exact semantic declaration identity.
    pub fn find_current_references_target(
        &self,
        target: &SymbolTarget,
    ) -> std::result::Result<CurrentReferences, CurrentUsageQueryError> {
        self.find_current_references_target_cancellable(target, || false)
    }

    pub fn find_current_references_target_cancellable<F>(
        &self,
        target: &SymbolTarget,
        is_cancelled: F,
    ) -> std::result::Result<CurrentReferences, CurrentUsageQueryError>
    where
        F: Fn() -> bool + Sync,
    {
        for _ in 0..3 {
            if is_cancelled() {
                return Err(CurrentUsageQueryError::Cancelled);
            }
            let workspace_revision = self.index.current_workspace_revision();
            let mut definitions = Vec::new();
            let mut external_definitions = Vec::new();
            let mut targets = vec![target.clone()];

            let target_definition = self.index.find_symbol_definition(target);
            if target_definition.as_ref().is_some_and(|definition| {
                !self
                    .index
                    .workspace_bindings_agree(definition.workspace_revision, workspace_revision)
            }) {
                continue;
            }
            let family_definitions = if target.namespace == DeclarationNamespace::Callable
                && target_definition.is_some()
                && target.fqn.contains('(')
            {
                match self
                    .index
                    .find_current_callable_override_family_cancellable(
                        target,
                        workspace_revision,
                        &is_cancelled,
                    ) {
                    Err(CurrentUsageQueryError::ConcurrentModification) => continue,
                    Ok(family) => family,
                    Err(CurrentUsageQueryError::Unavailable(reason)) => {
                        debug!(
                            "Exact callable-family expansion unavailable for '{}': {reason}",
                            target.fqn
                        );
                        target_definition.into_iter().collect()
                    }
                    Err(CurrentUsageQueryError::Cancelled) => {
                        return Err(CurrentUsageQueryError::Cancelled);
                    }
                }
            } else {
                target_definition.into_iter().collect()
            };

            for definition in family_definitions {
                targets.push(definition.target.clone());
                match definition.source {
                    DefinitionSourceAuthority::Workspace(revision) => {
                        definitions.push(RevisionedLocation {
                            revision,
                            location: definition.location.clone(),
                        });
                    }
                    DefinitionSourceAuthority::External => {
                        external_definitions.push(definition.location.clone());
                    }
                }
            }

            targets.sort_unstable_by(|left, right| {
                left.fqn
                    .cmp(&right.fqn)
                    .then((left.namespace as u8).cmp(&(right.namespace as u8)))
            });
            targets.dedup();
            definitions.sort_unstable_by(|left, right| {
                left.location
                    .file_id
                    .as_u32()
                    .cmp(&right.location.file_id.as_u32())
                    .then(left.location.range.start.cmp(&right.location.range.start))
            });
            definitions.dedup_by(|left, right| {
                left.revision == right.revision && left.location.range == right.location.range
            });
            external_definitions.sort_unstable_by(|left, right| {
                left.file_id
                    .as_u32()
                    .cmp(&right.file_id.as_u32())
                    .then(left.range.start.cmp(&right.range.start))
                    .then(left.range.end.cmp(&right.range.end))
                    .then(left.fqn.cmp(&right.fqn))
            });
            external_definitions.dedup_by(|left, right| {
                left.file_id == right.file_id && left.range == right.range && left.fqn == right.fqn
            });

            if !self
                .index
                .workspace_bindings_are_current(workspace_revision)
            {
                continue;
            }
            let usages = match self.index.find_current_usages_for_symbol_targets(
                &targets,
                Some(workspace_revision),
                &is_cancelled,
            ) {
                Err(CurrentUsageQueryError::ConcurrentModification) => continue,
                other => other?,
            };
            let result = CurrentReferences {
                definitions,
                external_definitions,
                usages,
            };
            if self.references_are_current(&result) {
                return Ok(result);
            }
        }
        Err(CurrentUsageQueryError::ConcurrentModification)
    }

    /// Find all references to a symbol using exact current source bytes.
    ///
    /// This compatibility API intentionally returns owned locations, as it did
    /// before revision proofs were introduced. Request paths that must retain
    /// and revalidate those proofs should use [`Self::find_current_references`]
    /// instead.
    pub fn find_references(&self, fqn: &str) -> Result<Vec<SymbolLocation>> {
        let mut locations: Vec<_> = self
            .find_current_references(fqn)
            .map_err(anyhow::Error::new)?
            .locations()
            .cloned()
            .collect();
        locations.sort_unstable_by(|left, right| {
            left.fqn
                .cmp(&right.fqn)
                .then(left.file_id.as_u32().cmp(&right.file_id.as_u32()))
                .then(left.range.start.cmp(&right.range.start))
                .then(left.range.end.cmp(&right.range.end))
        });
        locations.dedup_by(|left, right| {
            left.fqn == right.fqn && left.file_id == right.file_id && left.range == right.range
        });
        Ok(locations)
    }

    pub fn references_are_current(&self, references: &CurrentReferences) -> bool {
        self.index.reference_revisions_are_current(
            references.workspace_revision(),
            references.usages.scanned_revisions(),
            &references.definitions,
        )
    }

    /// Search for symbols by name (fuzzy match)
    pub fn workspace_symbols(&self, query: &str) -> Result<Vec<SymbolInformation>> {
        self.workspace_symbols_cancellable(query, || false)
    }

    /// Search for symbols while cooperatively observing request cancellation.
    pub fn workspace_symbols_cancellable(
        &self,
        query: &str,
        is_cancelled: impl Fn() -> bool,
    ) -> Result<Vec<SymbolInformation>> {
        let locations = self.index.search_cancellable(query, &is_cancelled);

        let mut symbols = Vec::new();
        let mut sources = WorkspaceSymbolSources::new();
        for location in locations {
            if is_cancelled() {
                return Ok(Vec::new());
            }
            if let Some(symbol) = self.workspace_symbol_information(&location, &mut sources) {
                symbols.push(symbol);
            }
        }

        Ok(symbols)
    }

    /// Convert only against the publication that originally supplied the range.
    /// A new publication with identical bytes is still a different authority.
    fn workspace_symbol_information(
        &self,
        location: &SymbolLocation,
        sources: &mut WorkspaceSymbolSources,
    ) -> Option<SymbolInformation> {
        let (uri, range) = if let Some(revisioned) = self.index.revisioned_location(location) {
            // Cached coordinates never authorize retired ranges. Validate the
            // original publication even when this request already read its text.
            if self.index.current_file_revision(location.file_id) != Some(revisioned.revision) {
                return None;
            }
            let source = sources
                .entry(revisioned.revision)
                .or_insert_with(|| {
                    let indexed = self
                        .index
                        .get_indexed_revision_content(revisioned.revision)?;
                    Some(WorkspaceSymbolSource {
                        uri: Url::from_file_path(indexed.file_path).ok()?,
                        lines: LineIndex::new(&indexed.content),
                        content: indexed.content,
                    })
                })
                .as_ref()?;
            (
                source.uri.clone(),
                Range::new(
                    source
                        .lines
                        .offset_to_position(&source.content, location.range.start as usize),
                    source
                        .lines
                        .offset_to_position(&source.content, location.range.end as usize),
                ),
            )
        } else {
            (
                Url::from_file_path(self.index.get_file_path(location.file_id)?).ok()?,
                Range::default(),
            )
        };
        #[allow(deprecated)]
        Some(SymbolInformation {
            name: location.name.to_string(),
            kind: location.kind,
            tags: None,
            deprecated: None,
            location: Location { uri, range },
            container_name: location.container_name.as_ref().map(|s| s.to_string()),
        })
    }

    /// Get the underlying index
    pub fn index(&self) -> &WorkspaceIndex {
        &self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use lsp_types::SymbolKind;
    use tempfile::TempDir;

    #[test]
    fn workspace_symbols_reject_locations_from_identical_republications() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Catalog.java");
        let source = "class Catalog {}";
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file_with_source(&path, source).unwrap();
        let engine = QueryEngine::new(index);
        let mut sources = WorkspaceSymbolSources::new();
        let original = engine.index.search("Catalog").pop().unwrap();
        assert!(engine
            .workspace_symbol_information(&original, &mut sources)
            .is_some());

        // Force A -> B -> A so text, path and symbol range all match again,
        // while the original publication authority has been retired.
        engine
            .index
            .index_file_with_source(&path, "class Changed {}")
            .unwrap();
        engine.index.index_file_with_source(&path, source).unwrap();
        let current = engine.index.search("Catalog").pop().unwrap();
        assert_eq!(original.range, current.range);
        assert_eq!(original.file_id, current.file_id);
        assert!(engine
            .workspace_symbol_information(&original, &mut sources)
            .is_none());
        assert!(engine
            .workspace_symbol_information(&current, &mut sources)
            .is_some());
    }

    #[test]
    fn workspace_symbols_reject_stale_ranges_when_only_utf16_columns_change() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Catalog.java");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(&path, "/* 😀 */ class Catalog {}")
            .unwrap();
        let engine = QueryEngine::new(index);
        let mut sources = WorkspaceSymbolSources::new();
        let original = engine.index.search("Catalog").pop().unwrap();
        let before = engine
            .workspace_symbol_information(&original, &mut sources)
            .unwrap();
        assert_eq!(before.location.range.start.character, 15);

        // Four ASCII bytes replace four UTF-8 bytes, preserving symbol offsets
        // and identity fields but changing the editor column by two code units.
        engine
            .index
            .index_file_with_source(&path, "/* abcd */ class Catalog {}")
            .unwrap();
        let current = engine.index.search("Catalog").pop().unwrap();
        assert_eq!(original.range, current.range);
        assert!(engine
            .workspace_symbol_information(&original, &mut sources)
            .is_none());
        let after = engine
            .workspace_symbol_information(&current, &mut sources)
            .unwrap();
        assert_eq!(after.location.range.start.character, 17);
    }

    #[test]
    fn workspace_symbols_reject_locations_after_file_removal() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("Removed.java");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(&path, "class Removed {}")
            .unwrap();
        let engine = QueryEngine::new(index);
        let mut sources = WorkspaceSymbolSources::new();
        let original = engine.index.search("Removed").pop().unwrap();
        engine.index.remove_file(original.file_id);
        assert!(engine
            .workspace_symbol_information(&original, &mut sources)
            .is_none());
    }

    #[tokio::test]
    async fn test_find_references_includes_interface_implementations() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let interface_file = root.join("Service.java");
        std::fs::write(
            &interface_file,
            "package com.example;\npublic interface Service { void process(String data); }",
        )
        .unwrap();

        let impl_file = root.join("UserService.java");
        std::fs::write(
            &impl_file,
            "package com.example;\npublic class UserService implements Service { public void process(String data) {} }",
        )
        .unwrap();

        let index = WorkspaceIndex::new(root);
        index.index_file(&interface_file).await.unwrap();
        index.index_file(&impl_file).await.unwrap();

        let query_engine = QueryEngine::new(index);
        let implementations = query_engine
            .index()
            .get_interface_implementations("com.example.Service");
        assert!(
            implementations
                .iter()
                .any(|fqn| fqn == "com.example.UserService"),
            "Expected interface implementation index to include UserService"
        );

        let iface_symbol = query_engine
            .index()
            .get_symbol("com.example.Service")
            .expect("Expected interface symbol");
        assert_eq!(iface_symbol.kind, SymbolKind::INTERFACE);
        let methods = query_engine
            .index()
            .find_methods_by_name("com.example.Service", "process");
        assert!(!methods.is_empty());
        assert!(
            !methods[0].parameters.is_empty(),
            "Expected interface method parameters to be captured"
        );
        let target_fqn = methods[0].fqn.to_string();
        assert!(
            target_fqn.contains('('),
            "Expected interface method FQN to include parameters"
        );
        let method_signature = target_fqn
            .find('(')
            .and_then(|paren| {
                target_fqn[..paren]
                    .rsplit_once('.')
                    .map(|(class_fqn, _)| &target_fqn[class_fqn.len() + 1..])
            })
            .unwrap_or("");

        let impl_methods = query_engine
            .index()
            .find_methods_by_name("com.example.UserService", "process");
        assert!(
            !impl_methods.is_empty(),
            "Expected implementing class method to be indexed"
        );
        let impl_fqn = impl_methods
            .first()
            .map(|m| m.fqn.to_string())
            .expect("Expected UserService.process symbol");
        assert!(
            impl_fqn.contains('('),
            "Expected implementing method FQN to include parameters"
        );

        let normalize = |param: &str| {
            let mut base = param.trim();
            let mut suffix = String::new();
            if base.ends_with("...") {
                base = base.trim_end_matches("...");
                suffix.push_str("...");
            } else {
                while base.ends_with("[]") {
                    base = base.trim_end_matches("[]");
                    suffix.push_str("[]");
                }
            }
            let simple = base.rsplit('.').next().unwrap_or(base);
            format!("{}{}", simple, suffix)
        };

        let expected_params: Vec<String> = method_signature
            .split_once('(')
            .and_then(|(_, rest)| rest.strip_suffix(')'))
            .map(|params| {
                if params.trim().is_empty() {
                    Vec::new()
                } else {
                    params.split(',').map(&normalize).collect()
                }
            })
            .unwrap_or_default();
        let candidate_params: Vec<String> = impl_methods
            .first()
            .unwrap()
            .parameters
            .iter()
            .map(|p| {
                let type_part = p.split_whitespace().next().unwrap_or("");
                let base = type_part.split('<').next().unwrap_or(type_part);
                normalize(base)
            })
            .collect();
        assert_eq!(expected_params, candidate_params);

        let references = query_engine.find_current_references(&target_fqn).unwrap();
        assert!(
            references
                .locations()
                .any(|loc| loc.fqn.as_ref() == impl_fqn.as_str()),
            "Expected interface method references to include implementing class"
        );
    }

    #[tokio::test]
    async fn test_find_references_uses_overload_signature_for_overrides() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();

        let base_file = root.join("Base.java");
        std::fs::write(
            &base_file,
            "package com.example;\npublic class Base { public void foo(int a) {} public void foo(String a) {} }",
        )
        .unwrap();

        let child_file = root.join("Child.java");
        std::fs::write(
            &child_file,
            "package com.example;\npublic class Child extends Base { @Override public void foo(int a) {} public void foo(String a) {} }",
        )
        .unwrap();

        let index = WorkspaceIndex::new(root);
        index.index_file(&base_file).await.unwrap();
        index.index_file(&child_file).await.unwrap();

        let query_engine = QueryEngine::new(index);
        let base_methods = query_engine
            .index()
            .find_methods_by_name("com.example.Base", "foo");
        let target_fqn = base_methods
            .iter()
            .find(|m| m.parameters.first().is_some_and(|p| p.starts_with("int ")))
            .map(|m| m.fqn.to_string())
            .expect("Expected Base.foo(int) symbol");

        let child_methods = query_engine
            .index()
            .find_methods_by_name("com.example.Child", "foo");
        let child_int_fqn = child_methods
            .iter()
            .find(|m| m.parameters.first().is_some_and(|p| p.starts_with("int ")))
            .map(|m| m.fqn.to_string())
            .expect("Expected Child.foo(int) symbol");
        let child_string_fqn = child_methods
            .iter()
            .find(|m| {
                m.parameters
                    .first()
                    .is_some_and(|p| p.contains("String") && !p.starts_with("int "))
            })
            .map(|m| m.fqn.to_string())
            .expect("Expected Child.foo(String) symbol");

        let references = query_engine.find_current_references(&target_fqn).unwrap();
        assert!(
            references
                .locations()
                .any(|loc| loc.fqn.as_ref() == child_int_fqn.as_str()),
            "Expected override references to include Child.foo(int)"
        );
        assert!(
            !references
                .locations()
                .any(|loc| loc.fqn.as_ref() == child_string_fqn.as_str()),
            "Expected overload Child.foo(String) to be excluded"
        );
    }

    #[tokio::test]
    async fn references_from_leaf_override_include_the_complete_method_family() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let sources = [
            (
                "Service.java",
                "package family; public interface Service { void run(String value); }",
            ),
            (
                "Base.java",
                concat!(
                    "package family; ",
                    "public abstract class Base implements Service { ",
                    "public abstract void run(String value); }"
                ),
            ),
            (
                "Left.java",
                concat!(
                    "package family; ",
                    "public class Left extends Base { ",
                    "public void run(String value) {} }"
                ),
            ),
            (
                "Right.java",
                concat!(
                    "package family; ",
                    "public class Right extends Base { ",
                    "public void run(String value) {} }"
                ),
            ),
            (
                "Caller.java",
                concat!(
                    "package family; public class Caller { ",
                    "void call(Service service, Base base, Left left, Right right) { ",
                    "service.run(\"\"); base.run(\"\"); ",
                    "left.run(\"\"); right.run(\"\"); } }"
                ),
            ),
        ];

        let index = WorkspaceIndex::new(root.clone());
        for (name, source) in sources {
            let path = root.join(name);
            std::fs::write(&path, source).unwrap();
            index.index_file(&path).await.unwrap();
        }

        let query_engine = QueryEngine::new(index);
        let owners = [
            "family.Service",
            "family.Base",
            "family.Left",
            "family.Right",
        ];
        let caller_id = query_engine
            .index()
            .get_file_id(&root.join("Caller.java"))
            .expect("Caller.java should be indexed");
        let mut expected_family = None;
        for selected_owner in owners {
            let selected = query_engine
                .index()
                .find_methods_by_name(selected_owner, "run")
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("{selected_owner}.run should be indexed"));
            let references = query_engine
                .find_current_references_target(&SymbolTarget::for_symbol(&selected))
                .unwrap();
            let family: std::collections::HashSet<_> = references
                .locations()
                .map(|location| location.fqn.to_string())
                .collect();

            for owner in owners {
                assert!(
                    family
                        .iter()
                        .any(|fqn| fqn.starts_with(&format!("{owner}.run("))),
                    "lookup from {selected_owner} omitted {owner}.run from {family:?}"
                );
            }
            assert_eq!(family.len(), owners.len(), "family contains duplicates");
            assert_eq!(
                references
                    .locations()
                    .filter(|location| location.file_id == caller_id)
                    .count(),
                owners.len(),
                "lookup from {selected_owner} omitted calls through a family receiver type"
            );
            if let Some(expected) = expected_family.as_ref() {
                assert_eq!(
                    &family, expected,
                    "family depends on selected declaration {selected_owner}"
                );
            } else {
                expected_family = Some(family);
            }
        }

        let selected = query_engine
            .index()
            .find_methods_by_name("family.Service", "run")
            .into_iter()
            .next()
            .unwrap();
        assert!(matches!(
            query_engine
                .index()
                .find_current_callable_override_family_cancellable(
                    &SymbolTarget::for_symbol(&selected),
                    query_engine.index().current_workspace_revision(),
                    || true,
                ),
            Err(CurrentUsageQueryError::Cancelled)
        ));
    }

    #[tokio::test]
    async fn override_matching_preserves_parameter_type_identity() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let sources = [
            ("AUser.java", "package a; public class User {}"),
            ("BUser.java", "package b; public class User {}"),
            (
                "Base.java",
                "package family; public class Base { public void accept(a.User user) {} }",
            ),
            (
                "Child.java",
                concat!(
                    "package family; public class Child extends Base { ",
                    "public void accept(b.User user) {} }"
                ),
            ),
        ];

        let index = WorkspaceIndex::new(root.clone());
        for (name, source) in sources {
            let path = root.join(name);
            std::fs::write(&path, source).unwrap();
            index.index_file(&path).await.unwrap();
        }

        let query_engine = QueryEngine::new(index);
        let base = query_engine
            .index()
            .find_methods_by_name("family.Base", "accept")
            .into_iter()
            .next()
            .expect("Base.accept should be indexed");
        let child = query_engine
            .index()
            .find_methods_by_name("family.Child", "accept")
            .into_iter()
            .next()
            .expect("Child.accept should be indexed");
        let references = query_engine
            .find_current_references_target(&SymbolTarget::for_symbol(&base))
            .unwrap();

        assert!(
            !references
                .locations()
                .any(|location| location.fqn == child.fqn),
            "same simple parameter name in another package is not an override"
        );
    }

    #[tokio::test]
    async fn private_and_static_methods_do_not_form_override_families() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let base_path = root.join("Base.java");
        let child_path = root.join("Child.java");
        std::fs::write(
            &base_path,
            concat!(
                "package family; public class Base { ",
                "private void hidden(String value) {} ",
                "public static void utility(String value) {} }"
            ),
        )
        .unwrap();
        std::fs::write(
            &child_path,
            concat!(
                "package family; public class Child extends Base { ",
                "public void hidden(String value) {} ",
                "public static void utility(String value) {} }"
            ),
        )
        .unwrap();

        let index = WorkspaceIndex::new(root);
        index.index_file(&base_path).await.unwrap();
        index.index_file(&child_path).await.unwrap();
        let query_engine = QueryEngine::new(index);

        for method_name in ["hidden", "utility"] {
            let base = query_engine
                .index()
                .find_methods_by_name("family.Base", method_name)
                .into_iter()
                .next()
                .unwrap();
            let child = query_engine
                .index()
                .find_methods_by_name("family.Child", method_name)
                .into_iter()
                .next()
                .unwrap();
            let references = query_engine
                .find_current_references_target(&SymbolTarget::for_symbol(&base))
                .unwrap();
            assert!(
                !references
                    .locations()
                    .any(|location| location.fqn == child.fqn),
                "{method_name} must not be treated as an override"
            );
        }
    }

    #[tokio::test]
    async fn override_edges_respect_package_paths_and_final_methods() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let sources = [
            (
                "Base.java",
                concat!(
                    "package a; public class Base { ",
                    "void packageOnly(String value) {} ",
                    "public void closed(String value) {} }"
                ),
            ),
            (
                "Direct.java",
                concat!(
                    "package a; public class Direct extends Base { ",
                    "public void packageOnly(String value) {} }"
                ),
            ),
            (
                "Middle.java",
                "package b; public class Middle extends a.Base {}",
            ),
            (
                "Returned.java",
                concat!(
                    "package a; public class Returned extends b.Middle { ",
                    "public void packageOnly(String value) {} }"
                ),
            ),
            (
                "FinalChild.java",
                concat!(
                    "package a; public class FinalChild extends Base { ",
                    "public final void closed(String value) {} }"
                ),
            ),
            (
                "InvalidGrandchild.java",
                concat!(
                    "package a; public class InvalidGrandchild extends FinalChild { ",
                    "public void closed(String value) {} }"
                ),
            ),
        ];
        let index = WorkspaceIndex::new(root.clone());
        for (name, source) in sources {
            let path = root.join(name);
            std::fs::write(&path, source).unwrap();
            index.index_file(&path).await.unwrap();
        }
        let query_engine = QueryEngine::new(index);

        let base_package = query_engine
            .index()
            .find_methods_by_name("a.Base", "packageOnly")
            .into_iter()
            .next()
            .unwrap();
        let package_family: std::collections::HashSet<_> = query_engine
            .find_current_references_target(&SymbolTarget::for_symbol(&base_package))
            .unwrap()
            .locations()
            .map(|location| location.fqn.to_string())
            .collect();
        assert!(package_family
            .iter()
            .any(|fqn| fqn.starts_with("a.Direct.")));
        assert!(
            !package_family
                .iter()
                .any(|fqn| fqn.starts_with("a.Returned.")),
            "package-private inheritance must not cross out of and back into a package"
        );

        let base_closed = query_engine
            .index()
            .find_methods_by_name("a.Base", "closed")
            .into_iter()
            .next()
            .unwrap();
        let closed_family: std::collections::HashSet<_> = query_engine
            .find_current_references_target(&SymbolTarget::for_symbol(&base_closed))
            .unwrap()
            .locations()
            .map(|location| location.fqn.to_string())
            .collect();
        assert!(closed_family
            .iter()
            .any(|fqn| fqn.starts_with("a.FinalChild.")));
        assert!(
            !closed_family
                .iter()
                .any(|fqn| fqn.starts_with("a.InvalidGrandchild.")),
            "a final override must terminate descendant family edges"
        );

        let invalid_grandchild = query_engine
            .index()
            .find_methods_by_name("a.InvalidGrandchild", "closed")
            .into_iter()
            .next()
            .unwrap();
        let invalid_family: Vec<_> = query_engine
            .find_current_references_target(&SymbolTarget::for_symbol(&invalid_grandchild))
            .unwrap()
            .locations()
            .map(|location| location.fqn.to_string())
            .collect();
        assert_eq!(invalid_family, [invalid_grandchild.fqn.to_string()]);
    }

    #[tokio::test]
    async fn varargs_and_arrays_share_one_override_signature() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let base_path = root.join("Base.java");
        let child_path = root.join("Child.java");
        std::fs::write(
            &base_path,
            "package family; public class Base { public void collect(String... values) {} }",
        )
        .unwrap();
        std::fs::write(
            &child_path,
            concat!(
                "package family; public class Child extends Base { ",
                "public void collect(String[] values) {} }"
            ),
        )
        .unwrap();

        let index = WorkspaceIndex::new(root);
        index.index_file(&base_path).await.unwrap();
        index.index_file(&child_path).await.unwrap();
        let query_engine = QueryEngine::new(index);
        let base = query_engine
            .index()
            .find_methods_by_name("family.Base", "collect")
            .into_iter()
            .next()
            .unwrap();
        let references = query_engine
            .find_current_references_target(&SymbolTarget::for_symbol(&base))
            .unwrap();
        assert!(
            references
                .locations()
                .any(|location| location.fqn.starts_with("family.Child.collect(")),
            "varargs and array declarations have the same JVM override signature"
        );
    }

    #[tokio::test]
    async fn unsubstituted_generic_override_families_fail_closed() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let base_path = root.join("Base.java");
        let child_path = root.join("Child.java");
        std::fs::write(
            &base_path,
            concat!(
                "package family; import java.util.List; ",
                "public class Base<T> { public void set(List<T> value) {} ",
                "public T get() { return null; } }"
            ),
        )
        .unwrap();
        std::fs::write(
            &child_path,
            concat!(
                "package family; import java.util.List; ",
                "public class Child extends Base<String> { ",
                "public void set(List<Integer> value) {} ",
                "public String get() { return null; } }"
            ),
        )
        .unwrap();

        let index = WorkspaceIndex::new(root);
        index.index_file(&base_path).await.unwrap();
        index.index_file(&child_path).await.unwrap();
        let query_engine = QueryEngine::new(index);
        for owner in ["family.Base", "family.Child"] {
            for method_name in ["set", "get"] {
                let method = query_engine
                    .index()
                    .find_methods_by_name(owner, method_name)
                    .into_iter()
                    .next()
                    .unwrap();
                let target = SymbolTarget::for_symbol(&method);
                assert!(matches!(
                    query_engine.index().find_current_callable_override_family(
                        &target,
                        query_engine.index().current_workspace_revision(),
                    ),
                    Err(CurrentUsageQueryError::Unavailable(_))
                ));
                let direct = query_engine.find_current_references_target(&target).expect(
                    "ordinary references should remain available without a complete family",
                );
                assert_eq!(
                    direct.definitions.len(),
                    1,
                    "incomplete {method_name} family fallback must remain exact to {owner}"
                );
            }
        }
    }

    #[tokio::test]
    async fn inherited_class_method_satisfies_interface_at_join_type() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let sources = [
            (
                "Contract.java",
                "package join; public interface Contract { void run(); }",
            ),
            (
                "Base.java",
                concat!(
                    "package join; public class Base { ",
                    "public final void run() {} }"
                ),
            ),
            (
                "Child.java",
                "package join; public class Child extends Base implements Contract {}",
            ),
            (
                "Caller.java",
                concat!(
                    "package join; public class Caller { ",
                    "void call(Contract contract, Base base, Child child) { ",
                    "contract.run(); base.run(); child.run(); } }"
                ),
            ),
        ];
        let index = WorkspaceIndex::new(root.clone());
        for (name, source) in sources {
            let path = root.join(name);
            std::fs::write(&path, source).unwrap();
            index.index_file(&path).await.unwrap();
        }
        let caller_id = index.get_file_id(&root.join("Caller.java")).unwrap();
        let query_engine = QueryEngine::new(index);
        let mut expected_family = None;

        for owner in ["join.Contract", "join.Base"] {
            let method = query_engine
                .index()
                .find_methods_by_name(owner, "run")
                .into_iter()
                .next()
                .unwrap();
            let references = query_engine
                .find_current_references_target(&SymbolTarget::for_symbol(&method))
                .unwrap();
            let family = references
                .locations()
                .filter(|location| location.file_id != caller_id)
                .map(|location| location.fqn.to_string())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(family.len(), 2);
            assert!(family
                .iter()
                .any(|fqn| fqn.starts_with("join.Contract.run(")));
            assert!(family.iter().any(|fqn| fqn.starts_with("join.Base.run(")));
            assert_eq!(
                references
                    .locations()
                    .filter(|location| location.file_id == caller_id)
                    .count(),
                3,
                "calls through the interface, base, and join type must share a family"
            );
            if let Some(expected) = expected_family.as_ref() {
                assert_eq!(&family, expected);
            } else {
                expected_family = Some(family);
            }
        }

        let contract = query_engine
            .index()
            .find_methods_by_name("join.Contract", "run")
            .into_iter()
            .next()
            .unwrap();
        let implementations = query_engine
            .index()
            .find_current_overriding_method_definitions(
                &SymbolTarget::for_symbol(&contract),
                query_engine.index().current_workspace_revision(),
            )
            .unwrap();
        assert!(implementations
            .iter()
            .any(|definition| definition.target.fqn.starts_with("join.Base.run(")));
    }

    #[tokio::test]
    async fn override_subsignatures_preserve_parameterized_type_direction() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let sources = [
            (
                "GenericBase.java",
                concat!(
                    "package generic; import java.util.List; ",
                    "public class GenericBase { public void take(List<String> value) {} }"
                ),
            ),
            (
                "RawChild.java",
                concat!(
                    "package generic; import java.util.List; ",
                    "public class RawChild extends GenericBase { ",
                    "public void take(List value) {} }"
                ),
            ),
            (
                "EqualChild.java",
                concat!(
                    "package generic; import java.util.List; ",
                    "public class EqualChild extends GenericBase { ",
                    "public void take(List<String> value) {} }"
                ),
            ),
            (
                "RawBase.java",
                concat!(
                    "package generic; import java.util.List; ",
                    "public class RawBase { public void take(List value) {} }"
                ),
            ),
            (
                "TypedChild.java",
                concat!(
                    "package generic; import java.util.List; ",
                    "public class TypedChild extends RawBase { ",
                    "public void take(List<String> value) {} }"
                ),
            ),
            (
                "MismatchChild.java",
                concat!(
                    "package generic; import java.util.List; ",
                    "public class MismatchChild extends GenericBase { ",
                    "public void take(List<Integer> value) {} }"
                ),
            ),
        ];
        let index = WorkspaceIndex::new(root.clone());
        for (name, source) in sources {
            let path = root.join(name);
            std::fs::write(&path, source).unwrap();
            index.index_file(&path).await.unwrap();
        }
        let query_engine = QueryEngine::new(index);
        let family_for = |owner: &str| {
            let method = query_engine
                .index()
                .find_methods_by_name(owner, "take")
                .into_iter()
                .next()
                .unwrap();
            query_engine
                .index()
                .find_current_callable_override_family(
                    &SymbolTarget::for_symbol(&method),
                    query_engine.index().current_workspace_revision(),
                )
                .unwrap()
                .into_iter()
                .map(|definition| definition.target.fqn.to_string())
                .collect::<std::collections::HashSet<_>>()
        };

        let raw_child_family = family_for("generic.RawChild");
        assert_eq!(raw_child_family.len(), 3);
        assert!(raw_child_family
            .iter()
            .any(|fqn| fqn.starts_with("generic.GenericBase.take(")));
        assert!(raw_child_family
            .iter()
            .any(|fqn| fqn.starts_with("generic.EqualChild.take(")));
        assert_eq!(family_for("generic.EqualChild"), raw_child_family);
        assert_eq!(family_for("generic.TypedChild").len(), 1);
        assert_eq!(family_for("generic.MismatchChild").len(), 1);
    }

    #[tokio::test]
    async fn override_edges_validate_visibility_returns_and_final_owners() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let sources = [
            ("Animal.java", "package legality; public class Animal {}"),
            (
                "Dog.java",
                "package legality; public class Dog extends Animal {}",
            ),
            ("Stone.java", "package legality; public class Stone {}"),
            (
                "Base.java",
                concat!(
                    "package legality; public class Base { ",
                    "public Animal make() { return null; } }"
                ),
            ),
            (
                "Covariant.java",
                concat!(
                    "package legality; public class Covariant extends Base { ",
                    "public Dog make() { return null; } }"
                ),
            ),
            (
                "Weak.java",
                concat!(
                    "package legality; public class Weak extends Base { ",
                    "protected Animal make() { return null; } }"
                ),
            ),
            (
                "Incompatible.java",
                concat!(
                    "package legality; public class Incompatible extends Base { ",
                    "public Stone make() { return null; } }"
                ),
            ),
            (
                "Closed.java",
                concat!(
                    "package legality; public final class Closed { ",
                    "public void run() {} }"
                ),
            ),
            (
                "InvalidClosedChild.java",
                concat!(
                    "package legality; public class InvalidClosedChild extends Closed { ",
                    "public void run() {} }"
                ),
            ),
            (
                "ArrayBase.java",
                concat!(
                    "package legality; public class ArrayBase { ",
                    "public Object[] array() { return null; } }"
                ),
            ),
            (
                "ArrayChild.java",
                concat!(
                    "package legality; public class ArrayChild extends ArrayBase { ",
                    "public String[] array() { return null; } }"
                ),
            ),
            (
                "GenericParent.java",
                "package legality; public interface GenericParent<T> {}",
            ),
            (
                "GenericReturnBase.java",
                concat!(
                    "package legality; public class GenericReturnBase { ",
                    "public GenericParent<? extends Animal> generic() { return null; } }"
                ),
            ),
            (
                "GenericReturnChild.java",
                concat!(
                    "package legality; public class GenericReturnChild extends GenericReturnBase { ",
                    "public GenericParent<Dog> generic() { return null; } }"
                ),
            ),
            (
                "SwappedParent.java",
                "package legality; public interface SwappedParent<A, B> {}",
            ),
            (
                "SwappedChild.java",
                concat!(
                    "package legality; public interface SwappedChild<A, B> ",
                    "extends SwappedParent<B, A> {}"
                ),
            ),
            (
                "SwappedReturnBase.java",
                concat!(
                    "package legality; public class SwappedReturnBase { ",
                    "public SwappedParent<String, Integer> swapped() { return null; } }"
                ),
            ),
            (
                "SwappedReturnChild.java",
                concat!(
                    "package legality; public class SwappedReturnChild extends SwappedReturnBase { ",
                    "public SwappedChild<String, Integer> swapped() { return null; } }"
                ),
            ),
        ];
        let index = WorkspaceIndex::new(root.clone());
        for (name, source) in sources {
            let path = root.join(name);
            std::fs::write(&path, source).unwrap();
            index.index_file(&path).await.unwrap();
        }
        let query_engine = QueryEngine::new(index);
        let base = query_engine
            .index()
            .find_methods_by_name("legality.Base", "make")
            .into_iter()
            .next()
            .unwrap();
        let family = query_engine
            .index()
            .find_current_callable_override_family(
                &SymbolTarget::for_symbol(&base),
                query_engine.index().current_workspace_revision(),
            )
            .unwrap();
        assert_eq!(family.len(), 2);
        assert!(family.iter().any(|definition| definition
            .target
            .fqn
            .starts_with("legality.Covariant.make(")));

        let array_base = query_engine
            .index()
            .find_methods_by_name("legality.ArrayBase", "array")
            .into_iter()
            .next()
            .unwrap();
        let array_family = query_engine
            .index()
            .find_current_callable_override_family(
                &SymbolTarget::for_symbol(&array_base),
                query_engine.index().current_workspace_revision(),
            )
            .unwrap();
        assert_eq!(array_family.len(), 2, "reference arrays are covariant");

        let generic_base = query_engine
            .index()
            .find_methods_by_name("legality.GenericReturnBase", "generic")
            .into_iter()
            .next()
            .unwrap();
        let generic_family = query_engine
            .index()
            .find_current_callable_override_family(
                &SymbolTarget::for_symbol(&generic_base),
                query_engine.index().current_workspace_revision(),
            )
            .unwrap();
        assert_eq!(
            generic_family.len(),
            2,
            "same-owner wildcard return containment is provable"
        );

        for owner in ["legality.SwappedReturnBase", "legality.SwappedReturnChild"] {
            let method = query_engine
                .index()
                .find_methods_by_name(owner, "swapped")
                .into_iter()
                .next()
                .unwrap();
            assert_eq!(
                query_engine
                    .index()
                    .find_current_callable_override_family(
                        &SymbolTarget::for_symbol(&method),
                        query_engine.index().current_workspace_revision(),
                    )
                    .unwrap()
                    .len(),
                1,
                "projected, swapped type arguments prove that {owner} is not override-related"
            );
        }

        for owner in ["legality.Weak", "legality.Incompatible"] {
            let method = query_engine
                .index()
                .find_methods_by_name(owner, "make")
                .into_iter()
                .next()
                .unwrap();
            assert_eq!(
                query_engine
                    .index()
                    .find_current_callable_override_family(
                        &SymbolTarget::for_symbol(&method),
                        query_engine.index().current_workspace_revision(),
                    )
                    .unwrap()
                    .len(),
                1,
                "illegal override edge for {owner} must stay disconnected"
            );
        }

        for owner in ["legality.Closed", "legality.InvalidClosedChild"] {
            let method = query_engine
                .index()
                .find_methods_by_name(owner, "run")
                .into_iter()
                .next()
                .unwrap();
            assert_eq!(
                query_engine
                    .index()
                    .find_current_callable_override_family(
                        &SymbolTarget::for_symbol(&method),
                        query_engine.index().current_workspace_revision(),
                    )
                    .unwrap()
                    .len(),
                1
            );
        }
    }

    #[tokio::test]
    async fn exact_callable_family_keeps_overload_usage_resolution() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().to_path_buf();
        let service_path = root.join("Service.java");
        let main_path = root.join("Main.java");
        std::fs::write(
            &service_path,
            concat!(
                "package family; public class Service { ",
                "public void choose(long value) {} ",
                "public void choose(Integer value) {} ",
                "public void cross(short left, float right) {} ",
                "public void cross(int left, int right) {} ",
                "public void pick(Object value) {} ",
                "public void pick(long value) {} ",
                "public void spread(String... values) {} ",
                "public void spread(Object... values) {} ",
                "public void phase(Object value) {} ",
                "public void phase(String... values) {} ",
                "public void invalid(String value) {} }"
            ),
        )
        .unwrap();
        std::fs::write(
            &main_path,
            concat!(
                "package family; public class Main { ",
                "public void run(Service service, byte left, byte right, Integer boxed) { ",
                "service.choose(1); service.cross(left, right); ",
                "service.pick(boxed); service.spread(\"one\"); ",
                "service.phase(\"one\"); service.invalid(1); } }"
            ),
        )
        .unwrap();

        let index = WorkspaceIndex::new(root);
        index.index_file(&service_path).await.unwrap();
        index.index_file(&main_path).await.unwrap();
        let main_id = index.get_file_id(&main_path).unwrap();
        let long_method = index
            .find_methods_by_name("family.Service", "choose")
            .into_iter()
            .find(|method| method.fqn.ends_with("(long)"))
            .unwrap();
        let target = SymbolTarget::for_symbol(&long_method);
        let direct_usages = index
            .find_current_usages_for_symbol_target(&target)
            .unwrap();
        assert!(
            direct_usages
                .locations()
                .any(|location| location.file_id == main_id),
            "exact overload usage extraction must retain primitive widening"
        );
        let query_engine = QueryEngine::new(index);
        let references = query_engine
            .find_current_references_target(&target)
            .unwrap();
        assert!(
            references
                .locations()
                .any(|location| location.file_id == main_id),
            "override expansion must preserve exact overload usage resolution"
        );

        for cross in query_engine
            .index()
            .find_methods_by_name("family.Service", "cross")
        {
            let usages = query_engine
                .index()
                .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(&cross))
                .unwrap();
            assert!(
                usages
                    .locations()
                    .all(|location| location.file_id != main_id),
                "incomparable primitive signatures must remain ambiguous"
            );
        }

        let picks = query_engine
            .index()
            .find_methods_by_name("family.Service", "pick");
        let object_pick = picks
            .iter()
            .find(|method| method.fqn.ends_with("(java.lang.Object)"))
            .or_else(|| picks.iter().find(|method| method.fqn.ends_with("(Object)")))
            .unwrap();
        let long_pick = picks
            .iter()
            .find(|method| method.fqn.ends_with("(long)"))
            .unwrap();
        assert!(query_engine
            .index()
            .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(object_pick))
            .unwrap()
            .locations()
            .any(|location| location.file_id == main_id));
        assert!(query_engine
            .index()
            .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(long_pick))
            .unwrap()
            .locations()
            .all(|location| location.file_id != main_id));

        let spreads = query_engine
            .index()
            .find_methods_by_name("family.Service", "spread");
        let string_spread = spreads
            .iter()
            .find(|method| {
                method.fqn.ends_with("(java.lang.String...)") || method.fqn.ends_with("(String...)")
            })
            .unwrap();
        let object_spread = spreads
            .iter()
            .find(|method| {
                method.fqn.ends_with("(java.lang.Object...)") || method.fqn.ends_with("(Object...)")
            })
            .unwrap();
        assert!(query_engine
            .index()
            .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(string_spread))
            .unwrap()
            .locations()
            .any(|location| location.file_id == main_id));
        assert!(query_engine
            .index()
            .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(object_spread))
            .unwrap()
            .locations()
            .all(|location| location.file_id != main_id));

        let phases = query_engine
            .index()
            .find_methods_by_name("family.Service", "phase");
        let fixed_object = phases
            .iter()
            .find(|method| {
                method.fqn.ends_with("(java.lang.Object)") || method.fqn.ends_with("(Object)")
            })
            .unwrap();
        let vararg_string = phases
            .iter()
            .find(|method| {
                method.fqn.ends_with("(java.lang.String...)") || method.fqn.ends_with("(String...)")
            })
            .unwrap();
        assert!(query_engine
            .index()
            .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(fixed_object))
            .unwrap()
            .locations()
            .any(|location| location.file_id == main_id));
        assert!(query_engine
            .index()
            .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(vararg_string))
            .unwrap()
            .locations()
            .all(|location| location.file_id != main_id));

        let invalid = query_engine
            .index()
            .find_methods_by_name("family.Service", "invalid")
            .into_iter()
            .next()
            .unwrap();
        assert!(query_engine
            .index()
            .find_current_usages_for_symbol_target(&SymbolTarget::for_symbol(&invalid))
            .unwrap()
            .locations()
            .all(|location| location.file_id != main_id));
    }
}
