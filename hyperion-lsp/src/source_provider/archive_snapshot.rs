// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Immutable, process-lifetime ZIP snapshots for source attachments.

use std::io::{self, Seek, SeekFrom};
use std::path::Path;
use tempfile::TempPath;

use super::MaterializedSourceStore;

/// Copy an archive into a private named temporary path, then close every
/// descriptor used for staging.
///
/// Dependency source providers can retain hundreds of attachments. Keeping a
/// `ZipArchive<File>` per attachment exhausts modest process descriptor limits,
/// especially while old and staged generations overlap. A `TempPath` retains
/// immutable ownership without an open descriptor; source lookup reopens only
/// the one archive it needs.
pub(super) fn snapshot_to_temp_path(
    path: &Path,
    maximum_bytes: u64,
    store: &MaterializedSourceStore,
) -> Result<(TempPath, u64), String> {
    let snapshot = hyperion_workspace::ArchiveSnapshot::capture(path, maximum_bytes)?;
    let copied = snapshot.bytes();
    let mut input = snapshot.into_file();
    input
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let directory = store.archive_snapshot_directory()?;
    let mut output =
        tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    let written = io::copy(&mut input, &mut output).map_err(|error| error.to_string())?;
    if written != copied {
        return Err(format!(
            "archive snapshot copy wrote {written} bytes, expected {copied}"
        ));
    }
    Ok((output.into_temp_path(), copied))
}
