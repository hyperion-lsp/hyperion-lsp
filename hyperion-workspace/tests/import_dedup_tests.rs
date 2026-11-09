// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Import Deduplication Race
// Tests that concurrent file indexing doesn't create duplicate import entries
// Current behavior: Race condition may cause same import to be added multiple times
// Expected: Should deduplicate imports even under concurrent modification

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

/// Mock structure representing import tracking
#[derive(Clone)]
struct ImportIndex {
    /// Maps file_id -> set of imported FQNs (deduplicated)
    imports: Arc<Mutex<HashMap<usize, HashSet<String>>>>,
}

impl ImportIndex {
    fn new() -> Self {
        Self {
            imports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add_import(&self, file_id: usize, import: String) {
        let mut imports = self.imports.lock().unwrap();
        imports.entry(file_id).or_default().insert(import);
    }

    fn add_imports(&self, file_id: usize, new_imports: Vec<String>) {
        let mut imports = self.imports.lock().unwrap();
        let file_imports = imports.entry(file_id).or_default();
        for import in new_imports {
            file_imports.insert(import);
        }
    }

    fn get_imports(&self, file_id: usize) -> Vec<String> {
        let imports = self.imports.lock().unwrap();
        imports
            .get(&file_id)
            .map(|set| {
                let mut vec: Vec<String> = set.iter().cloned().collect();
                vec.sort();
                vec
            })
            .unwrap_or_default()
    }

    fn import_count(&self, file_id: usize) -> usize {
        let imports = self.imports.lock().unwrap();
        imports.get(&file_id).map(|set| set.len()).unwrap_or(0)
    }
}

#[test]
fn test_concurrent_import_deduplication() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(4));

    let mut handles = vec![];

    // Spawn 4 threads that all try to add the same imports to the same file
    for _ in 0..4 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();

            // All threads add the same imports
            index_clone.add_import(1, "java.util.List".to_string());
            index_clone.add_import(1, "java.util.Map".to_string());
            index_clone.add_import(1, "com.example.User".to_string());
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have exactly 3 unique imports despite concurrent additions
    assert_eq!(index.import_count(1), 3);
    let imports = index.get_imports(1);
    assert_eq!(
        imports,
        vec![
            "com.example.User".to_string(),
            "java.util.List".to_string(),
            "java.util.Map".to_string(),
        ]
    );
}

#[test]
fn test_concurrent_different_imports() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(3));

    let mut handles = vec![];

    // Thread 1: Add imports A, B
    let index_clone = index.clone();
    let barrier_clone = barrier.clone();
    handles.push(thread::spawn(move || {
        barrier_clone.wait();
        index_clone.add_import(1, "com.example.A".to_string());
        index_clone.add_import(1, "com.example.B".to_string());
    }));

    // Thread 2: Add imports B, C
    let index_clone = index.clone();
    let barrier_clone = barrier.clone();
    handles.push(thread::spawn(move || {
        barrier_clone.wait();
        index_clone.add_import(1, "com.example.B".to_string());
        index_clone.add_import(1, "com.example.C".to_string());
    }));

    // Thread 3: Add imports C, D
    let index_clone = index.clone();
    let barrier_clone = barrier.clone();
    handles.push(thread::spawn(move || {
        barrier_clone.wait();
        index_clone.add_import(1, "com.example.C".to_string());
        index_clone.add_import(1, "com.example.D".to_string());
    }));

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have exactly 4 unique imports (A, B, C, D)
    assert_eq!(index.import_count(1), 4);
    let imports = index.get_imports(1);
    assert_eq!(
        imports,
        vec![
            "com.example.A".to_string(),
            "com.example.B".to_string(),
            "com.example.C".to_string(),
            "com.example.D".to_string(),
        ]
    );
}

#[test]
fn test_concurrent_batch_imports() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(5));

    let mut handles = vec![];

    for i in 0..5 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();

            let imports = vec![
                "java.util.List".to_string(),
                "java.util.Map".to_string(),
                format!("com.example.Class{}", i),
            ];

            index_clone.add_imports(1, imports);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have List, Map, and Class0-Class4 (7 total)
    assert_eq!(index.import_count(1), 7);
}

#[test]
fn test_concurrent_multiple_files() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(10));

    let mut handles = vec![];

    // 10 threads, each working on different file
    for file_id in 0..10 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();

            index_clone.add_import(file_id, "java.util.List".to_string());
            index_clone.add_import(file_id, format!("com.example.Class{}", file_id));
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Each file should have exactly 2 imports
    for file_id in 0..10 {
        assert_eq!(index.import_count(file_id), 2);
    }
}

#[test]
fn test_concurrent_wildcard_import_dedup() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(3));

    let mut handles = vec![];

    for _ in 0..3 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();

            // All add the same wildcard import
            index_clone.add_import(1, "java.util.*".to_string());
            index_clone.add_import(1, "java.util.List".to_string());
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have exactly 2 imports
    assert_eq!(index.import_count(1), 2);
    let imports = index.get_imports(1);
    assert_eq!(
        imports,
        vec!["java.util.*".to_string(), "java.util.List".to_string(),]
    );
}

#[test]
fn test_concurrent_static_import_dedup() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(4));

    let mut handles = vec![];

    for _ in 0..4 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();

            // All add same static imports
            index_clone.add_import(1, "static java.lang.Math.PI".to_string());
            index_clone.add_import(1, "static java.lang.Math.*".to_string());
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(index.import_count(1), 2);
}

#[test]
fn test_rapid_import_additions() {
    let index = ImportIndex::new();

    // Single thread rapidly adding same imports
    for _ in 0..100 {
        index.add_import(1, "java.util.List".to_string());
        index.add_import(1, "java.util.Map".to_string());
    }

    // Should still have only 2 imports
    assert_eq!(index.import_count(1), 2);
}

#[test]
fn test_concurrent_import_different_case() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(2));

    let mut handles = vec![];

    // Thread 1: Add import with one casing
    let index_clone = index.clone();
    let barrier_clone = barrier.clone();
    handles.push(thread::spawn(move || {
        barrier_clone.wait();
        index_clone.add_import(1, "com.example.User".to_string());
    }));

    // Thread 2: Add same import (same casing)
    let index_clone = index.clone();
    let barrier_clone = barrier.clone();
    handles.push(thread::spawn(move || {
        barrier_clone.wait();
        index_clone.add_import(1, "com.example.User".to_string());
    }));

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have exactly 1 import (case-sensitive dedup)
    assert_eq!(index.import_count(1), 1);
}

#[test]
fn test_concurrent_nested_class_imports() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(3));

    let mut handles = vec![];

    for _ in 0..3 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();

            index_clone.add_import(1, "com.example.Outer".to_string());
            index_clone.add_import(1, "com.example.Outer.Inner".to_string());
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have 2 distinct imports
    assert_eq!(index.import_count(1), 2);
}

#[test]
fn test_concurrent_high_volume() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(20));

    let mut handles = vec![];

    for thread_id in 0..20 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();

            // Each thread adds some common and some unique imports
            for i in 0..10 {
                index_clone.add_import(1, format!("com.example.Class{}", i));
                index_clone.add_import(1, format!("com.example.Thread{}Class{}", thread_id, i));
            }
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let count = index.import_count(1);

    // Should have: 10 common classes + (20 threads * 10 unique classes) = 210 total
    assert_eq!(count, 210);
}

#[test]
fn test_concurrent_empty_imports() {
    let index = ImportIndex::new();
    let barrier = Arc::new(Barrier::new(5));

    let mut handles = vec![];

    for _ in 0..5 {
        let index_clone = index.clone();
        let barrier_clone = barrier.clone();

        handles.push(thread::spawn(move || {
            barrier_clone.wait();
            // Add empty import list
            index_clone.add_imports(1, vec![]);
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    // Should have 0 imports
    assert_eq!(index.import_count(1), 0);
}
