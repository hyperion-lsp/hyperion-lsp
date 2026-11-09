// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::content::SourceFile;
use lru::LruCache;
use std::borrow::Borrow;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;

const MAX_ENTRIES: usize = 512;
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// Byte-bounded cache for immutable archive source text.
///
/// A count-only bound is unsafe here because one accepted source entry may be
/// several megabytes. `Arc<str>` also lets a navigation response share the
/// cached allocation instead of cloning the complete source file.
pub(super) struct SourceCache<K = String> {
    entries: LruCache<K, Arc<SourceFile>>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl<K: Eq + Hash> SourceCache<K> {
    pub(super) fn new() -> Self {
        Self::with_limits(MAX_ENTRIES, MAX_BYTES)
    }

    fn with_limits(max_entries: usize, max_bytes: usize) -> Self {
        let capacity = NonZeroUsize::new(max_entries).expect("source cache must accept entries");
        Self {
            entries: LruCache::new(capacity),
            bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    pub(super) fn get<Q>(&mut self, key: &Q) -> Option<Arc<SourceFile>>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.reconcile_retained_bytes();
        self.entries.get(key).cloned()
    }

    pub(super) fn insert(&mut self, key: K, source: Arc<SourceFile>) {
        self.reconcile_retained_bytes();
        if let Some(replaced) = self.entries.pop(&key) {
            self.bytes = self.bytes.saturating_sub(replaced.retained_bytes());
        }

        let source_bytes = source.retained_bytes();
        if source_bytes > self.max_bytes {
            return;
        }

        while self.entries.len() >= self.max_entries
            || self.bytes.saturating_add(source_bytes) > self.max_bytes
        {
            let Some((_key, evicted)) = self.entries.pop_lru() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.retained_bytes());
        }

        self.bytes += source_bytes;
        let replaced = self.entries.put(key, source);
        debug_assert!(replaced.is_none(), "entry was removed before insertion");
    }

    fn reconcile_retained_bytes(&mut self) {
        self.bytes = self
            .entries
            .iter()
            .map(|(_, source)| source.retained_bytes())
            .sum();
        while self.bytes > self.max_bytes {
            let Some((_key, evicted)) = self.entries.pop_lru() else {
                self.bytes = 0;
                break;
            };
            self.bytes = self.bytes.saturating_sub(evicted.retained_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceCache, SourceFile};
    use std::sync::Arc;

    fn source(content: &'static str) -> Arc<SourceFile> {
        Arc::new(SourceFile::new(Arc::from(content)))
    }

    #[test]
    fn eviction_is_bounded_by_retained_bytes() {
        let mut cache = SourceCache::with_limits(10, 5);
        cache.insert("first".to_string(), source("abc"));
        cache.insert("second".to_string(), source("def"));

        assert!(cache.get("first").is_none());
        assert_eq!(
            cache.get("second").as_deref().map(SourceFile::content),
            Some("def")
        );
        assert_eq!(cache.bytes, 3);
    }

    #[test]
    fn oversized_entries_are_returned_to_callers_but_not_retained() {
        let mut cache = SourceCache::with_limits(10, 2);
        cache.insert("large".to_string(), source("abc"));

        assert!(cache.get("large").is_none());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn entry_count_eviction_keeps_byte_accounting_exact() {
        let mut cache = SourceCache::with_limits(1, 100);
        cache.insert("first".to_string(), source("one"));
        cache.insert("second".to_string(), source("two"));

        assert!(cache.get("first").is_none());
        assert_eq!(
            cache.get("second").as_deref().map(SourceFile::content),
            Some("two")
        );
        assert_eq!(cache.bytes, 3);
    }

    #[test]
    fn lazily_parsed_symbol_memory_participates_in_eviction() {
        let text = "class Cached { void method() {} }";
        let source = Arc::new(SourceFile::new(Arc::from(text)));
        let mut cache = SourceCache::with_limits(10, text.len());
        cache.insert("Cached".to_owned(), Arc::clone(&source));
        assert!(source.java_symbols().is_ok());

        assert!(
            cache.get("Cached").is_none(),
            "derived symbol locations must not grow outside the byte budget"
        );
        assert_eq!(cache.bytes, 0);
    }
}
