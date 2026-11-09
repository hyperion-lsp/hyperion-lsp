// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bounded UTF-8 source-file reads shared by indexing and LSP consumers.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use tokio::io::AsyncReadExt;

/// Maximum accepted size for one source file.
pub const MAX_SOURCE_FILE_BYTES: usize = 16 * 1024 * 1024;

/// Read a UTF-8 file up to a caller-provided limit.
pub fn read_utf8_file_bounded(path: &Path, maximum: usize) -> io::Result<String> {
    let file = File::open(path)?;
    let declared_size = file.metadata()?.len();
    reject_oversized_metadata(declared_size, maximum)?;

    let mut bytes = Vec::with_capacity(declared_size as usize);
    let read_limit = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    file.take(read_limit).read_to_end(&mut bytes)?;
    decode_utf8(bytes, maximum)
}

/// Read one source file without trusting metadata or allowing unbounded allocation.
pub fn read_source_file(path: &Path) -> io::Result<String> {
    read_utf8_file_bounded(path, MAX_SOURCE_FILE_BYTES)
}

/// Async counterpart to [`read_source_file`].
pub async fn read_source_file_async(path: &Path) -> io::Result<String> {
    let file = tokio::fs::File::open(path).await?;
    let declared_size = file.metadata().await?.len();
    reject_oversized_metadata(declared_size, MAX_SOURCE_FILE_BYTES)?;

    let mut bytes = Vec::with_capacity(declared_size as usize);
    let mut limited = file.take(MAX_SOURCE_FILE_BYTES as u64 + 1);
    limited.read_to_end(&mut bytes).await?;
    decode_utf8(bytes, MAX_SOURCE_FILE_BYTES)
}

fn reject_oversized_metadata(size: u64, maximum: usize) -> io::Result<()> {
    if size > u64::try_from(maximum).unwrap_or(u64::MAX) {
        return Err(oversized_file_error(maximum));
    }
    Ok(())
}

fn decode_utf8(bytes: Vec<u8>, maximum: usize) -> io::Result<String> {
    if bytes.len() > maximum {
        return Err(oversized_file_error(maximum));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn oversized_file_error(maximum: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("file exceeds {maximum} bytes"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_oversized_and_invalid_sources() {
        assert_eq!(
            decode_utf8(vec![b' '; MAX_SOURCE_FILE_BYTES + 1], MAX_SOURCE_FILE_BYTES)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            decode_utf8(vec![0xff], MAX_SOURCE_FILE_BYTES)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
