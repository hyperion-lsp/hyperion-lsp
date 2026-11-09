// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Source Provider System
//!
//! Provides source code for symbols that don't exist as workspace files,
//! including JDK classes, external dependencies, and decompiled bytecode.
//!
//! This enables goto-definition to work for all symbols, not just workspace files.

mod archive_snapshot;
mod cache;
mod chain;
pub mod content;
mod dependency_archive;
pub mod jar;
pub mod jdk;
mod stub;
mod temp_file;

pub use chain::SourceProviderChain;
pub(crate) use chain::{
    begin_coordinated_external_publication, SourceProviderPublication, SourceProviderRefreshGuard,
};
#[cfg(test)]
use chain::{PreparedPublicationCommitError, ProviderEntry, ProviderHandle};
pub use content::{SourceContent, SourceFile};
#[cfg(test)]
pub(crate) use dependency_archive::MAX_BINARY_ARCHIVE_SNAPSHOT_BYTES;
pub(crate) use dependency_archive::{
    capture_dependency_archive_plan, prepare_dependency_archive, DependencyArchivePlan,
};
pub use jar::JarSourceProvider;
pub use jdk::JdkSourceProvider;
pub(crate) use stub::stub_source_content;
pub use temp_file::MaterializedSourceStore;

use std::collections::HashSet;
use std::sync::Arc;

use hyperion_workspace::{ExternalSourceIdentity, ExternalSourceRevisionToken};

/// Exact external declaration selected by the workspace index.
///
/// `source_identity` is returned by
/// [`hyperion_workspace::WorkspaceIndex::get_external_source_identity`].
/// Providers must match it before considering the FQN, so duplicate classes in
/// different archives cannot cross provider or archive boundaries.
#[derive(Debug, Clone, Copy)]
pub struct ExternalSourceRequest<'a> {
    source_identity: &'a ExternalSourceIdentity,
    source_revision: Option<ExternalSourceRevisionToken>,
    fqn: &'a str,
}

impl<'a> ExternalSourceRequest<'a> {
    pub const fn new(source_identity: &'a ExternalSourceIdentity, fqn: &'a str) -> Self {
        Self {
            source_identity,
            source_revision: None,
            fqn,
        }
    }

    /// Construct a request for one exact published source generation.
    pub const fn for_revision(
        source_identity: &'a ExternalSourceIdentity,
        source_revision: ExternalSourceRevisionToken,
        fqn: &'a str,
    ) -> Self {
        Self {
            source_identity,
            source_revision: Some(source_revision),
            fqn,
        }
    }

    pub const fn source_identity(self) -> &'a ExternalSourceIdentity {
        self.source_identity
    }

    pub const fn source_revision(self) -> Option<ExternalSourceRevisionToken> {
        self.source_revision
    }

    pub const fn fqn(self) -> &'a str {
        self.fqn
    }
}

/// Provides source code for symbols that don't exist as workspace files
pub trait SourceProvider: Send + Sync {
    /// Legacy FQN-only lookup retained for source compatibility.
    fn get_source(&self, _fqn: &str) -> Option<SourceContent> {
        None
    }

    /// Legacy FQN-only capability check retained for source compatibility.
    fn can_provide(&self, _fqn: &str) -> bool {
        false
    }

    /// Get source content for one exact external declaration identity.
    ///
    /// Legacy FQN-only providers cannot prove ownership of `source_id`, so the
    /// default deliberately fails closed. Providers that participate in exact
    /// external navigation must opt in and check both identity components.
    fn get_source_exact(&self, _request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
        None
    }

    /// Check if this provider owns the exact source/FQN pair.
    fn can_provide_exact(&self, _request: ExternalSourceRequest<'_>) -> bool {
        false
    }

    /// Whether this provider is authoritative for one exact archive identity.
    ///
    /// Once an authoritative provider is reached, a missing FQN is terminal:
    /// falling through could return source from another build generation which
    /// happens to reference the same archive path.
    fn owns_source_identity(&self, _source_identity: &ExternalSourceIdentity) -> bool {
        false
    }

    /// Retain immutable snapshots only for the requested external identities.
    ///
    /// Refreshable providers override this so partial-failure fallback does not
    /// retain whole obsolete classpaths. Providers that cannot prove a bounded
    /// exact subset return `None` and therefore fail closed.
    fn snapshot_for_sources(
        &self,
        _source_identities: &HashSet<ExternalSourceIdentity>,
    ) -> Option<Arc<dyn SourceProvider>> {
        None
    }

    /// Provider priority (lower = higher priority)
    fn priority(&self) -> u32;

    /// Human-readable name for logging
    fn name(&self) -> &'static str;
}

#[cfg(test)]
mod tests;
