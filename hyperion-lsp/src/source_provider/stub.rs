// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bytecode-stub fallback for declarations that have no source anywhere.
//!
//! A JDK without `src.zip` (distribution packages often omit it) and library
//! jars published without a `-sources` archive leave go-to-definition with
//! nothing to open. Instead of "No definition found" the class file of the
//! declaring type is rendered as a signatures-only Java stub
//! ([`hyperion_bytecode::stub`]) and served through the same read-only
//! materialized-source store as real library sources.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use tracing::debug;
use zip::ZipArchive;

use super::temp_file::AttachmentLanguage;
use super::{MaterializedSourceStore, SourceContent, SourceFile};

/// Largest class file (per entry) that is rendered.
const MAX_CLASS_BYTES: u64 = 16 * 1024 * 1024;
/// Most member types rendered into one stub.
const MAX_NESTED_CLASSES: usize = 400;
/// Archives above this size are not opened for stubs.
const MAX_ARCHIVE_BYTES: u64 = 700 * 1024 * 1024;
/// Rendered stubs kept for repeated navigation to the same type.
const MAX_CACHED_STUBS: usize = 128;
const JMOD_HEADER_BYTES: u64 = 4;

type StubCache = Mutex<HashMap<(PathBuf, String), Arc<SourceFile>>>;

fn cache() -> &'static StubCache {
    static CACHE: OnceLock<StubCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Serve a stub for `fqn` (a type, or a member of one) declared in the class
/// archive at `archive_path` (a jar or a JDK `.jmod`).
pub(crate) fn stub_source_content(
    store: &MaterializedSourceStore,
    archive_path: &Path,
    fqn: &str,
) -> Option<SourceContent> {
    let mut archive = open_archive(archive_path)?;
    let (top_entry, top_binary_name) = find_top_level_entry(&mut archive, fqn)?;
    let key = (archive_path.to_path_buf(), top_binary_name.clone());
    let cached = cache().lock().get(&key).cloned();
    let source = if let Some(cached) = cached {
        cached
    } else {
        let stub = render(&mut archive, &top_entry, &top_binary_name)?;
        let source = Arc::new(SourceFile::new(Arc::from(stub)));
        let mut cache = cache().lock();
        if cache.len() >= MAX_CACHED_STUBS {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&source));
        source
    };
    let temp_file = store
        .materialize(source.content().as_bytes(), AttachmentLanguage::Java)
        .map_err(|error| debug!("stub for {fqn} could not be materialized: {error}"))
        .ok()?;
    Some(SourceContent::Stub { source, temp_file })
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A JMOD is a ZIP with a 4-byte magic header in front; its offsets are
/// relative to the data after it.
struct OffsetReader {
    inner: File,
    offset: u64,
}

impl Read for OffsetReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Seek for OffsetReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let translated = match position {
            SeekFrom::Start(start) => SeekFrom::Start(
                start
                    .checked_add(self.offset)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek overflow"))?,
            ),
            other => other,
        };
        self.inner
            .seek(translated)
            .map(|absolute| absolute.saturating_sub(self.offset))
    }
}

struct ClassArchive {
    zip: ZipArchive<Box<dyn ReadSeek>>,
    /// Entry-name prefix of class files (`classes/` in a JMOD).
    prefix: &'static str,
}

fn open_archive(path: &Path) -> Option<ClassArchive> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_ARCHIVE_BYTES {
        return None;
    }
    let is_jmod = path.extension().and_then(|extension| extension.to_str()) == Some("jmod");
    let reader: Box<dyn ReadSeek> = if is_jmod {
        Box::new(OffsetReader {
            inner: file,
            offset: JMOD_HEADER_BYTES,
        })
    } else {
        Box::new(file)
    };
    Some(ClassArchive {
        zip: ZipArchive::new(reader).ok()?,
        prefix: if is_jmod { "classes/" } else { "" },
    })
}

/// The class file entry of the top-level type that owns `fqn`: peel member and
/// parameter suffixes off until an entry exists (`java.util.Map.Entry.getKey()`
/// -> `java/util/Map.class`).
fn find_top_level_entry(archive: &mut ClassArchive, fqn: &str) -> Option<(String, String)> {
    let type_path = fqn.split_once('(').map_or(fqn, |(before, _)| before);
    let mut candidate = type_path;
    for _ in 0..12 {
        let binary_name = candidate.replace('.', "/");
        let entry = format!("{}{binary_name}.class", archive.prefix);
        if archive.zip.by_name(&entry).is_ok() {
            return Some((entry, binary_name));
        }
        candidate = candidate.rsplit_once('.')?.0;
    }
    None
}

fn read_entry(archive: &mut ClassArchive, name: &str) -> Option<Vec<u8>> {
    let entry = archive.zip.by_name(name).ok()?;
    if entry.size() > MAX_CLASS_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).ok()?);
    entry.take(MAX_CLASS_BYTES).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn render(archive: &mut ClassArchive, top_entry: &str, top_binary_name: &str) -> Option<String> {
    let top = read_entry(archive, top_entry)?;
    let nested_prefix = format!("{}{top_binary_name}$", archive.prefix);
    let nested_names: Vec<String> = archive
        .zip
        .file_names()
        .filter_map(|name| match name {
            Ok(name)
                if name.starts_with(&nested_prefix)
                    && Path::new(name.as_ref())
                        .extension()
                        .is_some_and(|extension| extension == "class") =>
            {
                Some(Ok(name.into_owned()))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .take(MAX_NESTED_CLASSES)
        .collect::<Result<_, _>>()
        .ok()?;
    let nested: Vec<Vec<u8>> = nested_names
        .iter()
        .filter_map(|name| read_entry(archive, name))
        .collect();
    hyperion_bytecode::stub::render_class_stub(&top, &nested)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_zip(path: &Path, prefix_header: &[u8], entries: &[(&str, &[u8])]) {
        // Built in memory so the directory offsets are relative to the ZIP data
        // itself, as the `jmod` tool writes them.
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        let data = zip.finish().unwrap().into_inner();
        let mut file = File::create(path).unwrap();
        file.write_all(prefix_header).unwrap();
        file.write_all(&data).unwrap();
    }

    #[test]
    fn top_level_entries_are_found_from_member_fqns_in_jars_and_jmods() {
        let temp = tempfile::tempdir().unwrap();
        let jar = temp.path().join("lib.jar");
        write_zip(
            &jar,
            b"",
            &[("demo/Map.class", b"x"), ("demo/Map$Entry.class", b"x")],
        );
        let mut archive = open_archive(&jar).unwrap();
        assert_eq!(
            find_top_level_entry(&mut archive, "demo.Map.Entry.getKey()"),
            Some(("demo/Map.class".to_owned(), "demo/Map".to_owned()))
        );
        assert_eq!(find_top_level_entry(&mut archive, "demo.Missing.x"), None);

        // The ZIP directory of a JMOD is offset by its 4-byte header.
        let jmod = temp.path().join("m.jmod");
        write_zip(&jmod, b"JM\x01\x00", &[("classes/demo/Map.class", b"x")]);
        let mut archive = open_archive(&jmod).unwrap();
        assert_eq!(
            find_top_level_entry(&mut archive, "demo.Map"),
            Some(("classes/demo/Map.class".to_owned(), "demo/Map".to_owned()))
        );
    }
}
