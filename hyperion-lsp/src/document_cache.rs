// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Document Cache with LRU Eviction
//!
//! This module provides an LRU cache for document contents and parsed trees.
//! It helps limit memory usage on large projects by evicting least-recently-used documents.

use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use tower_lsp::lsp_types::Url;

/// Cached document information
#[derive(Clone)]
pub struct CachedDocument {
    /// The document URI
    pub uri: Url,
    /// The document text content
    pub text: String,
    /// Document version (for LSP)
    pub version: Option<i32>,
}

/// LRU cache for document contents
///
/// Limits memory usage by evicting least-recently-used documents when the cache is full.
/// Default capacity is 100 documents, which should handle most projects well while
/// limiting memory usage to a reasonable level.
pub struct DocumentCache {
    cache: Mutex<LruCache<Url, CachedDocument>>,
}

impl DocumentCache {
    /// Default cache capacity (number of documents)
    pub const DEFAULT_CAPACITY: usize = 100;

    /// Create a new document cache with default capacity
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// Create a new document cache with specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        // 100 is non-zero, checked at compile time via const unwrap
        const DEFAULT_FALLBACK: NonZeroUsize = NonZeroUsize::new(100).unwrap();
        let cap = NonZeroUsize::new(capacity).unwrap_or(DEFAULT_FALLBACK);
        Self {
            cache: Mutex::new(LruCache::new(cap)),
        }
    }

    /// Get a document from the cache
    ///
    /// Returns None if the document is not in the cache.
    /// Marks the document as recently used.
    pub fn get(&self, uri: &Url) -> Option<CachedDocument> {
        let mut cache = self.cache.lock();
        cache.get(uri).cloned()
    }

    /// Put a document into the cache
    ///
    /// If the cache is full, evicts the least-recently-used document.
    pub fn put(&self, uri: Url, text: String, version: Option<i32>) {
        let mut cache = self.cache.lock();
        let doc = CachedDocument {
            uri: uri.clone(),
            text,
            version,
        };
        cache.put(uri, doc);
    }

    /// Update an existing document in the cache
    ///
    /// If the document doesn't exist, it will be added.
    /// Marks the document as recently used.
    pub fn update(&self, uri: &Url, text: String, version: Option<i32>) {
        self.put(uri.clone(), text, version);
    }

    /// Remove a document from the cache
    pub fn remove(&self, uri: &Url) -> Option<CachedDocument> {
        let mut cache = self.cache.lock();
        cache.pop(uri)
    }

    /// Clear all documents from the cache
    pub fn clear(&self) {
        let mut cache = self.cache.lock();
        cache.clear();
    }

    /// Get the current number of documents in the cache
    pub fn len(&self) -> usize {
        let cache = self.cache.lock();
        cache.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the cache capacity
    pub fn capacity(&self) -> usize {
        let cache = self.cache.lock();
        cache.cap().get()
    }
}

impl Default for DocumentCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_uri(path: &str) -> Url {
        Url::parse(&format!("file://{}", path)).unwrap()
    }

    #[test]
    fn test_cache_put_and_get() {
        let cache = DocumentCache::new();
        let uri = make_uri("/test.java");
        let text = "class Test {}".to_string();

        cache.put(uri.clone(), text.clone(), Some(1));

        let doc = cache.get(&uri).unwrap();
        assert_eq!(doc.text, text);
        assert_eq!(doc.version, Some(1));
    }

    #[test]
    fn test_cache_update() {
        let cache = DocumentCache::new();
        let uri = make_uri("/test.java");

        cache.put(uri.clone(), "class Test {}".to_string(), Some(1));
        cache.update(&uri, "class Test { int x; }".to_string(), Some(2));

        let doc = cache.get(&uri).unwrap();
        assert_eq!(doc.text, "class Test { int x; }");
        assert_eq!(doc.version, Some(2));
    }

    #[test]
    fn test_cache_remove() {
        let cache = DocumentCache::new();
        let uri = make_uri("/test.java");

        cache.put(uri.clone(), "class Test {}".to_string(), Some(1));
        assert!(cache.get(&uri).is_some());

        cache.remove(&uri);
        assert!(cache.get(&uri).is_none());
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache = DocumentCache::with_capacity(2);

        let uri1 = make_uri("/test1.java");
        let uri2 = make_uri("/test2.java");
        let uri3 = make_uri("/test3.java");

        cache.put(uri1.clone(), "class Test1 {}".to_string(), Some(1));
        cache.put(uri2.clone(), "class Test2 {}".to_string(), Some(1));

        // Both should be in cache
        assert!(cache.get(&uri1).is_some());
        assert!(cache.get(&uri2).is_some());

        // Adding third should evict uri1 (least recently used)
        cache.put(uri3.clone(), "class Test3 {}".to_string(), Some(1));

        assert!(cache.get(&uri1).is_none()); // Evicted
        assert!(cache.get(&uri2).is_some());
        assert!(cache.get(&uri3).is_some());
    }

    #[test]
    fn test_cache_clear() {
        let cache = DocumentCache::new();

        cache.put(
            make_uri("/test1.java"),
            "class Test1 {}".to_string(),
            Some(1),
        );
        cache.put(
            make_uri("/test2.java"),
            "class Test2 {}".to_string(),
            Some(1),
        );

        assert_eq!(cache.len(), 2);

        cache.clear();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_capacity() {
        let cache = DocumentCache::with_capacity(50);
        assert_eq!(cache.capacity(), 50);
    }
}
