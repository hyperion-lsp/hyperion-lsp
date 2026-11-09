// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bounded file I/O shared by build-system integrations.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Maximum size accepted for Gradle scripts, settings files, catalogs, and Maven POMs.
pub(crate) const MAX_BUILD_FILE_BYTES: usize = 16 * 1024 * 1024;

/// Read a UTF-8 file without trusting metadata or allowing unbounded growth.
pub(crate) fn read_utf8_file_bounded(path: &Path, maximum: usize) -> io::Result<String> {
    let file = File::open(path)?;
    if file.metadata()?.len() > maximum as u64 {
        return Err(oversized_file_error(maximum));
    }
    read_utf8_bounded(file, maximum)
}

fn read_utf8_bounded(reader: impl Read, maximum: usize) -> io::Result<String> {
    let mut bytes = Vec::new();
    reader.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(oversized_file_error(maximum));
    }
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn oversized_file_error(maximum: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("file exceeds the {maximum}-byte limit"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_reader_accepts_text_at_limit() {
        assert_eq!(read_utf8_bounded("text".as_bytes(), 4).unwrap(), "text");
    }

    #[test]
    fn bounded_reader_rejects_content_past_limit() {
        let error = read_utf8_bounded("large".as_bytes(), 4).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn bounded_reader_reports_invalid_utf8_as_data_error() {
        let error = read_utf8_bounded([0xff].as_slice(), 1).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
