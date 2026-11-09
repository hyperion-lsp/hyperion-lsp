// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Phase 3.1 race condition synchronization detection
//!
//! Tests the pragmatic synchronization detection approach from FP_REDUCTION_ROUND_4.md

use hyperion_security::finding::VulnType;
use hyperion_security::{ScanConfig, SecurityScanner};
use std::path::Path;

/// Helper to scan code and return findings
fn scan_code(code: &str) -> Vec<hyperion_security::finding::Finding> {
    let config = ScanConfig::default();
    let scanner = SecurityScanner::new(config);
    scanner.scan_source(code, Path::new("Test.java"))
}

/// Helper to count race condition findings
fn count_race_findings(code: &str) -> usize {
    scan_code(code)
        .iter()
        .filter(|f| f.vuln_type == VulnType::RaceCondition)
        .count()
}

// Test 1: Synchronized method modifier
#[test]
fn test_synchronized_method_not_flagged() {
    let code = r#"
        public class Counter {
            private List<Integer> values = new ArrayList<>();

            public synchronized void add(int value) {
                if (!values.contains(value)) {  // Check-then-act
                    values.add(value);           // Would normally flag
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(
        count, 0,
        "Synchronized method should not be flagged for race conditions"
    );
}

// Test 2: Synchronized block
#[test]
fn test_synchronized_block_not_flagged() {
    let code = r#"
        public class Cache {
            private Map<String, String> data = new HashMap<>();

            public void put(String key, String value) {
                synchronized (data) {
                    if (!data.containsKey(key)) {
                        data.put(key, value);
                    }
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(
        count, 0,
        "Code inside synchronized block should not be flagged"
    );
}

// Test 4: ConcurrentHashMap usage
#[test]
fn test_concurrent_hashmap_not_flagged() {
    let code = r#"
        public class ThreadSafeCache {
            private final ConcurrentHashMap<String, String> cache = new ConcurrentHashMap<>();

            public void putIfAbsent(String key, String value) {
                if (!cache.containsKey(key)) {
                    cache.put(key, value);
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "ConcurrentHashMap usage should not be flagged");
}

// Test 5: CopyOnWriteArrayList usage
#[test]
fn test_copy_on_write_list_not_flagged() {
    let code = r#"
        public class EventListeners {
            private final CopyOnWriteArrayList<Listener> listeners = new CopyOnWriteArrayList<>();

            public void addIfNew(Listener listener) {
                if (!listeners.contains(listener)) {
                    listeners.add(listener);
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "CopyOnWriteArrayList usage should not be flagged");
}

// Test 6: AtomicInteger usage
#[test]
fn test_atomic_integer_not_flagged() {
    let code = r#"
        public class Counter {
            private final AtomicInteger count = new AtomicInteger(0);

            public void incrementIfBelowLimit(int limit) {
                if (count.get() < limit) {
                    count.incrementAndGet();
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "AtomicInteger usage should not be flagged");
}

// Test 7: ReentrantLock usage
#[test]
fn test_reentrant_lock_not_flagged() {
    let code = r#"
        import java.util.concurrent.locks.ReentrantLock;

        public class SafeCounter {
            private final ReentrantLock lock = new ReentrantLock();
            private List<String> items = new ArrayList<>();

            public void addIfNotExists(String item) {
                lock.lock();
                try {
                    if (!items.contains(item)) {
                        items.add(item);
                    }
                } finally {
                    lock.unlock();
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "ReentrantLock usage should not be flagged");
}

// Test 8: ReadWriteLock usage
#[test]
fn test_read_write_lock_not_flagged() {
    let code = r#"
        import java.util.concurrent.locks.ReadWriteLock;

        public class Cache {
            private final ReadWriteLock rwLock = new ReentrantReadWriteLock();
            private Map<String, String> data = new HashMap<>();

            public void putIfAbsent(String key, String value) {
                rwLock.writeLock().lock();
                try {
                    if (!data.containsKey(key)) {
                        data.put(key, value);
                    }
                } finally {
                    rwLock.writeLock().unlock();
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "ReadWriteLock usage should not be flagged");
}

// Test 9: Unsynchronized code SHOULD still be flagged
// Note: Using a clearly shared collection (field accessed from multiple methods)
#[test]
fn test_unsynchronized_code_still_flagged() {
    let code = r#"
        public class UnsafeCache {
            private Map<String, String> sharedCache = new HashMap<>();

            public void put(String key, String value) {
                if (!sharedCache.containsKey(key)) {  // RACE CONDITION
                    sharedCache.put(key, value);
                }
            }

            public void remove(String key) {
                sharedCache.remove(key);
            }
        }
    "#;

    let count = count_race_findings(code);
    assert!(count > 0, "Unsynchronized check-then-act must be flagged");
}

// Test 10: Non-synchronized public method SHOULD be flagged
// Note: Using a clearly shared collection field
#[test]
fn test_public_method_without_sync_flagged() {
    let code = r#"
        public class Counter {
            private List<Integer> sharedValues = new ArrayList<>();

            public void add(int value) {  // NOT synchronized
                if (!sharedValues.contains(value)) {
                    sharedValues.add(value);
                }
            }

            public void remove(int value) {
                sharedValues.remove(Integer.valueOf(value));
            }
        }
    "#;

    let count = count_race_findings(code);
    assert!(
        count > 0,
        "Public unsynchronized check-then-act must be flagged"
    );
}

// Test 11: Balance check-then-withdraw with synchronized
#[test]
fn test_balance_check_synchronized_not_flagged() {
    let code = r#"
        public class Account {
            private double balance = 0.0;

            public synchronized void withdraw(double amount) {
                if (balance >= amount) {
                    balance -= amount;
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "Synchronized balance check should not be flagged");
}

// Test 12: Balance check-then-withdraw with @Transactional
#[test]
fn test_balance_check_transactional_not_flagged() {
    let code = r#"
        public class Account {
            private double balance = 0.0;

            @Transactional
            public void withdraw(double amount) {
                if (balance >= amount) {
                    balance -= amount;
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "@Transactional should protect balance operations");
}

// Test 13: File existence check with synchronized
#[test]
fn test_file_check_synchronized_not_flagged() {
    let code = r#"
        public class FileManager {
            public synchronized void processFile(File file) {
                if (file.exists()) {
                    FileInputStream fis = new FileInputStream(file);
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(
        count, 0,
        "Synchronized file operations should not be flagged"
    );
}

// Test 14: BlockingQueue usage
#[test]
fn test_blocking_queue_not_flagged() {
    let code = r#"
        import java.util.concurrent.BlockingQueue;

        public class TaskQueue {
            private final BlockingQueue<Task> queue = new LinkedBlockingQueue<>();

            public void addIfNotFull(Task task) {
                if (!queue.isEmpty()) {
                    queue.poll();
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(
        count, 0,
        "BlockingQueue is thread-safe, should not be flagged"
    );
}

// Test 15: Nested synchronization
#[test]
fn test_nested_synchronized_blocks() {
    let code = r#"
        public class NestedSync {
            private final Object lock1 = new Object();
            private final Object lock2 = new Object();
            private Map<String, String> data = new HashMap<>();

            public void update(String key, String value) {
                synchronized (lock1) {
                    synchronized (lock2) {
                        if (!data.containsKey(key)) {
                            data.put(key, value);
                        }
                    }
                }
            }
        }
    "#;

    let count = count_race_findings(code);
    assert_eq!(count, 0, "Nested synchronized blocks should not be flagged");
}

// A field that the method reassigns is still shared. "items = fresh;" is not a local
// declaration.
#[test]
fn test_field_reassigned_in_method_is_still_flagged() {
    let code = r#"
        public class Queue {
            private List<String> items = new ArrayList<>();
            public void refresh(List<String> fresh, String s) {
                items = fresh;
                if (items.isEmpty()) {
                    items.add(s);
                }
            }
            public void run() {
                refresh(new ArrayList<>(), "x");
            }
        }
    "#;

    assert_eq!(
        count_race_findings(code),
        1,
        "A reassigned field is shared state and must be flagged"
    );
}

// The check and the file operation are in one synchronized block inside the method body.
#[test]
fn test_file_check_and_use_inside_a_synchronized_block_not_flagged() {
    let code = r#"
        public class Loader {
            public void load(String path) {
                synchronized (this) {
                    File f = new File(path);
                    if (f.exists()) {
                        new FileInputStream(f);
                    }
                }
            }
            public void run() {
                load("a");
            }
        }
    "#;

    assert_eq!(
        count_race_findings(code),
        0,
        "A check and its use under one lock are not a race"
    );
}

// A file check outside the synchronized block still races with the use.
#[test]
fn test_file_check_outside_the_synchronized_block_is_flagged() {
    let code = r#"
        public class Loader {
            public void load(String path) {
                File f = new File(path);
                if (f.exists()) {
                    synchronized (this) {
                        new FileInputStream(f);
                    }
                }
            }
            public void run() {
                load("a");
            }
        }
    "#;

    assert_eq!(count_race_findings(code), 1);
}

// The word "Request" in the condition names no part of the shared collection.
#[test]
fn test_request_word_in_condition_does_not_hide_a_shared_collection() {
    let code = r#"
        public class Queue {
            private List<String> items = new ArrayList<>();
            public void add(String s) {
                if (items.isEmpty() && HttpRequest.current() == null) {
                    items.add(s);
                }
            }
            public void run() {
                add("x");
            }
        }
    "#;

    assert_eq!(
        count_race_findings(code),
        1,
        "A shared collection is flagged whatever else the condition mentions"
    );
}
