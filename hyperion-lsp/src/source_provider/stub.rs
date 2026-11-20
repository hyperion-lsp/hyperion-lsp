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
/// Declaration paths remembered as belonging to a cached stub.
const MAX_CACHED_OWNERS: usize = 8192;
const JMOD_HEADER_BYTES: u64 = 4;

#[derive(Default)]
struct StubCache {
    /// (archive, binary name of a top-level type) -> its rendered stub.
    stubs: HashMap<(PathBuf, String), Arc<SourceFile>>,
    /// (archive, dotted path of a type or member) -> binary name of the
    /// top-level type that declares it, as [`find_top_level_entry`] found it.
    ///
    /// Finding it means opening the archive, and opening `java.base.jmod`
    /// parses a central directory of several thousand entries. Requests that
    /// resolve many calls into one library (inlay hints) spent most of their
    /// time doing that again for every call.
    owners: HashMap<(PathBuf, String), String>,
    /// Size and modification time of each archive when its entries above were
    /// read. An archive replaced in place (a JDK or dependency upgrade) must
    /// not keep serving the stubs of the classes it used to contain.
    stamps: HashMap<PathBuf, ArchiveStamp>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ArchiveStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}

impl ArchiveStamp {
    fn of(archive_path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(archive_path).ok()?;
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

impl StubCache {
    /// Forget everything read from `archive_path` unless it is still the
    /// file `stamp` describes.
    fn retain_current(&mut self, archive_path: &Path, stamp: ArchiveStamp) {
        if self.stamps.get(archive_path) == Some(&stamp) {
            return;
        }
        self.stubs.retain(|(archive, _), _| archive != archive_path);
        self.owners
            .retain(|(archive, _), _| archive != archive_path);
        self.stamps.insert(archive_path.to_path_buf(), stamp);
    }
}

fn cache() -> &'static Mutex<StubCache> {
    static CACHE: OnceLock<Mutex<StubCache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// `fqn` without a method's parameter list.
fn type_path(fqn: &str) -> &str {
    fqn.split_once('(').map_or(fqn, |(before, _)| before)
}

fn cached_stub(archive_path: &Path, stamp: ArchiveStamp, fqn: &str) -> Option<Arc<SourceFile>> {
    let mut cache = cache().lock();
    cache.retain_current(archive_path, stamp);
    let owner = cache
        .owners
        .get(&(archive_path.to_path_buf(), type_path(fqn).to_owned()))?;
    cache
        .stubs
        .get(&(archive_path.to_path_buf(), owner.clone()))
        .cloned()
}

/// Serve a stub for `fqn` (a type, or a member of one) declared in the class
/// archive at `archive_path` (a jar or a JDK `.jmod`).
pub(crate) fn stub_source_content(
    store: &MaterializedSourceStore,
    archive_path: &Path,
    fqn: &str,
) -> Option<SourceContent> {
    let stamp = ArchiveStamp::of(archive_path)?;
    let source = if let Some(cached) = cached_stub(archive_path, stamp, fqn) {
        cached
    } else {
        let mut archive = open_archive(archive_path)?;
        let (top_entry, top_binary_name) = find_top_level_entry(&mut archive, fqn)?;
        let key = (archive_path.to_path_buf(), top_binary_name.clone());
        let cached = cache().lock().stubs.get(&key).cloned();
        let source = if let Some(cached) = cached {
            cached
        } else {
            let stub = render(&mut archive, &top_entry, &top_binary_name)?;
            Arc::new(SourceFile::new(Arc::from(stub)))
        };
        let mut cache = cache().lock();
        if cache.stubs.len() >= MAX_CACHED_STUBS || cache.owners.len() >= MAX_CACHED_OWNERS {
            cache.stubs.clear();
            cache.owners.clear();
            cache.stamps.clear();
        }
        cache.retain_current(archive_path, stamp);
        cache.stubs.insert(key, Arc::clone(&source));
        cache.owners.insert(
            (archive_path.to_path_buf(), type_path(fqn).to_owned()),
            top_binary_name,
        );
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
    let mut candidate = type_path(fqn);
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
    fn a_replaced_archive_forgets_only_its_own_cached_entries() {
        let stamp = |len| ArchiveStamp {
            len,
            modified: None,
        };
        let stub = || Arc::new(SourceFile::new(Arc::from("class Stub {}")));
        let (jdk, library) = (
            PathBuf::from("/jdk/java.base.jmod"),
            PathBuf::from("/lib.jar"),
        );
        let mut cache = StubCache::default();
        for archive in [&jdk, &library] {
            cache.retain_current(archive, stamp(1));
            cache
                .stubs
                .insert((archive.clone(), "p/Owner".to_owned()), stub());
            cache.owners.insert(
                (archive.clone(), "p.Owner.member".to_owned()),
                "p/Owner".to_owned(),
            );
        }

        // Same file as before: nothing is dropped.
        cache.retain_current(&jdk, stamp(1));
        assert_eq!((cache.stubs.len(), cache.owners.len()), (2, 2));

        // The JDK image was replaced in place; the library was not.
        cache.retain_current(&jdk, stamp(2));
        assert!(cache.stubs.keys().all(|(archive, _)| archive == &library));
        assert!(cache.owners.keys().all(|(archive, _)| archive == &library));
        assert_eq!((cache.stubs.len(), cache.owners.len()), (1, 1));
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
