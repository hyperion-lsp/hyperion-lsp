// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Mark & Sweep Garbage Collector
//!
//! Simple but effective GC implementation for the JVM bytecode interpreter.
//! Uses classic mark-sweep algorithm with BFS traversal for the mark phase.

use super::heap::{Heap, HeapEntry};
use super::value::{HeapId, ObjectRef, Value};
use std::collections::HashSet;

/// Statistics returned after a GC run
#[derive(Debug, Clone)]
pub struct GCStats {
    /// Number of objects freed during this collection
    pub objects_freed: usize,
    /// Bytes freed during this collection
    pub bytes_freed: usize,
    /// Objects remaining after collection
    pub objects_remaining: usize,
    /// Duration of collection in milliseconds
    pub duration_ms: u128,
}

/// Roots for garbage collection
///
/// Contains all values that should be treated as GC roots:
/// - Stack values from all threads
/// - Static fields
/// - Direct heap references from native code
#[derive(Debug, Default)]
pub struct GCRoots {
    /// Values that may contain references
    pub values: Vec<Value>,
    /// Direct heap IDs to treat as roots
    pub heap_ids: Vec<HeapId>,
}

impl GCRoots {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a value that may contain a reference
    pub fn add_value(&mut self, value: &Value) {
        self.values.push(value.clone());
    }

    /// Add a direct heap ID as a root
    pub fn add_heap_id(&mut self, id: HeapId) {
        self.heap_ids.push(id);
    }
}

/// Mark & Sweep garbage collector
pub struct GarbageCollector;

impl GarbageCollector {
    /// Mark phase - traverse from roots and mark all reachable objects
    ///
    /// Uses BFS traversal to find all objects reachable from the roots.
    /// Returns set of all marked (live) heap IDs.
    pub fn mark(heap: &Heap, roots: &GCRoots) -> HashSet<HeapId> {
        let mut marked = HashSet::new();
        // Emergency VM exceptions are heap-intrinsic roots. Seeding them here,
        // rather than at each collector call site, makes it impossible for an
        // embedder to collect the objects needed to report OOME/SOE.
        let mut work_list: Vec<HeapId> = heap.intrinsic_root_ids().collect();

        // Add root values to work list
        for value in &roots.values {
            if let Some(id) = Self::extract_heap_id(value) {
                work_list.push(id);
            }
        }

        // Add direct heap IDs
        work_list.extend(&roots.heap_ids);

        // BFS traversal - process until work list is empty
        while let Some(id) = work_list.pop() {
            if marked.contains(&id) {
                continue;
            }
            marked.insert(id);

            // Get object/array and add referenced IDs to work list
            if let Some(entry) = heap.get_entry(id) {
                Self::collect_references(entry, &mut work_list);
            }
            for value in heap.dynamic_property_values(id) {
                if let Some(reference) = Self::extract_heap_id(value) {
                    work_list.push(reference);
                }
            }
        }

        marked
    }

    /// Extract heap ID from a Value if it contains one
    fn extract_heap_id(value: &Value) -> Option<HeapId> {
        match value {
            Value::Reference(ObjectRef::Object(id)) => Some(*id),
            Value::Reference(ObjectRef::Array(id)) => Some(*id),
            _ => None,
        }
    }

    /// Collect all heap references from a heap entry
    fn collect_references(entry: &HeapEntry, work_list: &mut Vec<HeapId>) {
        match entry {
            HeapEntry::Object(obj) => {
                // Check all fields for references
                for value in obj.fields.iter().flatten() {
                    if let Some(id) = Self::extract_heap_id(value) {
                        work_list.push(id);
                    }
                }
                if let Some(throwable) = &obj.throwable {
                    for reference in [throwable.detail_message, throwable.cause] {
                        if let Some(id) = reference.heap_id() {
                            work_list.push(id);
                        }
                    }
                }
            }
            HeapEntry::Array(arr) => {
                // Only reference arrays can contain heap references
                if let super::heap::ArrayData::Reference(refs) = &arr.data {
                    for obj_ref in refs {
                        match obj_ref {
                            ObjectRef::Object(id) | ObjectRef::Array(id) => {
                                work_list.push(*id);
                            }
                            _ => {}
                        }
                    }
                }
            }
            HeapEntry::Collection(coll) => {
                // Collections contain values that may reference heap objects
                use super::heap::CollectionData;
                match &coll.data {
                    CollectionData::ArrayList(list) | CollectionData::LinkedList(list) => {
                        for value in list {
                            if let Some(id) = Self::extract_heap_id(value) {
                                work_list.push(id);
                            }
                        }
                    }
                    CollectionData::HashMap(map) => {
                        for (k, v) in map {
                            if let Some(id) = Self::extract_heap_id(k) {
                                work_list.push(id);
                            }
                            if let Some(id) = Self::extract_heap_id(v) {
                                work_list.push(id);
                            }
                        }
                    }
                    CollectionData::HashSet(set) => {
                        for value in set {
                            if let Some(id) = Self::extract_heap_id(value) {
                                work_list.push(id);
                            }
                        }
                    }
                    CollectionData::Iterator { items, .. } => {
                        for value in items {
                            if let Some(id) = Self::extract_heap_id(value) {
                                work_list.push(id);
                            }
                        }
                    }
                    CollectionData::IntRangeIterator { .. } => {}
                }
            }
            HeapEntry::Class(_) => {
                // Class objects don't contain heap references (just a class name string)
            }
            HeapEntry::Thread(thread) => {
                if let Some(id) = thread.target.heap_id() {
                    work_list.push(id);
                }
            }
        }
    }

    /// Sweep phase - remove all unmarked objects
    ///
    /// Returns (objects_freed, bytes_freed)
    pub fn sweep(heap: &mut Heap, marked: &HashSet<HeapId>) -> (usize, usize) {
        let mut freed_count = 0;
        let mut freed_bytes = 0;

        // Collect IDs to remove (can't remove during iteration)
        let ids_to_remove: Vec<HeapId> = heap.all_ids().filter(|id| !marked.contains(id)).collect();

        for id in ids_to_remove {
            if let Some((size, _entry)) = heap.remove_unmarked(id) {
                freed_count += 1;
                freed_bytes += size;
            }
        }

        (freed_count, freed_bytes)
    }

    /// Full collection cycle - mark then sweep
    pub fn collect(heap: &mut Heap, roots: &GCRoots) -> GCStats {
        let start = std::time::Instant::now();

        let marked = Self::mark(heap, roots);
        let (freed_count, freed_bytes) = Self::sweep(heap, &marked);

        GCStats {
            objects_freed: freed_count,
            bytes_freed: freed_bytes,
            // Emergency reserve objects are VM-owned and unmetered; report
            // guest heap occupancy just as `memory_used` does.
            objects_remaining: heap.guest_object_count(),
            duration_ms: start.elapsed().as_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::heap::{ArrayElementType, ClassRef};

    fn test_heap() -> Heap {
        Heap::new(1024 * 1024) // 1MB
    }

    fn test_class(name: &str, field_count: usize) -> ClassRef {
        ClassRef {
            name: name.to_string(),
            instance_field_count: field_count,
            static_field_count: 0,
        }
    }

    #[test]
    fn test_mark_single_object() {
        let mut heap = test_heap();
        let id = heap.allocate_object(test_class("Test", 0)).unwrap();

        let mut roots = GCRoots::new();
        roots.add_heap_id(id);

        let marked = GarbageCollector::mark(&heap, &roots);
        assert!(marked.contains(&id));
        assert_eq!(marked.len(), 3); // guest root + OOME/SOE reserve
    }

    #[test]
    fn rooted_thread_keeps_its_runnable_target_alive() {
        let mut heap = test_heap();
        let target = heap.allocate_object(test_class("Runnable", 0)).unwrap();
        let thread = heap
            .allocate_thread_with_runnable(ObjectRef::Object(target))
            .unwrap();
        let mut roots = GCRoots::new();
        roots.add_heap_id(thread);

        let stats = GarbageCollector::collect(&mut heap, &roots);

        assert_eq!(stats.objects_freed, 0);
        assert!(heap.get_entry(target).is_some());
    }

    #[test]
    fn test_mark_object_graph() {
        let mut heap = test_heap();

        // Create object A with field pointing to B
        let id_b = heap.allocate_object(test_class("B", 0)).unwrap();
        let id_a = heap.allocate_object(test_class("A", 1)).unwrap();
        heap.set_field(id_a, 0, Value::Reference(ObjectRef::Object(id_b)))
            .unwrap();

        // Root only references A, but B should be marked through A
        let mut roots = GCRoots::new();
        roots.add_heap_id(id_a);

        let marked = GarbageCollector::mark(&heap, &roots);
        assert!(marked.contains(&id_a));
        assert!(marked.contains(&id_b));
        assert_eq!(marked.len(), 4); // guest graph + OOME/SOE reserve
    }

    #[test]
    fn test_mark_circular_references() {
        let mut heap = test_heap();

        // Create circular: A -> B -> C -> A
        let id_a = heap.allocate_object(test_class("A", 1)).unwrap();
        let id_b = heap.allocate_object(test_class("B", 1)).unwrap();
        let id_c = heap.allocate_object(test_class("C", 1)).unwrap();

        heap.set_field(id_a, 0, Value::Reference(ObjectRef::Object(id_b)))
            .unwrap();
        heap.set_field(id_b, 0, Value::Reference(ObjectRef::Object(id_c)))
            .unwrap();
        heap.set_field(id_c, 0, Value::Reference(ObjectRef::Object(id_a)))
            .unwrap();

        let mut roots = GCRoots::new();
        roots.add_heap_id(id_a);

        let marked = GarbageCollector::mark(&heap, &roots);
        assert!(marked.contains(&id_a));
        assert!(marked.contains(&id_b));
        assert!(marked.contains(&id_c));
        assert_eq!(marked.len(), 5); // guest graph + OOME/SOE reserve
    }

    #[test]
    fn test_sweep_removes_unmarked() {
        let mut heap = test_heap();

        let id_live = heap.allocate_object(test_class("Live", 0)).unwrap();
        let id_dead = heap.allocate_object(test_class("Dead", 0)).unwrap();

        // Only mark the live object
        let mut marked = HashSet::new();
        marked.insert(id_live);

        let (freed_count, _freed_bytes) = GarbageCollector::sweep(&mut heap, &marked);

        assert_eq!(freed_count, 1);
        assert!(heap.get_entry(id_live).is_some());
        assert!(heap.get_entry(id_dead).is_none());
    }

    #[test]
    fn test_full_collection_cycle() {
        let mut heap = test_heap();

        // Create some live objects
        let id_live1 = heap.allocate_object(test_class("Live1", 1)).unwrap();
        let id_live2 = heap.allocate_object(test_class("Live2", 0)).unwrap();
        heap.set_field(id_live1, 0, Value::Reference(ObjectRef::Object(id_live2)))
            .unwrap();

        // Create some dead objects (unreachable)
        let _id_dead1 = heap.allocate_object(test_class("Dead1", 0)).unwrap();
        let _id_dead2 = heap.allocate_object(test_class("Dead2", 0)).unwrap();

        let mut roots = GCRoots::new();
        roots.add_heap_id(id_live1);

        let stats = GarbageCollector::collect(&mut heap, &roots);

        assert_eq!(stats.objects_freed, 2);
        assert_eq!(stats.objects_remaining, 2);
        assert!(heap.get_entry(id_live1).is_some());
        assert!(heap.get_entry(id_live2).is_some());
    }

    #[test]
    fn test_gc_with_reference_array() {
        let mut heap = test_heap();

        // Create array holding references
        let id_obj = heap.allocate_object(test_class("Obj", 0)).unwrap();
        let id_arr = heap
            .allocate_array(ArrayElementType::Reference("Obj".to_string()), 2)
            .unwrap();
        heap.array_store(id_arr, 0, Value::Reference(ObjectRef::Object(id_obj)))
            .unwrap();

        // Root only the array
        let mut roots = GCRoots::new();
        roots.add_heap_id(id_arr);

        let marked = GarbageCollector::mark(&heap, &roots);
        assert!(marked.contains(&id_arr));
        assert!(marked.contains(&id_obj));
    }

    #[test]
    fn test_gc_roots_from_values() {
        let mut heap = test_heap();
        let id = heap.allocate_object(test_class("Test", 0)).unwrap();

        let mut roots = GCRoots::new();
        roots.add_value(&Value::Reference(ObjectRef::Object(id)));

        let marked = GarbageCollector::mark(&heap, &roots);
        assert!(marked.contains(&id));
    }

    #[test]
    fn test_gc_empty_heap() {
        let mut heap = test_heap();
        let roots = GCRoots::new();

        let stats = GarbageCollector::collect(&mut heap, &roots);

        assert_eq!(stats.objects_freed, 0);
        assert_eq!(stats.objects_remaining, 0);
    }

    #[test]
    fn test_gc_all_dead() {
        let mut heap = test_heap();

        // Allocate objects but don't root any
        heap.allocate_object(test_class("Dead1", 0)).unwrap();
        heap.allocate_object(test_class("Dead2", 0)).unwrap();
        heap.allocate_object(test_class("Dead3", 0)).unwrap();

        let roots = GCRoots::new();
        let stats = GarbageCollector::collect(&mut heap, &roots);

        assert_eq!(stats.objects_freed, 3);
        assert_eq!(stats.objects_remaining, 0);
    }

    #[test]
    fn test_heap_should_collect_threshold() {
        // Create heap with 1000 byte limit
        let mut heap = Heap::new(1000);
        assert!(!heap.should_collect());

        // Allocate objects until we hit 70% threshold
        // Each object is ~64 bytes (see heap.rs memory_estimate)
        for _ in 0..11 {
            // 11 * 64 = 704 bytes = 70.4%
            heap.allocate_object(test_class("Test", 0)).unwrap();
        }

        // Should trigger GC after crossing 70%
        assert!(heap.should_collect());
    }

    #[test]
    fn test_gc_preserves_rooted_objects() {
        let mut heap = test_heap();

        // Create live object graph: A -> B
        let id_b = heap.allocate_object(test_class("B", 0)).unwrap();
        let id_a = heap.allocate_object(test_class("A", 1)).unwrap();
        heap.set_field(id_a, 0, Value::Reference(ObjectRef::Object(id_b)))
            .unwrap();

        // Create dead object
        let _id_dead = heap.allocate_object(test_class("Dead", 0)).unwrap();

        // Root only A
        let mut roots = GCRoots::new();
        roots.add_heap_id(id_a);

        let stats = GarbageCollector::collect(&mut heap, &roots);

        // Should free 1 dead object, keep 2 live ones
        assert_eq!(stats.objects_freed, 1);
        assert_eq!(stats.objects_remaining, 2);

        // Verify both A and B are still accessible
        assert!(heap.get_entry(id_a).is_some());
        assert!(heap.get_entry(id_b).is_some());
    }

    #[test]
    fn integer_cache_is_a_heap_intrinsic_root() {
        let mut heap = test_heap();
        let cached = heap.box_integer_cached(42).unwrap();

        GarbageCollector::collect(&mut heap, &GCRoots::new());

        assert_eq!(heap.box_integer_cached(42).unwrap(), cached);
        assert_eq!(heap.get_boxed_int(cached).unwrap(), 42);
    }
}
