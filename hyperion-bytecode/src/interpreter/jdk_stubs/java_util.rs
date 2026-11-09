// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.util package stub implementations
//!
//! Collections framework: ArrayList, HashMap, HashSet, LinkedList, TreeMap, etc.

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

/// Create java.util.ArrayList class stub
pub fn create_array_list_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Collection;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Collection;)V"),
    );

    // Instance methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "add:(ILjava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("add", "(ILjava/lang/Object;)V"),
    );
    methods.insert(
        "addAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("addAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "addAll:(ILjava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("addAll", "(ILjava/util/Collection;)Z"),
    );
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(ILjava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("set", "(ILjava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "retainAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("retainAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "toArray:([Ljava/lang/Object;)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "listIterator:()Ljava/util/ListIterator;".to_string(),
        StubMethodDescriptor::instance_method("listIterator", "()Ljava/util/ListIterator;"),
    );
    methods.insert(
        "listIterator:(I)Ljava/util/ListIterator;".to_string(),
        StubMethodDescriptor::instance_method("listIterator", "(I)Ljava/util/ListIterator;"),
    );
    methods.insert(
        "subList:(II)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("subList", "(II)Ljava/util/List;"),
    );
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "trimToSize:()V".to_string(),
        StubMethodDescriptor::instance_method("trimToSize", "()V"),
    );
    methods.insert(
        "ensureCapacity:(I)V".to_string(),
        StubMethodDescriptor::instance_method("ensureCapacity", "(I)V"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "sort:(Ljava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::instance_method("sort", "(Ljava/util/Comparator;)V"),
    );
    methods.insert(
        "forEach:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/Consumer;)V"),
    );
    methods.insert(
        "stream:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("stream", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "parallelStream:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("parallelStream", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "removeIf:(Ljava/util/function/Predicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeIf", "(Ljava/util/function/Predicate;)Z"),
    );
    methods.insert(
        "replaceAll:(Ljava/util/function/UnaryOperator;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "replaceAll",
            "(Ljava/util/function/UnaryOperator;)V",
        ),
    );

    StubClass {
        name: "java/util/ArrayList".to_string(),
        superclass: Some("java/util/AbstractList".to_string()),
        interfaces: vec![
            "java/util/List".to_string(),
            "java/util/RandomAccess".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.LinkedList class stub
pub fn create_linked_list_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Collection;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Collection;)V"),
    );

    // List methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "add:(ILjava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("add", "(ILjava/lang/Object;)V"),
    );
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(ILjava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("set", "(ILjava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );

    // Deque methods
    methods.insert(
        "addFirst:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("addFirst", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "addLast:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("addLast", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "getFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "removeFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("removeFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "removeLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("removeLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "offer:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offer", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offerFirst:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offerFirst", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offerLast:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offerLast", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "peek:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peek", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peekFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peekFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peekLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peekLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "poll:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("poll", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "push:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("push", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "pop:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pop", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "descendingIterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("descendingIterator", "()Ljava/util/Iterator;"),
    );

    StubClass {
        name: "java/util/LinkedList".to_string(),
        superclass: Some("java/util/AbstractSequentialList".to_string()),
        interfaces: vec![
            "java/util/List".to_string(),
            "java/util/Deque".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.HashMap class stub
pub fn create_hash_map_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(IF)V".to_string(),
        StubMethodDescriptor::constructor("(IF)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Map;)V"),
    );

    // Instance methods
    methods.insert(
        "put:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "putAll:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::instance_method("putAll", "(Ljava/util/Map;)V"),
    );
    methods.insert(
        "putIfAbsent:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "putIfAbsent",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "getOrDefault:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getOrDefault",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "containsKey:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsKey", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsValue:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsValue", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "keySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("keySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "values:()Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::instance_method("values", "()Ljava/util/Collection;"),
    );
    methods.insert(
        "entrySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("entrySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "replace:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "replace",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "replace:(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "replace",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        ),
    );
    methods.insert(
        "compute:(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "compute",
            "(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "computeIfAbsent:(Ljava/lang/Object;Ljava/util/function/Function;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "computeIfAbsent",
            "(Ljava/lang/Object;Ljava/util/function/Function;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "computeIfPresent:(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "computeIfPresent",
            "(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "merge:(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "merge",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "forEach:(Ljava/util/function/BiConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/BiConsumer;)V"),
    );
    methods.insert(
        "replaceAll:(Ljava/util/function/BiFunction;)V".to_string(),
        StubMethodDescriptor::instance_method("replaceAll", "(Ljava/util/function/BiFunction;)V"),
    );

    StubClass {
        name: "java/util/HashMap".to_string(),
        superclass: Some("java/util/AbstractMap".to_string()),
        interfaces: vec![
            "java/util/Map".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.LinkedHashMap class stub
pub fn create_linked_hash_map_class() -> StubClass {
    // LinkedHashMap has same methods as HashMap but maintains insertion order
    let mut class = create_hash_map_class();
    class.name = "java/util/LinkedHashMap".to_string();
    class.superclass = Some("java/util/HashMap".to_string());
    class
}

/// Create java.util.HashSet class stub
pub fn create_hash_set_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(IF)V".to_string(),
        StubMethodDescriptor::constructor("(IF)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Collection;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Collection;)V"),
    );

    // Instance methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "addAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("addAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "retainAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("retainAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "toArray:([Ljava/lang/Object;)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "stream:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("stream", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "forEach:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/Consumer;)V"),
    );
    methods.insert(
        "removeIf:(Ljava/util/function/Predicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeIf", "(Ljava/util/function/Predicate;)Z"),
    );

    StubClass {
        name: "java/util/HashSet".to_string(),
        superclass: Some("java/util/AbstractSet".to_string()),
        interfaces: vec![
            "java/util/Set".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.TreeMap class stub
pub fn create_tree_map_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Comparator;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Map;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/SortedMap;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/SortedMap;)V"),
    );

    // Map methods
    methods.insert(
        "put:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "containsKey:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsKey", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsValue:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsValue", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "keySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("keySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "values:()Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::instance_method("values", "()Ljava/util/Collection;"),
    );
    methods.insert(
        "entrySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("entrySet", "()Ljava/util/Set;"),
    );

    // NavigableMap methods
    methods.insert(
        "firstKey:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("firstKey", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "lastKey:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("lastKey", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "firstEntry:()Ljava/util/Map$Entry;".to_string(),
        StubMethodDescriptor::instance_method("firstEntry", "()Ljava/util/Map$Entry;"),
    );
    methods.insert(
        "lastEntry:()Ljava/util/Map$Entry;".to_string(),
        StubMethodDescriptor::instance_method("lastEntry", "()Ljava/util/Map$Entry;"),
    );
    methods.insert(
        "pollFirstEntry:()Ljava/util/Map$Entry;".to_string(),
        StubMethodDescriptor::instance_method("pollFirstEntry", "()Ljava/util/Map$Entry;"),
    );
    methods.insert(
        "pollLastEntry:()Ljava/util/Map$Entry;".to_string(),
        StubMethodDescriptor::instance_method("pollLastEntry", "()Ljava/util/Map$Entry;"),
    );
    methods.insert(
        "lowerKey:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("lowerKey", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "floorKey:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("floorKey", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "ceilingKey:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "ceilingKey",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "higherKey:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "higherKey",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "comparator:()Ljava/util/Comparator;".to_string(),
        StubMethodDescriptor::instance_method("comparator", "()Ljava/util/Comparator;"),
    );
    methods.insert(
        "subMap:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/SortedMap;".to_string(),
        StubMethodDescriptor::instance_method(
            "subMap",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/SortedMap;",
        ),
    );
    methods.insert(
        "headMap:(Ljava/lang/Object;)Ljava/util/SortedMap;".to_string(),
        StubMethodDescriptor::instance_method(
            "headMap",
            "(Ljava/lang/Object;)Ljava/util/SortedMap;",
        ),
    );
    methods.insert(
        "tailMap:(Ljava/lang/Object;)Ljava/util/SortedMap;".to_string(),
        StubMethodDescriptor::instance_method(
            "tailMap",
            "(Ljava/lang/Object;)Ljava/util/SortedMap;",
        ),
    );
    methods.insert(
        "descendingMap:()Ljava/util/NavigableMap;".to_string(),
        StubMethodDescriptor::instance_method("descendingMap", "()Ljava/util/NavigableMap;"),
    );
    methods.insert(
        "navigableKeySet:()Ljava/util/NavigableSet;".to_string(),
        StubMethodDescriptor::instance_method("navigableKeySet", "()Ljava/util/NavigableSet;"),
    );
    methods.insert(
        "descendingKeySet:()Ljava/util/NavigableSet;".to_string(),
        StubMethodDescriptor::instance_method("descendingKeySet", "()Ljava/util/NavigableSet;"),
    );

    StubClass {
        name: "java/util/TreeMap".to_string(),
        superclass: Some("java/util/AbstractMap".to_string()),
        interfaces: vec![
            "java/util/NavigableMap".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.TreeSet class stub
pub fn create_tree_set_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Comparator;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Collection;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Collection;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/SortedSet;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/SortedSet;)V"),
    );

    // Set methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );

    // NavigableSet methods
    methods.insert(
        "first:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("first", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "last:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("last", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "lower:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("lower", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "floor:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("floor", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "ceiling:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("ceiling", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "higher:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("higher", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "comparator:()Ljava/util/Comparator;".to_string(),
        StubMethodDescriptor::instance_method("comparator", "()Ljava/util/Comparator;"),
    );
    methods.insert(
        "descendingSet:()Ljava/util/NavigableSet;".to_string(),
        StubMethodDescriptor::instance_method("descendingSet", "()Ljava/util/NavigableSet;"),
    );
    methods.insert(
        "descendingIterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("descendingIterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "subSet:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/SortedSet;".to_string(),
        StubMethodDescriptor::instance_method(
            "subSet",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/SortedSet;",
        ),
    );
    methods.insert(
        "headSet:(Ljava/lang/Object;)Ljava/util/SortedSet;".to_string(),
        StubMethodDescriptor::instance_method(
            "headSet",
            "(Ljava/lang/Object;)Ljava/util/SortedSet;",
        ),
    );
    methods.insert(
        "tailSet:(Ljava/lang/Object;)Ljava/util/SortedSet;".to_string(),
        StubMethodDescriptor::instance_method(
            "tailSet",
            "(Ljava/lang/Object;)Ljava/util/SortedSet;",
        ),
    );

    StubClass {
        name: "java/util/TreeSet".to_string(),
        superclass: Some("java/util/AbstractSet".to_string()),
        interfaces: vec![
            "java/util/NavigableSet".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.List interface stub
pub fn create_list_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "sort:(Ljava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::instance_method("sort", "(Ljava/util/Comparator;)V"),
    );

    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "add:(ILjava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("add", "(ILjava/lang/Object;)V"),
    );
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(ILjava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("set", "(ILjava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "subList:(II)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("subList", "(II)Ljava/util/List;"),
    );

    // Static factory methods (Java 9+)
    methods.insert(
        "of:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("of", "()Ljava/util/List;"),
    );
    methods.insert(
        "of:(Ljava/lang/Object;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("of", "(Ljava/lang/Object;)Ljava/util/List;"),
    );
    methods.insert(
        "of:([Ljava/lang/Object;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("of", "([Ljava/lang/Object;)Ljava/util/List;"),
    );

    StubClass {
        name: "java/util/List".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Collection".to_string()],
        methods,
    }
}

/// Create java.util.Map interface stub
pub fn create_map_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "put:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "containsKey:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsKey", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsValue:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsValue", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "keySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("keySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "values:()Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::instance_method("values", "()Ljava/util/Collection;"),
    );
    methods.insert(
        "entrySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("entrySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );

    // Static factory methods (Java 9+)
    methods.insert(
        "of:()Ljava/util/Map;".to_string(),
        StubMethodDescriptor::static_method("of", "()Ljava/util/Map;"),
    );
    methods.insert(
        "of:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;".to_string(),
        StubMethodDescriptor::static_method(
            "of",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;",
        ),
    );

    StubClass {
        name: "java/util/Map".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Set interface stub
pub fn create_set_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );

    // Static factory methods (Java 9+)
    methods.insert(
        "of:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::static_method("of", "()Ljava/util/Set;"),
    );
    methods.insert(
        "of:(Ljava/lang/Object;)Ljava/util/Set;".to_string(),
        StubMethodDescriptor::static_method("of", "(Ljava/lang/Object;)Ljava/util/Set;"),
    );
    methods.insert(
        "of:([Ljava/lang/Object;)Ljava/util/Set;".to_string(),
        StubMethodDescriptor::static_method("of", "([Ljava/lang/Object;)Ljava/util/Set;"),
    );

    StubClass {
        name: "java/util/Set".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Collection".to_string()],
        methods,
    }
}

/// Create java.util.Collection interface stub
pub fn create_collection_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "addAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("addAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "retainAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("retainAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "toArray:([Ljava/lang/Object;)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "stream:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("stream", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "parallelStream:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("parallelStream", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "forEach:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/Consumer;)V"),
    );
    methods.insert(
        "removeIf:(Ljava/util/function/Predicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeIf", "(Ljava/util/function/Predicate;)Z"),
    );

    StubClass {
        name: "java/util/Collection".to_string(),
        superclass: None,
        interfaces: vec!["java/lang/Iterable".to_string()],
        methods,
    }
}

/// Create java.lang.Iterable interface stub
pub fn create_iterable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "forEach:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/Consumer;)V"),
    );
    methods.insert(
        "spliterator:()Ljava/util/Spliterator;".to_string(),
        StubMethodDescriptor::instance_method("spliterator", "()Ljava/util/Spliterator;"),
    );

    StubClass {
        name: "java/lang/Iterable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Iterator interface stub
pub fn create_iterator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "hasNext:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNext", "()Z"),
    );
    methods.insert(
        "next:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("next", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:()V".to_string(),
        StubMethodDescriptor::instance_method("remove", "()V"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/Consumer;)V",
        ),
    );

    StubClass {
        name: "java/util/Iterator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.PrimitiveIterator interface stub
pub fn create_primitive_iterator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "forEachRemaining:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("forEachRemaining", "(Ljava/lang/Object;)V"),
    );

    StubClass {
        name: "java/util/PrimitiveIterator".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Iterator".to_string()],
        methods,
    }
}

/// Create java.util.PrimitiveIterator$OfInt interface stub
pub fn create_primitive_iterator_of_int_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "nextInt:()I".to_string(),
        StubMethodDescriptor::instance_method("nextInt", "()I"),
    );
    methods.insert(
        "hasNext:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNext", "()Z"),
    );
    methods.insert(
        "next:()Ljava/lang/Integer;".to_string(),
        StubMethodDescriptor::instance_method("next", "()Ljava/lang/Integer;"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/IntConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/IntConsumer;)V",
        ),
    );

    StubClass {
        name: "java/util/PrimitiveIterator$OfInt".to_string(),
        superclass: None,
        interfaces: vec!["java/util/PrimitiveIterator".to_string()],
        methods,
    }
}

/// Create java.util.PrimitiveIterator$OfLong interface stub
pub fn create_primitive_iterator_of_long_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "nextLong:()J".to_string(),
        StubMethodDescriptor::instance_method("nextLong", "()J"),
    );
    methods.insert(
        "hasNext:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNext", "()Z"),
    );
    methods.insert(
        "next:()Ljava/lang/Long;".to_string(),
        StubMethodDescriptor::instance_method("next", "()Ljava/lang/Long;"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/LongConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/LongConsumer;)V",
        ),
    );

    StubClass {
        name: "java/util/PrimitiveIterator$OfLong".to_string(),
        superclass: None,
        interfaces: vec!["java/util/PrimitiveIterator".to_string()],
        methods,
    }
}

/// Create java.util.PrimitiveIterator$OfDouble interface stub
pub fn create_primitive_iterator_of_double_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "nextDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("nextDouble", "()D"),
    );
    methods.insert(
        "hasNext:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNext", "()Z"),
    );
    methods.insert(
        "next:()Ljava/lang/Double;".to_string(),
        StubMethodDescriptor::instance_method("next", "()Ljava/lang/Double;"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/DoubleConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/DoubleConsumer;)V",
        ),
    );

    StubClass {
        name: "java/util/PrimitiveIterator$OfDouble".to_string(),
        superclass: None,
        interfaces: vec!["java/util/PrimitiveIterator".to_string()],
        methods,
    }
}

/// Create java.util.Arrays utility class stub
pub fn create_arrays_class() -> StubClass {
    let mut methods = HashMap::new();

    // Sort methods
    methods.insert(
        "sort:([I)V".to_string(),
        StubMethodDescriptor::static_method("sort", "([I)V"),
    );
    methods.insert(
        "sort:([J)V".to_string(),
        StubMethodDescriptor::static_method("sort", "([J)V"),
    );
    methods.insert(
        "sort:([D)V".to_string(),
        StubMethodDescriptor::static_method("sort", "([D)V"),
    );
    methods.insert(
        "sort:([Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::static_method("sort", "([Ljava/lang/Object;)V"),
    );
    methods.insert(
        "sort:([Ljava/lang/Object;Ljava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::static_method("sort", "([Ljava/lang/Object;Ljava/util/Comparator;)V"),
    );
    methods.insert(
        "sort:([III)V".to_string(),
        StubMethodDescriptor::static_method("sort", "([III)V"),
    );

    // Binary search
    methods.insert(
        "binarySearch:([II)I".to_string(),
        StubMethodDescriptor::static_method("binarySearch", "([II)I"),
    );
    methods.insert(
        "binarySearch:([JJ)I".to_string(),
        StubMethodDescriptor::static_method("binarySearch", "([JJ)I"),
    );
    methods.insert(
        "binarySearch:([Ljava/lang/Object;Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method(
            "binarySearch",
            "([Ljava/lang/Object;Ljava/lang/Object;)I",
        ),
    );

    // Fill methods
    methods.insert(
        "fill:([II)V".to_string(),
        StubMethodDescriptor::static_method("fill", "([II)V"),
    );
    methods.insert(
        "fill:([JJ)V".to_string(),
        StubMethodDescriptor::static_method("fill", "([JJ)V"),
    );
    methods.insert(
        "fill:([DD)V".to_string(),
        StubMethodDescriptor::static_method("fill", "([DD)V"),
    );
    methods.insert(
        "fill:([ZZ)V".to_string(),
        StubMethodDescriptor::static_method("fill", "([ZZ)V"),
    );
    methods.insert(
        "fill:([Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::static_method("fill", "([Ljava/lang/Object;Ljava/lang/Object;)V"),
    );

    // Copy methods
    methods.insert(
        "copyOf:([II)[I".to_string(),
        StubMethodDescriptor::static_method("copyOf", "([II)[I"),
    );
    methods.insert(
        "copyOf:([JI)[J".to_string(),
        StubMethodDescriptor::static_method("copyOf", "([JI)[J"),
    );
    methods.insert(
        "copyOf:([DI)[D".to_string(),
        StubMethodDescriptor::static_method("copyOf", "([DI)[D"),
    );
    methods.insert(
        "copyOf:([Ljava/lang/Object;I)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method("copyOf", "([Ljava/lang/Object;I)[Ljava/lang/Object;"),
    );
    methods.insert(
        "copyOfRange:([III)[I".to_string(),
        StubMethodDescriptor::static_method("copyOfRange", "([III)[I"),
    );
    methods.insert(
        "copyOfRange:([Ljava/lang/Object;II)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method(
            "copyOfRange",
            "([Ljava/lang/Object;II)[Ljava/lang/Object;",
        ),
    );

    // Equals methods
    methods.insert(
        "equals:([I[I)Z".to_string(),
        StubMethodDescriptor::static_method("equals", "([I[I)Z"),
    );
    methods.insert(
        "equals:([J[J)Z".to_string(),
        StubMethodDescriptor::static_method("equals", "([J[J)Z"),
    );
    methods.insert(
        "equals:([D[D)Z".to_string(),
        StubMethodDescriptor::static_method("equals", "([D[D)Z"),
    );
    methods.insert(
        "equals:([Ljava/lang/Object;[Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::static_method("equals", "([Ljava/lang/Object;[Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "deepEquals:([Ljava/lang/Object;[Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "deepEquals",
            "([Ljava/lang/Object;[Ljava/lang/Object;)Z",
        ),
    );

    // HashCode methods
    methods.insert(
        "hashCode:([I)I".to_string(),
        StubMethodDescriptor::static_method("hashCode", "([I)I"),
    );
    methods.insert(
        "hashCode:([Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method("hashCode", "([Ljava/lang/Object;)I"),
    );
    methods.insert(
        "deepHashCode:([Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method("deepHashCode", "([Ljava/lang/Object;)I"),
    );

    // ToString methods
    methods.insert(
        "toString:([I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "([I)Ljava/lang/String;"),
    );
    methods.insert(
        "toString:([Ljava/lang/Object;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "([Ljava/lang/Object;)Ljava/lang/String;"),
    );
    methods.insert(
        "deepToString:([Ljava/lang/Object;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "deepToString",
            "([Ljava/lang/Object;)Ljava/lang/String;",
        ),
    );

    // asList
    methods.insert(
        "asList:([Ljava/lang/Object;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("asList", "([Ljava/lang/Object;)Ljava/util/List;"),
    );

    // Stream
    methods.insert(
        "stream:([Ljava/lang/Object;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "stream",
            "([Ljava/lang/Object;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "stream:([I)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method("stream", "([I)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "stream:([J)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method("stream", "([J)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "stream:([D)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::static_method("stream", "([D)Ljava/util/stream/DoubleStream;"),
    );

    StubClass {
        name: "java/util/Arrays".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Collections utility class stub
pub fn create_collections_class() -> StubClass {
    let mut methods = HashMap::new();

    // Sort/shuffle
    methods.insert(
        "sort:(Ljava/util/List;)V".to_string(),
        StubMethodDescriptor::static_method("sort", "(Ljava/util/List;)V"),
    );
    methods.insert(
        "sort:(Ljava/util/List;Ljava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::static_method("sort", "(Ljava/util/List;Ljava/util/Comparator;)V"),
    );
    methods.insert(
        "shuffle:(Ljava/util/List;)V".to_string(),
        StubMethodDescriptor::static_method("shuffle", "(Ljava/util/List;)V"),
    );
    methods.insert(
        "reverse:(Ljava/util/List;)V".to_string(),
        StubMethodDescriptor::static_method("reverse", "(Ljava/util/List;)V"),
    );
    methods.insert(
        "swap:(Ljava/util/List;II)V".to_string(),
        StubMethodDescriptor::static_method("swap", "(Ljava/util/List;II)V"),
    );
    methods.insert(
        "fill:(Ljava/util/List;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::static_method("fill", "(Ljava/util/List;Ljava/lang/Object;)V"),
    );
    methods.insert(
        "copy:(Ljava/util/List;Ljava/util/List;)V".to_string(),
        StubMethodDescriptor::static_method("copy", "(Ljava/util/List;Ljava/util/List;)V"),
    );

    // Min/max
    methods.insert(
        "min:(Ljava/util/Collection;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method("min", "(Ljava/util/Collection;)Ljava/lang/Object;"),
    );
    methods.insert(
        "min:(Ljava/util/Collection;Ljava/util/Comparator;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method(
            "min",
            "(Ljava/util/Collection;Ljava/util/Comparator;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "max:(Ljava/util/Collection;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method("max", "(Ljava/util/Collection;)Ljava/lang/Object;"),
    );
    methods.insert(
        "max:(Ljava/util/Collection;Ljava/util/Comparator;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method(
            "max",
            "(Ljava/util/Collection;Ljava/util/Comparator;)Ljava/lang/Object;",
        ),
    );

    // Binary search
    methods.insert(
        "binarySearch:(Ljava/util/List;Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method(
            "binarySearch",
            "(Ljava/util/List;Ljava/lang/Object;)I",
        ),
    );

    // Singleton/empty
    methods.insert(
        "singleton:(Ljava/lang/Object;)Ljava/util/Set;".to_string(),
        StubMethodDescriptor::static_method("singleton", "(Ljava/lang/Object;)Ljava/util/Set;"),
    );
    methods.insert(
        "singletonList:(Ljava/lang/Object;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method(
            "singletonList",
            "(Ljava/lang/Object;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "singletonMap:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;".to_string(),
        StubMethodDescriptor::static_method(
            "singletonMap",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/util/Map;",
        ),
    );
    methods.insert(
        "emptyList:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("emptyList", "()Ljava/util/List;"),
    );
    methods.insert(
        "emptySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::static_method("emptySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "emptyMap:()Ljava/util/Map;".to_string(),
        StubMethodDescriptor::static_method("emptyMap", "()Ljava/util/Map;"),
    );
    methods.insert(
        "emptyIterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::static_method("emptyIterator", "()Ljava/util/Iterator;"),
    );

    // Unmodifiable wrappers
    methods.insert(
        "unmodifiableList:(Ljava/util/List;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method(
            "unmodifiableList",
            "(Ljava/util/List;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "unmodifiableSet:(Ljava/util/Set;)Ljava/util/Set;".to_string(),
        StubMethodDescriptor::static_method("unmodifiableSet", "(Ljava/util/Set;)Ljava/util/Set;"),
    );
    methods.insert(
        "unmodifiableMap:(Ljava/util/Map;)Ljava/util/Map;".to_string(),
        StubMethodDescriptor::static_method("unmodifiableMap", "(Ljava/util/Map;)Ljava/util/Map;"),
    );
    methods.insert(
        "unmodifiableCollection:(Ljava/util/Collection;)Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::static_method(
            "unmodifiableCollection",
            "(Ljava/util/Collection;)Ljava/util/Collection;",
        ),
    );

    // Synchronized wrappers
    methods.insert(
        "synchronizedList:(Ljava/util/List;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method(
            "synchronizedList",
            "(Ljava/util/List;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "synchronizedSet:(Ljava/util/Set;)Ljava/util/Set;".to_string(),
        StubMethodDescriptor::static_method("synchronizedSet", "(Ljava/util/Set;)Ljava/util/Set;"),
    );
    methods.insert(
        "synchronizedMap:(Ljava/util/Map;)Ljava/util/Map;".to_string(),
        StubMethodDescriptor::static_method("synchronizedMap", "(Ljava/util/Map;)Ljava/util/Map;"),
    );

    // Frequency/disjoint
    methods.insert(
        "frequency:(Ljava/util/Collection;Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method(
            "frequency",
            "(Ljava/util/Collection;Ljava/lang/Object;)I",
        ),
    );
    methods.insert(
        "disjoint:(Ljava/util/Collection;Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "disjoint",
            "(Ljava/util/Collection;Ljava/util/Collection;)Z",
        ),
    );

    // nCopies
    methods.insert(
        "nCopies:(ILjava/lang/Object;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("nCopies", "(ILjava/lang/Object;)Ljava/util/List;"),
    );

    // Comparator
    methods.insert(
        "reverseOrder:()Ljava/util/Comparator;".to_string(),
        StubMethodDescriptor::static_method("reverseOrder", "()Ljava/util/Comparator;"),
    );
    methods.insert(
        "reverseOrder:(Ljava/util/Comparator;)Ljava/util/Comparator;".to_string(),
        StubMethodDescriptor::static_method(
            "reverseOrder",
            "(Ljava/util/Comparator;)Ljava/util/Comparator;",
        ),
    );

    // Enumeration
    methods.insert(
        "enumeration:(Ljava/util/Collection;)Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::static_method(
            "enumeration",
            "(Ljava/util/Collection;)Ljava/util/Enumeration;",
        ),
    );
    methods.insert(
        "list:(Ljava/util/Enumeration;)Ljava/util/ArrayList;".to_string(),
        StubMethodDescriptor::static_method(
            "list",
            "(Ljava/util/Enumeration;)Ljava/util/ArrayList;",
        ),
    );

    StubClass {
        name: "java/util/Collections".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Objects utility class stub
pub fn create_objects_class() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "equals:(Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::static_method("equals", "(Ljava/lang/Object;Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "deepEquals:(Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "deepEquals",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        ),
    );
    methods.insert(
        "hashCode:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method("hashCode", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "hash:([Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method("hash", "([Ljava/lang/Object;)I"),
    );
    methods.insert(
        "toString:(Ljava/lang/Object;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(Ljava/lang/Object;)Ljava/lang/String;"),
    );
    methods.insert(
        "toString:(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "toString",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "compare:(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/Comparator;)I".to_string(),
        StubMethodDescriptor::static_method(
            "compare",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/Comparator;)I",
        ),
    );
    methods.insert(
        "requireNonNull:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method(
            "requireNonNull",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "requireNonNull:(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method(
            "requireNonNull",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "requireNonNullElse:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method(
            "requireNonNullElse",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "requireNonNullElseGet:(Ljava/lang/Object;Ljava/util/function/Supplier;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "requireNonNullElseGet",
            "(Ljava/lang/Object;Ljava/util/function/Supplier;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "isNull:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::static_method("isNull", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "nonNull:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::static_method("nonNull", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "checkIndex:(II)I".to_string(),
        StubMethodDescriptor::static_method("checkIndex", "(II)I"),
    );
    methods.insert(
        "checkFromToIndex:(III)I".to_string(),
        StubMethodDescriptor::static_method("checkFromToIndex", "(III)I"),
    );
    methods.insert(
        "checkFromIndexSize:(III)I".to_string(),
        StubMethodDescriptor::static_method("checkFromIndexSize", "(III)I"),
    );

    StubClass {
        name: "java/util/Objects".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Optional class stub
pub fn create_optional_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "empty:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::static_method("empty", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "of:(Ljava/lang/Object;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::static_method("of", "(Ljava/lang/Object;)Ljava/util/Optional;"),
    );
    methods.insert(
        "ofNullable:(Ljava/lang/Object;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::static_method(
            "ofNullable",
            "(Ljava/lang/Object;)Ljava/util/Optional;",
        ),
    );

    // Instance methods
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "isPresent:()Z".to_string(),
        StubMethodDescriptor::instance_method("isPresent", "()Z"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "ifPresent:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("ifPresent", "(Ljava/util/function/Consumer;)V"),
    );
    methods.insert(
        "ifPresentOrElse:(Ljava/util/function/Consumer;Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "ifPresentOrElse",
            "(Ljava/util/function/Consumer;Ljava/lang/Runnable;)V",
        ),
    );
    methods.insert(
        "filter:(Ljava/util/function/Predicate;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method(
            "filter",
            "(Ljava/util/function/Predicate;)Ljava/util/Optional;",
        ),
    );
    methods.insert(
        "map:(Ljava/util/function/Function;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method(
            "map",
            "(Ljava/util/function/Function;)Ljava/util/Optional;",
        ),
    );
    methods.insert(
        "flatMap:(Ljava/util/function/Function;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method(
            "flatMap",
            "(Ljava/util/function/Function;)Ljava/util/Optional;",
        ),
    );
    methods.insert(
        "or:(Ljava/util/function/Supplier;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method(
            "or",
            "(Ljava/util/function/Supplier;)Ljava/util/Optional;",
        ),
    );
    methods.insert(
        "orElse:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("orElse", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "orElseGet:(Ljava/util/function/Supplier;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "orElseGet",
            "(Ljava/util/function/Supplier;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "orElseThrow:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("orElseThrow", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "orElseThrow:(Ljava/util/function/Supplier;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "orElseThrow",
            "(Ljava/util/function/Supplier;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "stream:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("stream", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/Optional".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Queue interface stub
pub fn create_queue_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Queue-specific methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offer:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offer", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "remove:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "poll:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("poll", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "element:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("element", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peek:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peek", "()Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/Queue".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Collection".to_string()],
        methods,
    }
}

/// Create java.util.Deque interface stub
pub fn create_deque_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Deque-specific methods
    methods.insert(
        "addFirst:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("addFirst", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "addLast:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("addLast", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "offerFirst:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offerFirst", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offerLast:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offerLast", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("removeFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "removeLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("removeLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peekFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peekFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peekLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peekLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "removeFirstOccurrence:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeFirstOccurrence", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeLastOccurrence:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeLastOccurrence", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "push:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("push", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "pop:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pop", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "descendingIterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("descendingIterator", "()Ljava/util/Iterator;"),
    );

    // Queue methods (inherited)
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offer:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offer", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "remove:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "poll:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("poll", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "element:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("element", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peek:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peek", "()Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/Deque".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Queue".to_string()],
        methods,
    }
}

/// Create java.util.Stack class stub
pub fn create_stack_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Stack-specific methods
    methods.insert(
        "push:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("push", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "pop:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pop", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peek:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peek", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "empty:()Z".to_string(),
        StubMethodDescriptor::instance_method("empty", "()Z"),
    );
    methods.insert(
        "search:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("search", "(Ljava/lang/Object;)I"),
    );

    // Vector methods (inherited)
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "add:(ILjava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("add", "(ILjava/lang/Object;)V"),
    );
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(ILjava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("set", "(ILjava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/Stack".to_string(),
        superclass: Some("java/util/Vector".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Vector class stub
pub fn create_vector_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(II)V".to_string(),
        StubMethodDescriptor::constructor("(II)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Collection;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Collection;)V"),
    );

    // List methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "add:(ILjava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("add", "(ILjava/lang/Object;)V"),
    );
    methods.insert(
        "addAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("addAll", "(Ljava/util/Collection;)Z"),
    );
    methods.insert(
        "addAll:(ILjava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("addAll", "(ILjava/util/Collection;)Z"),
    );
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(ILjava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("set", "(ILjava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "toArray:([Ljava/lang/Object;)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
        ),
    );

    // Vector-specific methods
    methods.insert(
        "addElement:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("addElement", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "removeElement:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeElement", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeElementAt:(I)V".to_string(),
        StubMethodDescriptor::instance_method("removeElementAt", "(I)V"),
    );
    methods.insert(
        "insertElementAt:(Ljava/lang/Object;I)V".to_string(),
        StubMethodDescriptor::instance_method("insertElementAt", "(Ljava/lang/Object;I)V"),
    );
    methods.insert(
        "setElementAt:(Ljava/lang/Object;I)V".to_string(),
        StubMethodDescriptor::instance_method("setElementAt", "(Ljava/lang/Object;I)V"),
    );
    methods.insert(
        "elementAt:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("elementAt", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "firstElement:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("firstElement", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "lastElement:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("lastElement", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "elements:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("elements", "()Ljava/util/Enumeration;"),
    );
    methods.insert(
        "capacity:()I".to_string(),
        StubMethodDescriptor::instance_method("capacity", "()I"),
    );
    methods.insert(
        "ensureCapacity:(I)V".to_string(),
        StubMethodDescriptor::instance_method("ensureCapacity", "(I)V"),
    );
    methods.insert(
        "trimToSize:()V".to_string(),
        StubMethodDescriptor::instance_method("trimToSize", "()V"),
    );
    methods.insert(
        "setSize:(I)V".to_string(),
        StubMethodDescriptor::instance_method("setSize", "(I)V"),
    );
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/Vector".to_string(),
        superclass: Some("java/util/AbstractList".to_string()),
        interfaces: vec![
            "java/util/List".to_string(),
            "java/util/RandomAccess".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.PriorityQueue class stub
pub fn create_priority_queue_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Comparator;)V"),
    );
    methods.insert(
        "<init>:(ILjava/util/Comparator;)V".to_string(),
        StubMethodDescriptor::constructor("(ILjava/util/Comparator;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Collection;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Collection;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/PriorityQueue;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/PriorityQueue;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/SortedSet;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/SortedSet;)V"),
    );

    // Queue methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offer:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offer", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "peek:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peek", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "poll:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("poll", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "toArray:([Ljava/lang/Object;)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "comparator:()Ljava/util/Comparator;".to_string(),
        StubMethodDescriptor::instance_method("comparator", "()Ljava/util/Comparator;"),
    );
    methods.insert(
        "spliterator:()Ljava/util/Spliterator;".to_string(),
        StubMethodDescriptor::instance_method("spliterator", "()Ljava/util/Spliterator;"),
    );

    StubClass {
        name: "java/util/PriorityQueue".to_string(),
        superclass: Some("java/util/AbstractQueue".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.AbstractQueue class stub
pub fn create_abstract_queue_class() -> StubClass {
    let mut methods = HashMap::new();

    // Queue methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "remove:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "element:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("element", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "addAll:(Ljava/util/Collection;)Z".to_string(),
        StubMethodDescriptor::instance_method("addAll", "(Ljava/util/Collection;)Z"),
    );

    StubClass {
        name: "java/util/AbstractQueue".to_string(),
        superclass: Some("java/util/AbstractCollection".to_string()),
        interfaces: vec!["java/util/Queue".to_string()],
        methods,
    }
}

/// Create java.util.ArrayDeque class stub
pub fn create_array_deque_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Collection;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Collection;)V"),
    );

    // Deque methods
    methods.insert(
        "addFirst:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("addFirst", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "addLast:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("addLast", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "offerFirst:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offerFirst", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offerLast:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offerLast", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("removeFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "removeLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("removeLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "pollLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pollLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peekFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peekFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peekLast:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peekLast", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "push:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("push", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "pop:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("pop", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "removeFirstOccurrence:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeFirstOccurrence", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "removeLastOccurrence:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeLastOccurrence", "(Ljava/lang/Object;)Z"),
    );

    // Queue methods
    methods.insert(
        "add:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("add", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "offer:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("offer", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "remove:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "poll:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("poll", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "element:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("element", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "peek:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("peek", "()Ljava/lang/Object;"),
    );

    // Collection methods
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "toArray:([Ljava/lang/Object;)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "toArray",
            "([Ljava/lang/Object;)[Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "descendingIterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("descendingIterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "spliterator:()Ljava/util/Spliterator;".to_string(),
        StubMethodDescriptor::instance_method("spliterator", "()Ljava/util/Spliterator;"),
    );
    methods.insert(
        "clone:()Ljava/util/ArrayDeque;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/util/ArrayDeque;"),
    );

    StubClass {
        name: "java/util/ArrayDeque".to_string(),
        superclass: Some("java/util/AbstractCollection".to_string()),
        interfaces: vec![
            "java/util/Deque".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.Dictionary abstract class stub
pub fn create_dictionary_class() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract methods
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "keys:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("keys", "()Ljava/util/Enumeration;"),
    );
    methods.insert(
        "elements:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("elements", "()Ljava/util/Enumeration;"),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "put:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/Dictionary".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Enumeration interface stub
pub fn create_enumeration_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "hasMoreElements:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasMoreElements", "()Z"),
    );
    methods.insert(
        "nextElement:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("nextElement", "()Ljava/lang/Object;"),
    );
    // Default method (Java 9+)
    methods.insert(
        "asIterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("asIterator", "()Ljava/util/Iterator;"),
    );

    StubClass {
        name: "java/util/Enumeration".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Hashtable class stub
pub fn create_hashtable_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(IF)V".to_string(),
        StubMethodDescriptor::constructor("(IF)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Map;)V"),
    );

    // Map methods
    methods.insert(
        "put:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "containsKey:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsKey", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsValue:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsValue", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "keySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("keySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "values:()Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::instance_method("values", "()Ljava/util/Collection;"),
    );
    methods.insert(
        "entrySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("entrySet", "()Ljava/util/Set;"),
    );

    // Legacy Dictionary methods
    methods.insert(
        "keys:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("keys", "()Ljava/util/Enumeration;"),
    );
    methods.insert(
        "elements:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("elements", "()Ljava/util/Enumeration;"),
    );

    // Utility methods
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );

    // Java 8+ Map default methods
    methods.insert(
        "getOrDefault:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getOrDefault",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "putIfAbsent:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "putIfAbsent",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "replace:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "replace",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "replace:(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "replace",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Z",
        ),
    );
    methods.insert(
        "compute:(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "compute",
            "(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "computeIfAbsent:(Ljava/lang/Object;Ljava/util/function/Function;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "computeIfAbsent",
            "(Ljava/lang/Object;Ljava/util/function/Function;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "computeIfPresent:(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "computeIfPresent",
            "(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "merge:(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "merge",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "forEach:(Ljava/util/function/BiConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/BiConsumer;)V"),
    );
    methods.insert(
        "replaceAll:(Ljava/util/function/BiFunction;)V".to_string(),
        StubMethodDescriptor::instance_method("replaceAll", "(Ljava/util/function/BiFunction;)V"),
    );

    StubClass {
        name: "java/util/Hashtable".to_string(),
        superclass: Some("java/util/Dictionary".to_string()),
        interfaces: vec![
            "java/util/Map".to_string(),
            "java/lang/Cloneable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.Properties class stub
pub fn create_properties_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Properties;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Properties;)V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );

    // Properties-specific methods
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "getProperty:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "propertyNames:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("propertyNames", "()Ljava/util/Enumeration;"),
    );
    methods.insert(
        "stringPropertyNames:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("stringPropertyNames", "()Ljava/util/Set;"),
    );

    // Load/Store methods
    methods.insert(
        "load:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::instance_method("load", "(Ljava/io/InputStream;)V"),
    );
    methods.insert(
        "load:(Ljava/io/Reader;)V".to_string(),
        StubMethodDescriptor::instance_method("load", "(Ljava/io/Reader;)V"),
    );
    methods.insert(
        "loadFromXML:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::instance_method("loadFromXML", "(Ljava/io/InputStream;)V"),
    );
    methods.insert(
        "store:(Ljava/io/OutputStream;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "store",
            "(Ljava/io/OutputStream;Ljava/lang/String;)V",
        ),
    );
    methods.insert(
        "store:(Ljava/io/Writer;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("store", "(Ljava/io/Writer;Ljava/lang/String;)V"),
    );
    methods.insert(
        "storeToXML:(Ljava/io/OutputStream;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "storeToXML",
            "(Ljava/io/OutputStream;Ljava/lang/String;)V",
        ),
    );
    methods.insert(
        "storeToXML:(Ljava/io/OutputStream;Ljava/lang/String;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "storeToXML",
            "(Ljava/io/OutputStream;Ljava/lang/String;Ljava/lang/String;)V",
        ),
    );

    // List methods
    methods.insert(
        "list:(Ljava/io/PrintStream;)V".to_string(),
        StubMethodDescriptor::instance_method("list", "(Ljava/io/PrintStream;)V"),
    );
    methods.insert(
        "list:(Ljava/io/PrintWriter;)V".to_string(),
        StubMethodDescriptor::instance_method("list", "(Ljava/io/PrintWriter;)V"),
    );

    // Inherited from Hashtable - need to redefine for proper resolution
    methods.insert(
        "put:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "remove:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "containsKey:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsKey", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "clear:()V".to_string(),
        StubMethodDescriptor::instance_method("clear", "()V"),
    );
    methods.insert(
        "keySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("keySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "entrySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("entrySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "values:()Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::instance_method("values", "()Ljava/util/Collection;"),
    );
    methods.insert(
        "keys:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("keys", "()Ljava/util/Enumeration;"),
    );
    methods.insert(
        "elements:()Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method("elements", "()Ljava/util/Enumeration;"),
    );
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/Properties".to_string(),
        superclass: Some("java/util/Hashtable".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Scanner class stub
pub fn create_scanner_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/InputStream;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/nio/file/Path;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/nio/file/Path;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Readable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Readable;)V"),
    );

    // Pattern/delimiter
    methods.insert(
        "useDelimiter:(Ljava/util/regex/Pattern;)Ljava/util/Scanner;".to_string(),
        StubMethodDescriptor::instance_method(
            "useDelimiter",
            "(Ljava/util/regex/Pattern;)Ljava/util/Scanner;",
        ),
    );
    methods.insert(
        "useDelimiter:(Ljava/lang/String;)Ljava/util/Scanner;".to_string(),
        StubMethodDescriptor::instance_method(
            "useDelimiter",
            "(Ljava/lang/String;)Ljava/util/Scanner;",
        ),
    );
    methods.insert(
        "delimiter:()Ljava/util/regex/Pattern;".to_string(),
        StubMethodDescriptor::instance_method("delimiter", "()Ljava/util/regex/Pattern;"),
    );

    // Locale
    methods.insert(
        "useLocale:(Ljava/util/Locale;)Ljava/util/Scanner;".to_string(),
        StubMethodDescriptor::instance_method(
            "useLocale",
            "(Ljava/util/Locale;)Ljava/util/Scanner;",
        ),
    );
    methods.insert(
        "locale:()Ljava/util/Locale;".to_string(),
        StubMethodDescriptor::instance_method("locale", "()Ljava/util/Locale;"),
    );

    // Radix
    methods.insert(
        "useRadix:(I)Ljava/util/Scanner;".to_string(),
        StubMethodDescriptor::instance_method("useRadix", "(I)Ljava/util/Scanner;"),
    );
    methods.insert(
        "radix:()I".to_string(),
        StubMethodDescriptor::instance_method("radix", "()I"),
    );

    // hasNext variants
    methods.insert(
        "hasNext:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNext", "()Z"),
    );
    methods.insert(
        "hasNext:(Ljava/util/regex/Pattern;)Z".to_string(),
        StubMethodDescriptor::instance_method("hasNext", "(Ljava/util/regex/Pattern;)Z"),
    );
    methods.insert(
        "hasNext:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("hasNext", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "hasNextLine:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextLine", "()Z"),
    );
    methods.insert(
        "hasNextInt:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextInt", "()Z"),
    );
    methods.insert(
        "hasNextInt:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextInt", "(I)Z"),
    );
    methods.insert(
        "hasNextLong:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextLong", "()Z"),
    );
    methods.insert(
        "hasNextLong:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextLong", "(I)Z"),
    );
    methods.insert(
        "hasNextDouble:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextDouble", "()Z"),
    );
    methods.insert(
        "hasNextFloat:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextFloat", "()Z"),
    );
    methods.insert(
        "hasNextBoolean:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextBoolean", "()Z"),
    );
    methods.insert(
        "hasNextByte:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextByte", "()Z"),
    );
    methods.insert(
        "hasNextShort:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextShort", "()Z"),
    );
    methods.insert(
        "hasNextBigInteger:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextBigInteger", "()Z"),
    );
    methods.insert(
        "hasNextBigDecimal:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasNextBigDecimal", "()Z"),
    );

    // next variants
    methods.insert(
        "next:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("next", "()Ljava/lang/String;"),
    );
    methods.insert(
        "next:(Ljava/util/regex/Pattern;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "next",
            "(Ljava/util/regex/Pattern;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "next:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("next", "(Ljava/lang/String;)Ljava/lang/String;"),
    );
    methods.insert(
        "nextLine:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("nextLine", "()Ljava/lang/String;"),
    );
    methods.insert(
        "nextInt:()I".to_string(),
        StubMethodDescriptor::instance_method("nextInt", "()I"),
    );
    methods.insert(
        "nextInt:(I)I".to_string(),
        StubMethodDescriptor::instance_method("nextInt", "(I)I"),
    );
    methods.insert(
        "nextLong:()J".to_string(),
        StubMethodDescriptor::instance_method("nextLong", "()J"),
    );
    methods.insert(
        "nextLong:(I)J".to_string(),
        StubMethodDescriptor::instance_method("nextLong", "(I)J"),
    );
    methods.insert(
        "nextDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("nextDouble", "()D"),
    );
    methods.insert(
        "nextFloat:()F".to_string(),
        StubMethodDescriptor::instance_method("nextFloat", "()F"),
    );
    methods.insert(
        "nextBoolean:()Z".to_string(),
        StubMethodDescriptor::instance_method("nextBoolean", "()Z"),
    );
    methods.insert(
        "nextByte:()B".to_string(),
        StubMethodDescriptor::instance_method("nextByte", "()B"),
    );
    methods.insert(
        "nextShort:()S".to_string(),
        StubMethodDescriptor::instance_method("nextShort", "()S"),
    );
    methods.insert(
        "nextBigInteger:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("nextBigInteger", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "nextBigDecimal:()Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("nextBigDecimal", "()Ljava/math/BigDecimal;"),
    );

    // Misc
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "match:()Ljava/util/regex/MatchResult;".to_string(),
        StubMethodDescriptor::instance_method("match", "()Ljava/util/regex/MatchResult;"),
    );
    methods.insert(
        "skip:(Ljava/util/regex/Pattern;)Ljava/util/Scanner;".to_string(),
        StubMethodDescriptor::instance_method(
            "skip",
            "(Ljava/util/regex/Pattern;)Ljava/util/Scanner;",
        ),
    );
    methods.insert(
        "skip:(Ljava/lang/String;)Ljava/util/Scanner;".to_string(),
        StubMethodDescriptor::instance_method("skip", "(Ljava/lang/String;)Ljava/util/Scanner;"),
    );
    methods.insert(
        "findInLine:(Ljava/util/regex/Pattern;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "findInLine",
            "(Ljava/util/regex/Pattern;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "findInLine:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "findInLine",
            "(Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "findWithinHorizon:(Ljava/util/regex/Pattern;I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "findWithinHorizon",
            "(Ljava/util/regex/Pattern;I)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "ioException:()Ljava/io/IOException;".to_string(),
        StubMethodDescriptor::instance_method("ioException", "()Ljava/io/IOException;"),
    );
    methods.insert(
        "reset:()Ljava/util/Scanner;".to_string(),
        StubMethodDescriptor::instance_method("reset", "()Ljava/util/Scanner;"),
    );
    methods.insert(
        "tokens:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("tokens", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "findAll:(Ljava/util/regex/Pattern;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "findAll",
            "(Ljava/util/regex/Pattern;)Ljava/util/stream/Stream;",
        ),
    );

    StubClass {
        name: "java/util/Scanner".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/util/Iterator".to_string(),
            "java/io/Closeable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.Random class stub
pub fn create_random_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(J)V".to_string(),
        StubMethodDescriptor::constructor("(J)V"),
    );

    // Core methods
    methods.insert(
        "setSeed:(J)V".to_string(),
        StubMethodDescriptor::instance_method("setSeed", "(J)V"),
    );
    methods.insert(
        "nextBytes:([B)V".to_string(),
        StubMethodDescriptor::instance_method("nextBytes", "([B)V"),
    );
    methods.insert(
        "nextInt:()I".to_string(),
        StubMethodDescriptor::instance_method("nextInt", "()I"),
    );
    methods.insert(
        "nextInt:(I)I".to_string(),
        StubMethodDescriptor::instance_method("nextInt", "(I)I"),
    );
    methods.insert(
        "nextInt:(II)I".to_string(),
        StubMethodDescriptor::instance_method("nextInt", "(II)I"),
    );
    methods.insert(
        "nextLong:()J".to_string(),
        StubMethodDescriptor::instance_method("nextLong", "()J"),
    );
    methods.insert(
        "nextLong:(J)J".to_string(),
        StubMethodDescriptor::instance_method("nextLong", "(J)J"),
    );
    methods.insert(
        "nextLong:(JJ)J".to_string(),
        StubMethodDescriptor::instance_method("nextLong", "(JJ)J"),
    );
    methods.insert(
        "nextBoolean:()Z".to_string(),
        StubMethodDescriptor::instance_method("nextBoolean", "()Z"),
    );
    methods.insert(
        "nextFloat:()F".to_string(),
        StubMethodDescriptor::instance_method("nextFloat", "()F"),
    );
    methods.insert(
        "nextFloat:(F)F".to_string(),
        StubMethodDescriptor::instance_method("nextFloat", "(F)F"),
    );
    methods.insert(
        "nextDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("nextDouble", "()D"),
    );
    methods.insert(
        "nextDouble:(D)D".to_string(),
        StubMethodDescriptor::instance_method("nextDouble", "(D)D"),
    );
    methods.insert(
        "nextGaussian:()D".to_string(),
        StubMethodDescriptor::instance_method("nextGaussian", "()D"),
    );
    methods.insert(
        "nextGaussian:(DD)D".to_string(),
        StubMethodDescriptor::instance_method("nextGaussian", "(DD)D"),
    );

    // Stream methods (Java 8+)
    methods.insert(
        "ints:()Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("ints", "()Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "ints:(J)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("ints", "(J)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "ints:(II)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("ints", "(II)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "ints:(JII)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("ints", "(JII)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "longs:()Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("longs", "()Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "longs:(J)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("longs", "(J)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "longs:(JJ)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("longs", "(JJ)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "longs:(JJJ)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("longs", "(JJJ)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "doubles:()Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("doubles", "()Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "doubles:(J)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("doubles", "(J)Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "doubles:(DD)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("doubles", "(DD)Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "doubles:(JDD)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("doubles", "(JDD)Ljava/util/stream/DoubleStream;"),
    );

    StubClass {
        name: "java/util/Random".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.UUID class stub
pub fn create_uuid_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(JJ)V".to_string(),
        StubMethodDescriptor::constructor("(JJ)V"),
    );

    // Factory methods
    methods.insert(
        "randomUUID:()Ljava/util/UUID;".to_string(),
        StubMethodDescriptor::static_method("randomUUID", "()Ljava/util/UUID;"),
    );
    methods.insert(
        "nameUUIDFromBytes:([B)Ljava/util/UUID;".to_string(),
        StubMethodDescriptor::static_method("nameUUIDFromBytes", "([B)Ljava/util/UUID;"),
    );
    methods.insert(
        "fromString:(Ljava/lang/String;)Ljava/util/UUID;".to_string(),
        StubMethodDescriptor::static_method("fromString", "(Ljava/lang/String;)Ljava/util/UUID;"),
    );

    // Accessors
    methods.insert(
        "getMostSignificantBits:()J".to_string(),
        StubMethodDescriptor::instance_method("getMostSignificantBits", "()J"),
    );
    methods.insert(
        "getLeastSignificantBits:()J".to_string(),
        StubMethodDescriptor::instance_method("getLeastSignificantBits", "()J"),
    );
    methods.insert(
        "version:()I".to_string(),
        StubMethodDescriptor::instance_method("version", "()I"),
    );
    methods.insert(
        "variant:()I".to_string(),
        StubMethodDescriptor::instance_method("variant", "()I"),
    );
    methods.insert(
        "timestamp:()J".to_string(),
        StubMethodDescriptor::instance_method("timestamp", "()J"),
    );
    methods.insert(
        "clockSequence:()I".to_string(),
        StubMethodDescriptor::instance_method("clockSequence", "()I"),
    );
    methods.insert(
        "node:()J".to_string(),
        StubMethodDescriptor::instance_method("node", "()J"),
    );

    // Object methods
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/util/UUID;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/util/UUID;)I"),
    );

    StubClass {
        name: "java/util/UUID".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/Comparable".to_string(),
        ],
        methods,
    }
}
