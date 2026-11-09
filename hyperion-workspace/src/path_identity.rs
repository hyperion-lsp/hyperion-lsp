// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Filesystem-aware, stable path lookup keys.
//!
//! A target operating system does not determine path identity: macOS volumes
//! and Windows directories can opt into case-sensitive lookup, while filesystem
//! Unicode rules need not match Rust's Unicode version. Existing paths are
//! therefore resolved through the filesystem, while a missing suffix is kept
//! verbatim below the deepest ancestor whose identity can still be observed.

use std::ffi::OsString;
use std::ops::Deref;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Immutable key used for path identity in workspace hash maps and lock maps.
///
/// The inner representation is intentionally private. Callers may inspect the
/// normalized path but cannot construct a key without applying the filesystem
/// identity policy in [`path_lookup_key`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathLookupKey(Arc<Path>);

impl PathLookupKey {
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Share the normalized path allocation with another immutable owner.
    ///
    /// This is crate-private so callers still cannot manufacture unchecked
    /// lookup keys, while path registries can avoid storing the same absolute
    /// spelling twice when the display path already equals the identity key.
    pub(crate) fn shared_path(&self) -> Arc<Path> {
        Arc::clone(&self.0)
    }
}

impl Deref for PathLookupKey {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl AsRef<Path> for PathLookupKey {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

/// Construct the filesystem-aware lookup key for `path`.
///
/// The longest existing prefix is resolved before the missing suffix is
/// normalized, preserving filesystem `..` semantics across symlinks. On macOS,
/// F_GETPATH supplies the stored spelling for an existing prefix, so accepted
/// case and Unicode aliases converge without a guessed userland fold. Missing
/// suffixes retain their spelling and therefore never merge speculatively.
#[must_use]
pub fn path_lookup_key(path: &Path) -> PathLookupKey {
    let absolute = absolute_path(path);
    let resolved = resolve_from_deepest_existing_ancestor(&absolute);
    PathLookupKey(Arc::from(resolved))
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
}

fn lexically_normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::ParentDir) | None if !path.has_root() => normalized.push(".."),
                Some(Component::Prefix(_)) if !path.has_root() => normalized.push(".."),
                _ => {}
            },
            Component::Normal(value) => normalized.push(value),
        }
    }
    normalized
}

/// Reconstruct a key below the deepest prefix whose filesystem identity can be
/// observed. Resolution errors make the probe walk upward; if even the process
/// directory is unavailable, the absolute lexical path is retained exactly.
fn resolve_from_deepest_existing_ancestor(path: &Path) -> PathBuf {
    let mut probe = path.to_path_buf();
    let mut unresolved = Vec::<OsString>::new();

    loop {
        let candidate = if probe.as_os_str().is_empty() {
            Path::new(".")
        } else {
            probe.as_path()
        };
        if let Ok(existing) = std::fs::canonicalize(candidate) {
            let mut resolved = observed_existing_path(existing);
            for component in unresolved.iter().rev() {
                resolved.push(component);
            }
            return lexically_normalized(&resolved);
        }

        let Some(component) = probe.components().next_back() else {
            return lexically_normalized(path);
        };
        match component {
            Component::Normal(value) => unresolved.push(value.to_owned()),
            Component::ParentDir => unresolved.push(OsString::from("..")),
            Component::CurDir => unresolved.push(OsString::from(".")),
            Component::Prefix(_) | Component::RootDir => {
                return lexically_normalized(path);
            }
        };
        if !probe.pop() {
            return lexically_normalized(path);
        }
    }
}

#[cfg(target_os = "macos")]
fn observed_existing_path(path: PathBuf) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::io::AsRawFd;

    let Ok(file) = std::fs::File::open(&path) else {
        return path;
    };
    let mut buffer = vec![0_u8; libc::PATH_MAX as usize];

    // SAFETY: `file` owns a live descriptor and `buffer` is a writable,
    // PATH_MAX-sized allocation required by F_GETPATH. Failure falls back to
    // the exact canonical spelling; it never guesses a Unicode case fold.
    let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) };
    if result == -1 {
        return path;
    }
    let Some(nul) = buffer.iter().position(|byte| *byte == 0) else {
        return path;
    };
    PathBuf::from(OsString::from_vec(buffer[..nul].to_vec()))
}

#[cfg(not(target_os = "macos"))]
fn observed_existing_path(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;

    fn create_case_variant(directory: &Path) -> (PathBuf, PathBuf, bool) {
        let upper = directory.join("PathIdentityCase");
        let lower = directory.join("pathidentitycase");
        std::fs::write(&upper, b"upper").unwrap();
        let second = OpenOptions::new().write(true).create_new(true).open(&lower);
        match second {
            Ok(mut file) => {
                file.write_all(b"lower").unwrap();
                (upper, lower, true)
            }
            Err(error) => {
                assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
                (upper, lower, false)
            }
        }
    }

    #[test]
    fn case_identity_follows_the_real_test_volume() {
        let directory = tempfile::tempdir().unwrap();
        let (upper, lower, case_sensitive) = create_case_variant(directory.path());

        if case_sensitive {
            assert_ne!(path_lookup_key(&upper), path_lookup_key(&lower));
        } else {
            assert_eq!(path_lookup_key(&upper), path_lookup_key(&lower));
            assert!(lower.exists(), "alternate-case spelling must resolve");
        }
    }

    #[test]
    fn dot_and_dot_dot_are_normalized_before_lookup() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("nested");
        std::fs::create_dir(&nested).unwrap();

        assert_eq!(
            path_lookup_key(&nested.join(".").join("..").join("Missing.java")),
            path_lookup_key(&directory.path().join("Missing.java"))
        );
    }

    #[test]
    fn relative_paths_are_resolved_from_the_process_directory() {
        let cwd = std::env::current_dir().unwrap();
        let directory = tempfile::Builder::new()
            .prefix("hyperion-path-identity-")
            .tempdir_in(&cwd)
            .unwrap();
        let absolute = directory.path().join("Missing.java");
        let relative = absolute.strip_prefix(&cwd).unwrap();

        let relative_key = path_lookup_key(relative);
        assert!(relative_key.is_absolute());
        assert_eq!(relative_key, path_lookup_key(&absolute));
    }

    #[test]
    fn key_is_stable_when_an_existing_leaf_is_deleted() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("Deleted.java");
        std::fs::write(&file, b"class Deleted {}").unwrap();
        let existing = path_lookup_key(&file);

        std::fs::remove_file(&file).unwrap();

        assert_eq!(existing, path_lookup_key(&file));
        assert_eq!(path_lookup_key(&file), path_lookup_key(&file));
    }

    #[cfg(unix)]
    #[test]
    fn existing_symlink_and_real_path_share_a_key() {
        let directory = tempfile::tempdir().unwrap();
        let real = directory.path().join("real");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("Aliased.java"), b"class Aliased {}").unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();

        assert_eq!(
            path_lookup_key(&real.join("Aliased.java")),
            path_lookup_key(&alias.join("Aliased.java"))
        );
        assert!(path_lookup_key(&alias.join("Aliased.java")).starts_with(path_lookup_key(&real)));
    }

    #[cfg(unix)]
    #[test]
    fn parent_traversal_follows_an_existing_symlink_before_normalizing_suffix() {
        let directory = tempfile::tempdir().unwrap();
        let lexical_parent = directory.path().join("lexical");
        let target_parent = directory.path().join("target");
        let target_child = target_parent.join("child");
        std::fs::create_dir(&lexical_parent).unwrap();
        std::fs::create_dir_all(&target_child).unwrap();
        let alias = lexical_parent.join("alias");
        std::os::unix::fs::symlink(&target_child, &alias).unwrap();

        let through_alias = alias.join("..").join("Missing.java");
        let filesystem_destination = target_parent.join("Missing.java");
        assert_eq!(
            path_lookup_key(&through_alias),
            path_lookup_key(&filesystem_destination)
        );
        assert_ne!(
            path_lookup_key(&through_alias),
            path_lookup_key(&lexical_parent.join("Missing.java"))
        );
    }

    #[test]
    fn exact_spelling_is_stable_after_deletion_without_folding_missing_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let (upper, lower, case_sensitive) = create_case_variant(directory.path());
        let upper_key = path_lookup_key(&upper);
        std::fs::remove_file(&upper).unwrap();
        assert_eq!(upper_key, path_lookup_key(&upper));
        assert_ne!(upper_key, path_lookup_key(&lower));
        assert_eq!(case_sensitive, lower.exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_filesystem_spelling_for_existing_unicode_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let spellings = [
            ("Café", "Cafe\u{301}"),
            ("Straße", "STRASSE"),
            ("Σ", "σ"),
            ("Σ", "ς"),
        ];

        for (number, (stored_name, alias_name)) in spellings.into_iter().enumerate() {
            let case = directory.path().join(number.to_string());
            std::fs::create_dir(&case).unwrap();
            let stored = case.join(stored_name);
            let alias = case.join(alias_name);
            std::fs::write(&stored, b"content").unwrap();

            if std::fs::File::open(&alias).is_ok() {
                let stored_key = path_lookup_key(&stored);
                assert_eq!(stored_key, path_lookup_key(&alias));

                std::fs::remove_file(&stored).unwrap();
                assert_eq!(stored_key, path_lookup_key(&stored));
                assert_ne!(
                    stored_key,
                    path_lookup_key(&alias),
                    "a missing suffix must not use a guessed Unicode fold"
                );
            } else {
                std::fs::write(&alias, b"distinct").unwrap();
                assert_ne!(path_lookup_key(&stored), path_lookup_key(&alias));
            }
        }
    }
}
