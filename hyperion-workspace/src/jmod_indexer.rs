// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JMOD file indexing support for Java 9+
//!
//! JMOD files are ZIP files with a 4-byte magic header "JM\x01\x00"
//! Class files are stored under classes/ directory (not at root like JARs)

use anyhow::{Context, Result};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use tracing::{debug, warn};
use zip::ZipArchive;

use crate::jar_indexer::{
    ArchiveIndexData, ArchiveWorkBudget, ClassIndexData, JarIndexStats, MAX_ARCHIVE_ENTRIES,
};
use crate::{FileId, Symbol};

/// JMOD file magic header (4 bytes: "JM\x01\x00")
const JMOD_MAGIC: &[u8] = &[0x4A, 0x4D, 0x01, 0x00];

/// Maximum size for a JMOD file (500MB). JDK's java.base.jmod is ~130MB.
const MAX_JMOD_FILE_SIZE: u64 = 500 * 1024 * 1024;

/// Maximum size for a single class file within a JMOD (16 MiB).
const MAX_CLASS_FILE_SIZE: u64 = 16 * 1024 * 1024;

/// Extract symbols from a JMOD file
///
/// JMOD files are ZIP archives with a 4-byte magic header.
/// We need to skip the header to read them as standard ZIP files.
pub fn extract_jmod_symbols(
    jmod_path: &Path,
    file_id: FileId,
) -> Result<(Vec<Symbol>, JarIndexStats)> {
    let (data, stats) = extract_jmod_index_data(jmod_path, file_id)?;
    Ok((data.symbols, stats))
}

pub(crate) fn extract_jmod_index_data(
    jmod_path: &Path,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    let file = File::open(jmod_path)
        .with_context(|| format!("Failed to open JMOD file: {}", jmod_path.display()))?;

    extract_jmod_index_data_from_file(file, jmod_path, file_id)
}

/// Extract symbols from an immutable private JMOD snapshot.
pub fn extract_jmod_symbols_from_snapshot(
    snapshot: crate::ArchiveSnapshot,
    file_id: FileId,
) -> Result<(Vec<Symbol>, JarIndexStats)> {
    let (data, stats) = extract_jmod_index_data_from_snapshot(snapshot, file_id)?;
    Ok((data.symbols, stats))
}

pub(crate) fn extract_jmod_index_data_from_snapshot(
    snapshot: crate::ArchiveSnapshot,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    let display_path = snapshot.display_path().to_path_buf();
    extract_jmod_index_data_from_file(snapshot.into_file(), &display_path, file_id)
}

fn extract_jmod_index_data_from_file(
    mut file: File,
    jmod_path: &Path,
    file_id: FileId,
) -> Result<(ArchiveIndexData, JarIndexStats)> {
    // Check file size before reading (security limit)
    let file_size = file.metadata()?.len();
    if file_size > MAX_JMOD_FILE_SIZE {
        anyhow::bail!(
            "JMOD file too large: {} bytes > {} max",
            file_size,
            MAX_JMOD_FILE_SIZE
        );
    }

    // Read and verify magic header
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;

    if magic != JMOD_MAGIC {
        anyhow::bail!(
            "Invalid JMOD magic header: expected {:?}, got {:?}",
            JMOD_MAGIC,
            magic
        );
    }

    // ZIP offsets inside a JMOD are relative to the data after its four-byte
    // header. Translate seeks instead of copying the entire archive to RAM.
    let reader = OffsetReader::new(file, JMOD_MAGIC.len() as u64)?;
    let mut archive = ZipArchive::new(reader)
        .with_context(|| format!("Failed to read JMOD archive: {}", jmod_path.display()))?;

    let mut data = ArchiveIndexData::default();
    let mut stats = JarIndexStats::default();
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        anyhow::bail!(
            "JMOD contains {} entries (maximum {})",
            archive.len(),
            MAX_ARCHIVE_ENTRIES
        );
    }
    let mut budget = ArchiveWorkBudget::standard("JMOD");

    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name()?.into_owned();

        // Only process files under classes/ directory ending with .class
        if !name.starts_with("classes/") || !name.ends_with(".class") {
            continue;
        }

        // Skip module-info and package-info
        if name.ends_with("/module-info.class") || name.ends_with("/package-info.class") {
            continue;
        }

        stats.classes_found += 1;

        // Check file size before reading (ZIP bomb protection)
        let uncompressed_size = file.size();
        if uncompressed_size > MAX_CLASS_FILE_SIZE {
            warn!(
                "Skipping oversized class file {} ({} bytes > {} max)",
                name, uncompressed_size, MAX_CLASS_FILE_SIZE
            );
            continue;
        }

        // Read class file bytes with size limit
        let mut bytes = Vec::with_capacity(uncompressed_size as usize);
        if let Err(e) = file.take(MAX_CLASS_FILE_SIZE + 1).read_to_end(&mut bytes) {
            warn!("Failed to read class file {}: {}", name, e);
            continue;
        }
        if bytes.len() as u64 > MAX_CLASS_FILE_SIZE {
            warn!(
                "Skipping class file {} that expanded beyond the size limit",
                name
            );
            continue;
        }
        budget.record_class(bytes.len() as u64)?;

        // Remove "classes/" prefix for class name parsing
        let class_path = name.strip_prefix("classes/").unwrap_or(&name);

        // Parse class file
        match parse_jmod_class_file_data(&bytes, class_path, file_id) {
            Ok(mut class_data) => {
                budget.record_symbols(class_data.symbols.len())?;
                stats.record_class_symbols(&class_data.symbols);

                data.symbols.append(&mut class_data.symbols);
                data.generic_supertypes
                    .append(&mut class_data.generic_supertypes);
            }
            Err(e) => {
                debug!("Failed to parse class file {}: {}", name, e);
                continue;
            }
        }
    }

    debug!(
        "Indexed JMOD {}: {} classes, {} methods, {} fields",
        jmod_path.display(),
        stats.classes_indexed,
        stats.methods_indexed,
        stats.fields_indexed
    );

    Ok((data, stats))
}

struct OffsetReader<R> {
    inner: R,
    offset: u64,
}

impl<R: Seek> OffsetReader<R> {
    fn new(mut inner: R, offset: u64) -> io::Result<Self> {
        inner.seek(SeekFrom::Start(offset))?;
        Ok(Self { inner, offset })
    }

    fn logical_position(&self, position: u64) -> io::Result<u64> {
        position.checked_sub(self.offset).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "archive seek moved before the JMOD header",
            )
        })
    }
}

impl<R: Read> Read for OffsetReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl<R: Seek> Seek for OffsetReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let physical = match position {
            SeekFrom::Start(position) => {
                let position = self.offset.checked_add(position).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "archive seek overflow")
                })?;
                self.inner.seek(SeekFrom::Start(position))?
            }
            SeekFrom::End(position) => self.inner.seek(SeekFrom::End(position))?,
            SeekFrom::Current(position) => self.inner.seek(SeekFrom::Current(position))?,
        };
        self.logical_position(physical)
    }
}

fn parse_jmod_class_file_data(
    bytes: &[u8],
    class_path: &str,
    file_id: FileId,
) -> Result<ClassIndexData> {
    crate::jar_indexer::parse_class_file_data(bytes, class_path, file_id)
}

/// Check if a file is a JMOD file by reading its magic header
pub fn is_jmod_file(path: &Path) -> bool {
    if let Ok(mut file) = File::open(path) {
        let mut magic = [0u8; 4];
        if file.read_exact(&mut magic).is_ok() {
            return magic == JMOD_MAGIC;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_test_jmod(path: &Path, entry_name: &str, contents: &[u8]) {
        let mut zip_data = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut zip_data);
            writer
                .start_file(entry_name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
            writer.finish().unwrap();
        }
        let mut jmod = JMOD_MAGIC.to_vec();
        jmod.extend_from_slice(zip_data.get_ref());
        std::fs::write(path, jmod).unwrap();
    }

    #[test]
    fn jmod_jvm_constructor_statistics_match_jar_and_published_symbols() {
        let temp = tempfile::tempdir().unwrap();
        let source_path = temp.path().join("Example.java");
        std::fs::write(
            &source_path,
            "public class Example { public int field; public Example() {} public void method() {} }",
        ).unwrap();
        let output = std::process::Command::new("javac")
            .arg("-d")
            .arg(temp.path())
            .arg(&source_path)
            .output()
            .expect("run javac for JMOD fixture");
        assert!(
            output.status.success(),
            "javac failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bytes = std::fs::read(temp.path().join("Example.class")).unwrap();
        let jmod_path = temp.path().join("fixture.jmod");
        write_test_jmod(&jmod_path, "classes/Example.class", &bytes);
        let (symbols, stats) = extract_jmod_symbols(&jmod_path, FileId::new(1)).unwrap();
        assert!(symbols
            .iter()
            .any(|symbol| symbol.kind == lsp_types::SymbolKind::CONSTRUCTOR));
        assert_eq!(stats.classes_indexed, 1);
        assert_eq!(stats.methods_indexed, 2); // constructor and ordinary method
        assert_eq!(stats.fields_indexed, 1);
        assert_eq!(
            symbols.len(),
            stats.classes_indexed + stats.methods_indexed + stats.fields_indexed
        );

        let jar_path = temp.path().join("fixture.jar");
        let mut jar = zip::ZipWriter::new(std::fs::File::create(&jar_path).unwrap());
        jar.start_file("Example.class", zip::write::SimpleFileOptions::default())
            .unwrap();
        jar.write_all(&bytes).unwrap();
        jar.finish().unwrap();
        let (jar_symbols, jar_stats) =
            crate::extract_jar_symbols(&jar_path, FileId::new(2)).unwrap();
        assert_eq!(jar_symbols.len(), symbols.len());
        assert_eq!(jar_stats.classes_indexed, stats.classes_indexed);
        assert_eq!(jar_stats.methods_indexed, stats.methods_indexed);
        assert_eq!(jar_stats.fields_indexed, stats.fields_indexed);

        // Distinct archive entries containing the same class are extracted twice but published once per
        // symbol key. Extraction totals cannot validate cache snapshot totals.
        let mut zip_data = std::io::Cursor::new(Vec::new());
        {
            let mut duplicate = zip::ZipWriter::new(&mut zip_data);
            for entry in 0..2 {
                duplicate
                    .start_file(
                        format!("classes/copy{entry}/Example.class"),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                duplicate.write_all(&bytes).unwrap();
            }
            duplicate.finish().unwrap();
        }
        let mut duplicate_jmod = JMOD_MAGIC.to_vec();
        duplicate_jmod.extend_from_slice(zip_data.get_ref());
        std::fs::write(&jmod_path, duplicate_jmod).unwrap();
        let index = crate::WorkspaceIndex::new(temp.path().to_path_buf());
        let duplicate_stats = index.index_jmod_file(&jmod_path).unwrap();
        assert_eq!(duplicate_stats.classes_indexed, 2);
        assert_eq!(duplicate_stats.methods_indexed, 4);
        assert_eq!(index.get_external_symbols().len(), symbols.len());
    }

    #[test]
    fn test_jmod_detection() {
        // This test requires a real jmod file
        let jmod_path =
            std::path::PathBuf::from("/usr/lib/jvm/default-runtime/jmods/java.base.jmod");
        if jmod_path.exists() {
            assert!(is_jmod_file(&jmod_path), "Should detect jmod file");
        }
    }

    #[test]
    fn test_jmod_magic_header() {
        // Test that we correctly identify JMOD files by their magic header
        let magic = [0x4A, 0x4D, 0x01, 0x00]; // "JM\x01\x00"
        assert_eq!(magic, JMOD_MAGIC, "JMOD magic header should match");
    }

    #[test]
    fn offset_reader_exposes_prefixed_zip_without_copying() {
        let mut zip_data = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut zip_data);
            writer
                .start_file(
                    "classes/Example.class",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(b"fixture").unwrap();
            writer.finish().unwrap();
        }
        let mut prefixed = JMOD_MAGIC.to_vec();
        prefixed.extend_from_slice(zip_data.get_ref());
        let reader = OffsetReader::new(std::io::Cursor::new(prefixed), 4).unwrap();

        let mut archive = ZipArchive::new(reader).unwrap();
        let mut entry = archive.by_name("classes/Example.class").unwrap();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();

        assert_eq!(contents, b"fixture");
    }

    #[test]
    fn jmod_snapshot_publication_uses_the_captured_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let jmod_path = temp.path().join("fixture.jmod");
        write_test_jmod(&jmod_path, "classes/Example.class", b"not bytecode");

        let archive = crate::ExternalSourceArchive::capture(&jmod_path);
        let snapshot = archive.snapshot(MAX_JMOD_FILE_SIZE).unwrap();
        let index = crate::WorkspaceIndex::new(temp.path().to_path_buf());
        let ticket = index.begin_external_source_refresh("jdk-classpath");
        let refresh = index
            .replace_external_archives_for_refresh(&ticket, std::slice::from_ref(&archive))
            .unwrap();
        let revision = refresh
            .revision_for_identity(archive.identity())
            .expect("captured JMOD identity was reserved");

        std::fs::write(&jmod_path, b"replacement is not a JMOD").unwrap();
        assert!(extract_jmod_symbols(&jmod_path, FileId::new(1)).is_err());

        let stats = index
            .index_jmod_snapshot_for_revision(snapshot, revision)
            .expect("the immutable JMOD snapshot remains readable");
        assert_eq!(stats.classes_found, 1);
        assert_eq!(stats.classes_indexed, 0);
    }

    #[test]
    fn test_jmod_extraction() {
        // This test requires a real jmod file
        let jmod_path =
            std::path::PathBuf::from("/usr/lib/jvm/default-runtime/jmods/java.base.jmod");
        if !jmod_path.exists() {
            debug!("Skipping test_jmod_extraction: java.base.jmod not found");
            return;
        }

        let file_id = FileId::new(9999);
        let result = extract_jmod_symbols(&jmod_path, file_id);

        match result {
            Ok((symbols, stats)) => {
                // java.base should have many classes
                assert!(
                    stats.classes_indexed > 0,
                    "Should index at least some classes from java.base"
                );
                assert!(
                    stats.methods_indexed > 0,
                    "Should index at least some methods"
                );

                // Check for specific well-known classes
                let has_string = symbols.iter().any(|s| s.fqn.as_ref() == "java.lang.String");
                let has_list = symbols.iter().any(|s| s.fqn.as_ref() == "java.util.List");

                assert!(has_string, "Should find java.lang.String in java.base.jmod");
                assert!(has_list, "Should find java.util.List in java.base.jmod");

                println!(
                    "Indexed {} classes, {} methods, {} fields from java.base.jmod",
                    stats.classes_indexed, stats.methods_indexed, stats.fields_indexed
                );
            }
            Err(e) => {
                panic!("Failed to extract symbols from java.base.jmod: {}", e);
            }
        }
    }

    #[test]
    fn test_non_jmod_file_rejected() {
        // Create a temporary non-jmod file

        let temp_dir = std::env::temp_dir();
        let fake_jmod = temp_dir.join("fake.jmod");

        // Write some random data that's NOT a jmod
        std::fs::write(&fake_jmod, b"This is not a JMOD file").ok();

        // Should not be detected as jmod
        assert!(!is_jmod_file(&fake_jmod), "Should reject non-JMOD files");

        // Clean up
        std::fs::remove_file(&fake_jmod).ok();
    }
}
