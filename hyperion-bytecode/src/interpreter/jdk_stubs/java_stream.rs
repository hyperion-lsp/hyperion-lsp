// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.util.stream package stub implementations
//!
//! Stream API: Stream, IntStream, LongStream, DoubleStream, Collectors, etc.

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

/// Create java.util.stream.Stream interface stub
pub fn create_stream_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Intermediate operations
    methods.insert(
        "filter:(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "filter",
            "(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "map:(Ljava/util/function/Function;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "map",
            "(Ljava/util/function/Function;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "mapToInt:(Ljava/util/function/ToIntFunction;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "mapToInt",
            "(Ljava/util/function/ToIntFunction;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "mapToLong:(Ljava/util/function/ToLongFunction;)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "mapToLong",
            "(Ljava/util/function/ToLongFunction;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "mapToDouble:(Ljava/util/function/ToDoubleFunction;)Ljava/util/stream/DoubleStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "mapToDouble",
            "(Ljava/util/function/ToDoubleFunction;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "flatMap:(Ljava/util/function/Function;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "flatMap",
            "(Ljava/util/function/Function;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "flatMapToInt:(Ljava/util/function/Function;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "flatMapToInt",
            "(Ljava/util/function/Function;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "flatMapToLong:(Ljava/util/function/Function;)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "flatMapToLong",
            "(Ljava/util/function/Function;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "flatMapToDouble:(Ljava/util/function/Function;)Ljava/util/stream/DoubleStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "flatMapToDouble",
            "(Ljava/util/function/Function;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "distinct:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("distinct", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "sorted:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("sorted", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "sorted:(Ljava/util/Comparator;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "sorted",
            "(Ljava/util/Comparator;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "peek:(Ljava/util/function/Consumer;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "peek",
            "(Ljava/util/function/Consumer;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "limit:(J)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("limit", "(J)Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "skip:(J)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "takeWhile:(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "takeWhile",
            "(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "dropWhile:(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "dropWhile",
            "(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;",
        ),
    );

    // Terminal operations
    methods.insert(
        "forEach:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/Consumer;)V"),
    );
    methods.insert(
        "forEachOrdered:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEachOrdered", "(Ljava/util/function/Consumer;)V"),
    );
    methods.insert(
        "toArray:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "toArray:(Ljava/util/function/IntFunction;)[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "toArray",
            "(Ljava/util/function/IntFunction;)[Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "reduce:(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "reduce:(Ljava/util/function/BinaryOperator;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(Ljava/util/function/BinaryOperator;)Ljava/util/Optional;",
        ),
    );
    methods.insert(
        "reduce:(Ljava/lang/Object;Ljava/util/function/BiFunction;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(Ljava/lang/Object;Ljava/util/function/BiFunction;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "collect:(Ljava/util/stream/Collector;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "collect",
            "(Ljava/util/stream/Collector;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "collect:(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BiConsumer;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "collect",
            "(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BiConsumer;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "min:(Ljava/util/Comparator;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method(
            "min",
            "(Ljava/util/Comparator;)Ljava/util/Optional;",
        ),
    );
    methods.insert(
        "max:(Ljava/util/Comparator;)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method(
            "max",
            "(Ljava/util/Comparator;)Ljava/util/Optional;",
        ),
    );
    methods.insert(
        "count:()J".to_string(),
        StubMethodDescriptor::instance_method("count", "()J"),
    );
    methods.insert(
        "anyMatch:(Ljava/util/function/Predicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("anyMatch", "(Ljava/util/function/Predicate;)Z"),
    );
    methods.insert(
        "allMatch:(Ljava/util/function/Predicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("allMatch", "(Ljava/util/function/Predicate;)Z"),
    );
    methods.insert(
        "noneMatch:(Ljava/util/function/Predicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("noneMatch", "(Ljava/util/function/Predicate;)Z"),
    );
    methods.insert(
        "findFirst:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("findFirst", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "findAny:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("findAny", "()Ljava/util/Optional;"),
    );

    // Static factory methods
    methods.insert(
        "empty:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method("empty", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "of:(Ljava/lang/Object;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method("of", "(Ljava/lang/Object;)Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "of:([Ljava/lang/Object;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method("of", "([Ljava/lang/Object;)Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "ofNullable:(Ljava/lang/Object;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "ofNullable",
            "(Ljava/lang/Object;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "iterate:(Ljava/lang/Object;Ljava/util/function/UnaryOperator;)Ljava/util/stream/Stream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "iterate",
            "(Ljava/lang/Object;Ljava/util/function/UnaryOperator;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "iterate:(Ljava/lang/Object;Ljava/util/function/Predicate;Ljava/util/function/UnaryOperator;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "iterate",
            "(Ljava/lang/Object;Ljava/util/function/Predicate;Ljava/util/function/UnaryOperator;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "generate:(Ljava/util/function/Supplier;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "generate",
            "(Ljava/util/function/Supplier;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "concat:(Ljava/util/stream/Stream;Ljava/util/stream/Stream;)Ljava/util/stream/Stream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "concat",
            "(Ljava/util/stream/Stream;Ljava/util/stream/Stream;)Ljava/util/stream/Stream;",
        ),
    );

    // BaseStream methods
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "spliterator:()Ljava/util/Spliterator;".to_string(),
        StubMethodDescriptor::instance_method("spliterator", "()Ljava/util/Spliterator;"),
    );
    methods.insert(
        "isParallel:()Z".to_string(),
        StubMethodDescriptor::instance_method("isParallel", "()Z"),
    );
    methods.insert(
        "sequential:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("sequential", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "parallel:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("parallel", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "unordered:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("unordered", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "onClose:(Ljava/lang/Runnable;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "onClose",
            "(Ljava/lang/Runnable;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/util/stream/Stream".to_string(),
        superclass: None,
        interfaces: vec!["java/util/stream/BaseStream".to_string()],
        methods,
    }
}

/// Create java.util.stream.IntStream interface stub
pub fn create_int_stream_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Intermediate operations
    methods.insert(
        "filter:(Ljava/util/function/IntPredicate;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "filter",
            "(Ljava/util/function/IntPredicate;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "map:(Ljava/util/function/IntUnaryOperator;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "map",
            "(Ljava/util/function/IntUnaryOperator;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "mapToObj:(Ljava/util/function/IntFunction;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "mapToObj",
            "(Ljava/util/function/IntFunction;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "mapToLong:(Ljava/util/function/IntToLongFunction;)Ljava/util/stream/LongStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "mapToLong",
            "(Ljava/util/function/IntToLongFunction;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "mapToDouble:(Ljava/util/function/IntToDoubleFunction;)Ljava/util/stream/DoubleStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "mapToDouble",
            "(Ljava/util/function/IntToDoubleFunction;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "flatMap:(Ljava/util/function/IntFunction;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "flatMap",
            "(Ljava/util/function/IntFunction;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "distinct:()Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("distinct", "()Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "sorted:()Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("sorted", "()Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "peek:(Ljava/util/function/IntConsumer;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "peek",
            "(Ljava/util/function/IntConsumer;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "limit:(J)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("limit", "(J)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "skip:(J)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "takeWhile:(Ljava/util/function/IntPredicate;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "takeWhile",
            "(Ljava/util/function/IntPredicate;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "dropWhile:(Ljava/util/function/IntPredicate;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "dropWhile",
            "(Ljava/util/function/IntPredicate;)Ljava/util/stream/IntStream;",
        ),
    );

    // Terminal operations
    methods.insert(
        "forEach:(Ljava/util/function/IntConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/IntConsumer;)V"),
    );
    methods.insert(
        "forEachOrdered:(Ljava/util/function/IntConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachOrdered",
            "(Ljava/util/function/IntConsumer;)V",
        ),
    );
    methods.insert(
        "toArray:()[I".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[I"),
    );
    methods.insert(
        "reduce:(ILjava/util/function/IntBinaryOperator;)I".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(ILjava/util/function/IntBinaryOperator;)I",
        ),
    );
    methods.insert(
        "reduce:(Ljava/util/function/IntBinaryOperator;)Ljava/util/OptionalInt;".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(Ljava/util/function/IntBinaryOperator;)Ljava/util/OptionalInt;",
        ),
    );
    methods.insert(
        "collect:(Ljava/util/function/Supplier;Ljava/util/function/ObjIntConsumer;Ljava/util/function/BiConsumer;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "collect",
            "(Ljava/util/function/Supplier;Ljava/util/function/ObjIntConsumer;Ljava/util/function/BiConsumer;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "sum:()I".to_string(),
        StubMethodDescriptor::instance_method("sum", "()I"),
    );
    methods.insert(
        "min:()Ljava/util/OptionalInt;".to_string(),
        StubMethodDescriptor::instance_method("min", "()Ljava/util/OptionalInt;"),
    );
    methods.insert(
        "max:()Ljava/util/OptionalInt;".to_string(),
        StubMethodDescriptor::instance_method("max", "()Ljava/util/OptionalInt;"),
    );
    methods.insert(
        "count:()J".to_string(),
        StubMethodDescriptor::instance_method("count", "()J"),
    );
    methods.insert(
        "average:()Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method("average", "()Ljava/util/OptionalDouble;"),
    );
    methods.insert(
        "summaryStatistics:()Ljava/util/IntSummaryStatistics;".to_string(),
        StubMethodDescriptor::instance_method(
            "summaryStatistics",
            "()Ljava/util/IntSummaryStatistics;",
        ),
    );
    methods.insert(
        "anyMatch:(Ljava/util/function/IntPredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("anyMatch", "(Ljava/util/function/IntPredicate;)Z"),
    );
    methods.insert(
        "allMatch:(Ljava/util/function/IntPredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("allMatch", "(Ljava/util/function/IntPredicate;)Z"),
    );
    methods.insert(
        "noneMatch:(Ljava/util/function/IntPredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("noneMatch", "(Ljava/util/function/IntPredicate;)Z"),
    );
    methods.insert(
        "findFirst:()Ljava/util/OptionalInt;".to_string(),
        StubMethodDescriptor::instance_method("findFirst", "()Ljava/util/OptionalInt;"),
    );
    methods.insert(
        "findAny:()Ljava/util/OptionalInt;".to_string(),
        StubMethodDescriptor::instance_method("findAny", "()Ljava/util/OptionalInt;"),
    );
    methods.insert(
        "boxed:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("boxed", "()Ljava/util/stream/Stream;"),
    );

    // Static factory methods
    methods.insert(
        "empty:()Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method("empty", "()Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "of:(I)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method("of", "(I)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "of:([I)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method("of", "([I)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "range:(II)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method("range", "(II)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "rangeClosed:(II)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method("rangeClosed", "(II)Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "iterate:(ILjava/util/function/IntUnaryOperator;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method(
            "iterate",
            "(ILjava/util/function/IntUnaryOperator;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "iterate:(ILjava/util/function/IntPredicate;Ljava/util/function/IntUnaryOperator;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method(
            "iterate",
            "(ILjava/util/function/IntPredicate;Ljava/util/function/IntUnaryOperator;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "generate:(Ljava/util/function/IntSupplier;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method(
            "generate",
            "(Ljava/util/function/IntSupplier;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "concat:(Ljava/util/stream/IntStream;Ljava/util/stream/IntStream;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method(
            "concat",
            "(Ljava/util/stream/IntStream;Ljava/util/stream/IntStream;)Ljava/util/stream/IntStream;",
        ),
    );

    // BaseStream methods
    methods.insert(
        "iterator:()Ljava/util/PrimitiveIterator$OfInt;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/PrimitiveIterator$OfInt;"),
    );
    methods.insert(
        "spliterator:()Ljava/util/Spliterator$OfInt;".to_string(),
        StubMethodDescriptor::instance_method("spliterator", "()Ljava/util/Spliterator$OfInt;"),
    );
    methods.insert(
        "isParallel:()Z".to_string(),
        StubMethodDescriptor::instance_method("isParallel", "()Z"),
    );
    methods.insert(
        "sequential:()Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("sequential", "()Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "parallel:()Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("parallel", "()Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "unordered:()Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method("unordered", "()Ljava/util/stream/IntStream;"),
    );
    methods.insert(
        "onClose:(Ljava/lang/Runnable;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "onClose",
            "(Ljava/lang/Runnable;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/util/stream/IntStream".to_string(),
        superclass: None,
        interfaces: vec!["java/util/stream/BaseStream".to_string()],
        methods,
    }
}

/// Create java.util.stream.LongStream interface stub
pub fn create_long_stream_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Intermediate operations
    methods.insert(
        "filter:(Ljava/util/function/LongPredicate;)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "filter",
            "(Ljava/util/function/LongPredicate;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "map:(Ljava/util/function/LongUnaryOperator;)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "map",
            "(Ljava/util/function/LongUnaryOperator;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "mapToObj:(Ljava/util/function/LongFunction;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "mapToObj",
            "(Ljava/util/function/LongFunction;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "mapToInt:(Ljava/util/function/LongToIntFunction;)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "mapToInt",
            "(Ljava/util/function/LongToIntFunction;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "mapToDouble:(Ljava/util/function/LongToDoubleFunction;)Ljava/util/stream/DoubleStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "mapToDouble",
            "(Ljava/util/function/LongToDoubleFunction;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "flatMap:(Ljava/util/function/LongFunction;)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "flatMap",
            "(Ljava/util/function/LongFunction;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "distinct:()Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("distinct", "()Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "sorted:()Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("sorted", "()Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "peek:(Ljava/util/function/LongConsumer;)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "peek",
            "(Ljava/util/function/LongConsumer;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "limit:(J)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("limit", "(J)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "skip:(J)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)Ljava/util/stream/LongStream;"),
    );

    // Terminal operations
    methods.insert(
        "forEach:(Ljava/util/function/LongConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/LongConsumer;)V"),
    );
    methods.insert(
        "toArray:()[J".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[J"),
    );
    methods.insert(
        "reduce:(JLjava/util/function/LongBinaryOperator;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(JLjava/util/function/LongBinaryOperator;)J",
        ),
    );
    methods.insert(
        "reduce:(Ljava/util/function/LongBinaryOperator;)Ljava/util/OptionalLong;".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(Ljava/util/function/LongBinaryOperator;)Ljava/util/OptionalLong;",
        ),
    );
    methods.insert(
        "sum:()J".to_string(),
        StubMethodDescriptor::instance_method("sum", "()J"),
    );
    methods.insert(
        "min:()Ljava/util/OptionalLong;".to_string(),
        StubMethodDescriptor::instance_method("min", "()Ljava/util/OptionalLong;"),
    );
    methods.insert(
        "max:()Ljava/util/OptionalLong;".to_string(),
        StubMethodDescriptor::instance_method("max", "()Ljava/util/OptionalLong;"),
    );
    methods.insert(
        "count:()J".to_string(),
        StubMethodDescriptor::instance_method("count", "()J"),
    );
    methods.insert(
        "average:()Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method("average", "()Ljava/util/OptionalDouble;"),
    );
    methods.insert(
        "anyMatch:(Ljava/util/function/LongPredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("anyMatch", "(Ljava/util/function/LongPredicate;)Z"),
    );
    methods.insert(
        "allMatch:(Ljava/util/function/LongPredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("allMatch", "(Ljava/util/function/LongPredicate;)Z"),
    );
    methods.insert(
        "noneMatch:(Ljava/util/function/LongPredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method("noneMatch", "(Ljava/util/function/LongPredicate;)Z"),
    );
    methods.insert(
        "findFirst:()Ljava/util/OptionalLong;".to_string(),
        StubMethodDescriptor::instance_method("findFirst", "()Ljava/util/OptionalLong;"),
    );
    methods.insert(
        "findAny:()Ljava/util/OptionalLong;".to_string(),
        StubMethodDescriptor::instance_method("findAny", "()Ljava/util/OptionalLong;"),
    );
    methods.insert(
        "boxed:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("boxed", "()Ljava/util/stream/Stream;"),
    );

    // Static factory methods
    methods.insert(
        "empty:()Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method("empty", "()Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "of:(J)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method("of", "(J)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "of:([J)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method("of", "([J)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "range:(JJ)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method("range", "(JJ)Ljava/util/stream/LongStream;"),
    );
    methods.insert(
        "rangeClosed:(JJ)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method("rangeClosed", "(JJ)Ljava/util/stream/LongStream;"),
    );

    StubClass {
        name: "java/util/stream/LongStream".to_string(),
        superclass: None,
        interfaces: vec!["java/util/stream/BaseStream".to_string()],
        methods,
    }
}

/// Create java.util.stream.DoubleStream interface stub
pub fn create_double_stream_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Intermediate operations
    methods.insert(
        "filter:(Ljava/util/function/DoublePredicate;)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "filter",
            "(Ljava/util/function/DoublePredicate;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "map:(Ljava/util/function/DoubleUnaryOperator;)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "map",
            "(Ljava/util/function/DoubleUnaryOperator;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "mapToObj:(Ljava/util/function/DoubleFunction;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "mapToObj",
            "(Ljava/util/function/DoubleFunction;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "mapToInt:(Ljava/util/function/DoubleToIntFunction;)Ljava/util/stream/IntStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "mapToInt",
            "(Ljava/util/function/DoubleToIntFunction;)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "mapToLong:(Ljava/util/function/DoubleToLongFunction;)Ljava/util/stream/LongStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "mapToLong",
            "(Ljava/util/function/DoubleToLongFunction;)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "flatMap:(Ljava/util/function/DoubleFunction;)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "flatMap",
            "(Ljava/util/function/DoubleFunction;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "distinct:()Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("distinct", "()Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "sorted:()Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("sorted", "()Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "peek:(Ljava/util/function/DoubleConsumer;)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "peek",
            "(Ljava/util/function/DoubleConsumer;)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "limit:(J)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("limit", "(J)Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "skip:(J)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)Ljava/util/stream/DoubleStream;"),
    );

    // Terminal operations
    methods.insert(
        "forEach:(Ljava/util/function/DoubleConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/DoubleConsumer;)V"),
    );
    methods.insert(
        "toArray:()[D".to_string(),
        StubMethodDescriptor::instance_method("toArray", "()[D"),
    );
    methods.insert(
        "reduce:(DLjava/util/function/DoubleBinaryOperator;)D".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(DLjava/util/function/DoubleBinaryOperator;)D",
        ),
    );
    methods.insert(
        "reduce:(Ljava/util/function/DoubleBinaryOperator;)Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method(
            "reduce",
            "(Ljava/util/function/DoubleBinaryOperator;)Ljava/util/OptionalDouble;",
        ),
    );
    methods.insert(
        "sum:()D".to_string(),
        StubMethodDescriptor::instance_method("sum", "()D"),
    );
    methods.insert(
        "min:()Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method("min", "()Ljava/util/OptionalDouble;"),
    );
    methods.insert(
        "max:()Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method("max", "()Ljava/util/OptionalDouble;"),
    );
    methods.insert(
        "count:()J".to_string(),
        StubMethodDescriptor::instance_method("count", "()J"),
    );
    methods.insert(
        "average:()Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method("average", "()Ljava/util/OptionalDouble;"),
    );
    methods.insert(
        "anyMatch:(Ljava/util/function/DoublePredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "anyMatch",
            "(Ljava/util/function/DoublePredicate;)Z",
        ),
    );
    methods.insert(
        "allMatch:(Ljava/util/function/DoublePredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "allMatch",
            "(Ljava/util/function/DoublePredicate;)Z",
        ),
    );
    methods.insert(
        "noneMatch:(Ljava/util/function/DoublePredicate;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "noneMatch",
            "(Ljava/util/function/DoublePredicate;)Z",
        ),
    );
    methods.insert(
        "findFirst:()Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method("findFirst", "()Ljava/util/OptionalDouble;"),
    );
    methods.insert(
        "findAny:()Ljava/util/OptionalDouble;".to_string(),
        StubMethodDescriptor::instance_method("findAny", "()Ljava/util/OptionalDouble;"),
    );
    methods.insert(
        "boxed:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("boxed", "()Ljava/util/stream/Stream;"),
    );

    // Static factory methods
    methods.insert(
        "empty:()Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::static_method("empty", "()Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "of:(D)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::static_method("of", "(D)Ljava/util/stream/DoubleStream;"),
    );
    methods.insert(
        "of:([D)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::static_method("of", "([D)Ljava/util/stream/DoubleStream;"),
    );

    StubClass {
        name: "java/util/stream/DoubleStream".to_string(),
        superclass: None,
        interfaces: vec!["java/util/stream/BaseStream".to_string()],
        methods,
    }
}

/// Create java.util.stream.BaseStream interface stub
pub fn create_base_stream_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "spliterator:()Ljava/util/Spliterator;".to_string(),
        StubMethodDescriptor::instance_method("spliterator", "()Ljava/util/Spliterator;"),
    );
    methods.insert(
        "isParallel:()Z".to_string(),
        StubMethodDescriptor::instance_method("isParallel", "()Z"),
    );
    methods.insert(
        "sequential:()Ljava/util/stream/BaseStream;".to_string(),
        StubMethodDescriptor::instance_method("sequential", "()Ljava/util/stream/BaseStream;"),
    );
    methods.insert(
        "parallel:()Ljava/util/stream/BaseStream;".to_string(),
        StubMethodDescriptor::instance_method("parallel", "()Ljava/util/stream/BaseStream;"),
    );
    methods.insert(
        "unordered:()Ljava/util/stream/BaseStream;".to_string(),
        StubMethodDescriptor::instance_method("unordered", "()Ljava/util/stream/BaseStream;"),
    );
    methods.insert(
        "onClose:(Ljava/lang/Runnable;)Ljava/util/stream/BaseStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "onClose",
            "(Ljava/lang/Runnable;)Ljava/util/stream/BaseStream;",
        ),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/util/stream/BaseStream".to_string(),
        superclass: None,
        interfaces: vec!["java/lang/AutoCloseable".to_string()],
        methods,
    }
}

/// Create java.util.stream.Collectors utility class stub
pub fn create_collectors_class() -> StubClass {
    let mut methods = HashMap::new();

    // toList, toSet, toMap
    methods.insert(
        "toList:()Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method("toList", "()Ljava/util/stream/Collector;"),
    );
    methods.insert(
        "toSet:()Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method("toSet", "()Ljava/util/stream/Collector;"),
    );
    methods.insert(
        "toCollection:(Ljava/util/function/Supplier;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "toCollection",
            "(Ljava/util/function/Supplier;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "toMap:(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "toMap",
            "(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "toMap:(Ljava/util/function/Function;Ljava/util/function/Function;Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "toMap",
            "(Ljava/util/function/Function;Ljava/util/function/Function;Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "toMap:(Ljava/util/function/Function;Ljava/util/function/Function;Ljava/util/function/BinaryOperator;Ljava/util/function/Supplier;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "toMap",
            "(Ljava/util/function/Function;Ljava/util/function/Function;Ljava/util/function/BinaryOperator;Ljava/util/function/Supplier;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "toConcurrentMap:(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "toConcurrentMap",
            "(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;",
        ),
    );

    // joining
    methods.insert(
        "joining:()Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method("joining", "()Ljava/util/stream/Collector;"),
    );
    methods.insert(
        "joining:(Ljava/lang/CharSequence;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "joining",
            "(Ljava/lang/CharSequence;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "joining:(Ljava/lang/CharSequence;Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "joining",
            "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/util/stream/Collector;",
        ),
    );

    // groupingBy
    methods.insert(
        "groupingBy:(Ljava/util/function/Function;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "groupingBy",
            "(Ljava/util/function/Function;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "groupingBy:(Ljava/util/function/Function;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "groupingBy",
            "(Ljava/util/function/Function;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "groupingBy:(Ljava/util/function/Function;Ljava/util/function/Supplier;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "groupingBy",
            "(Ljava/util/function/Function;Ljava/util/function/Supplier;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "groupingByConcurrent:(Ljava/util/function/Function;)Ljava/util/stream/Collector;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "groupingByConcurrent",
            "(Ljava/util/function/Function;)Ljava/util/stream/Collector;",
        ),
    );

    // partitioningBy
    methods.insert(
        "partitioningBy:(Ljava/util/function/Predicate;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "partitioningBy",
            "(Ljava/util/function/Predicate;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "partitioningBy:(Ljava/util/function/Predicate;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "partitioningBy",
            "(Ljava/util/function/Predicate;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;",
        ),
    );

    // counting
    methods.insert(
        "counting:()Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method("counting", "()Ljava/util/stream/Collector;"),
    );

    // summing
    methods.insert(
        "summingInt:(Ljava/util/function/ToIntFunction;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "summingInt",
            "(Ljava/util/function/ToIntFunction;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "summingLong:(Ljava/util/function/ToLongFunction;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "summingLong",
            "(Ljava/util/function/ToLongFunction;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "summingDouble:(Ljava/util/function/ToDoubleFunction;)Ljava/util/stream/Collector;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "summingDouble",
            "(Ljava/util/function/ToDoubleFunction;)Ljava/util/stream/Collector;",
        ),
    );

    // averaging
    methods.insert(
        "averagingInt:(Ljava/util/function/ToIntFunction;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "averagingInt",
            "(Ljava/util/function/ToIntFunction;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "averagingLong:(Ljava/util/function/ToLongFunction;)Ljava/util/stream/Collector;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "averagingLong",
            "(Ljava/util/function/ToLongFunction;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "averagingDouble:(Ljava/util/function/ToDoubleFunction;)Ljava/util/stream/Collector;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "averagingDouble",
            "(Ljava/util/function/ToDoubleFunction;)Ljava/util/stream/Collector;",
        ),
    );

    // reducing
    methods.insert(
        "reducing:(Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "reducing",
            "(Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "reducing:(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "reducing",
            "(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "reducing:(Ljava/lang/Object;Ljava/util/function/Function;Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "reducing",
            "(Ljava/lang/Object;Ljava/util/function/Function;Ljava/util/function/BinaryOperator;)Ljava/util/stream/Collector;",
        ),
    );

    // mapping/flatMapping
    methods.insert(
        "mapping:(Ljava/util/function/Function;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "mapping",
            "(Ljava/util/function/Function;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "flatMapping:(Ljava/util/function/Function;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "flatMapping",
            "(Ljava/util/function/Function;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "filtering:(Ljava/util/function/Predicate;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "filtering",
            "(Ljava/util/function/Predicate;Ljava/util/stream/Collector;)Ljava/util/stream/Collector;",
        ),
    );

    // collectingAndThen
    methods.insert(
        "collectingAndThen:(Ljava/util/stream/Collector;Ljava/util/function/Function;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "collectingAndThen",
            "(Ljava/util/stream/Collector;Ljava/util/function/Function;)Ljava/util/stream/Collector;",
        ),
    );

    // maxBy/minBy
    methods.insert(
        "maxBy:(Ljava/util/Comparator;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "maxBy",
            "(Ljava/util/Comparator;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "minBy:(Ljava/util/Comparator;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "minBy",
            "(Ljava/util/Comparator;)Ljava/util/stream/Collector;",
        ),
    );

    // toUnmodifiable
    methods.insert(
        "toUnmodifiableList:()Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method("toUnmodifiableList", "()Ljava/util/stream/Collector;"),
    );
    methods.insert(
        "toUnmodifiableSet:()Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method("toUnmodifiableSet", "()Ljava/util/stream/Collector;"),
    );
    methods.insert(
        "toUnmodifiableMap:(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "toUnmodifiableMap",
            "(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;",
        ),
    );

    StubClass {
        name: "java/util/stream/Collectors".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.stream.Collector interface stub
pub fn create_collector_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "supplier:()Ljava/util/function/Supplier;".to_string(),
        StubMethodDescriptor::instance_method("supplier", "()Ljava/util/function/Supplier;"),
    );
    methods.insert(
        "accumulator:()Ljava/util/function/BiConsumer;".to_string(),
        StubMethodDescriptor::instance_method("accumulator", "()Ljava/util/function/BiConsumer;"),
    );
    methods.insert(
        "combiner:()Ljava/util/function/BinaryOperator;".to_string(),
        StubMethodDescriptor::instance_method("combiner", "()Ljava/util/function/BinaryOperator;"),
    );
    methods.insert(
        "finisher:()Ljava/util/function/Function;".to_string(),
        StubMethodDescriptor::instance_method("finisher", "()Ljava/util/function/Function;"),
    );
    methods.insert(
        "characteristics:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("characteristics", "()Ljava/util/Set;"),
    );

    // Static factory methods
    methods.insert(
        "of:(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BinaryOperator;[Ljava/util/stream/Collector$Characteristics;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "of",
            "(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BinaryOperator;[Ljava/util/stream/Collector$Characteristics;)Ljava/util/stream/Collector;",
        ),
    );
    methods.insert(
        "of:(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BinaryOperator;Ljava/util/function/Function;[Ljava/util/stream/Collector$Characteristics;)Ljava/util/stream/Collector;".to_string(),
        StubMethodDescriptor::static_method(
            "of",
            "(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BinaryOperator;Ljava/util/function/Function;[Ljava/util/stream/Collector$Characteristics;)Ljava/util/stream/Collector;",
        ),
    );

    StubClass {
        name: "java/util/stream/Collector".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Spliterator interface stub
pub fn create_spliterator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "tryAdvance:(Ljava/util/function/Consumer;)Z".to_string(),
        StubMethodDescriptor::instance_method("tryAdvance", "(Ljava/util/function/Consumer;)Z"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/Consumer;)V",
        ),
    );
    methods.insert(
        "trySplit:()Ljava/util/Spliterator;".to_string(),
        StubMethodDescriptor::instance_method("trySplit", "()Ljava/util/Spliterator;"),
    );
    methods.insert(
        "estimateSize:()J".to_string(),
        StubMethodDescriptor::instance_method("estimateSize", "()J"),
    );
    methods.insert(
        "getExactSizeIfKnown:()J".to_string(),
        StubMethodDescriptor::instance_method("getExactSizeIfKnown", "()J"),
    );
    methods.insert(
        "characteristics:()I".to_string(),
        StubMethodDescriptor::instance_method("characteristics", "()I"),
    );
    methods.insert(
        "hasCharacteristics:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("hasCharacteristics", "(I)Z"),
    );
    methods.insert(
        "getComparator:()Ljava/util/Comparator;".to_string(),
        StubMethodDescriptor::instance_method("getComparator", "()Ljava/util/Comparator;"),
    );

    StubClass {
        name: "java/util/Spliterator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.Spliterator$OfPrimitive interface stub
pub fn create_spliterator_of_primitive_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "trySplit:()Ljava/util/Spliterator$OfPrimitive;".to_string(),
        StubMethodDescriptor::instance_method("trySplit", "()Ljava/util/Spliterator$OfPrimitive;"),
    );
    methods.insert(
        "tryAdvance:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("tryAdvance", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("forEachRemaining", "(Ljava/lang/Object;)V"),
    );

    StubClass {
        name: "java/util/Spliterator$OfPrimitive".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Spliterator".to_string()],
        methods,
    }
}

/// Create java.util.Spliterator$OfInt interface stub
pub fn create_spliterator_of_int_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "trySplit:()Ljava/util/Spliterator$OfInt;".to_string(),
        StubMethodDescriptor::instance_method("trySplit", "()Ljava/util/Spliterator$OfInt;"),
    );
    methods.insert(
        "tryAdvance:(Ljava/util/function/IntConsumer;)Z".to_string(),
        StubMethodDescriptor::instance_method("tryAdvance", "(Ljava/util/function/IntConsumer;)Z"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/IntConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/IntConsumer;)V",
        ),
    );

    StubClass {
        name: "java/util/Spliterator$OfInt".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Spliterator$OfPrimitive".to_string()],
        methods,
    }
}

/// Create java.util.Spliterator$OfLong interface stub
pub fn create_spliterator_of_long_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "trySplit:()Ljava/util/Spliterator$OfLong;".to_string(),
        StubMethodDescriptor::instance_method("trySplit", "()Ljava/util/Spliterator$OfLong;"),
    );
    methods.insert(
        "tryAdvance:(Ljava/util/function/LongConsumer;)Z".to_string(),
        StubMethodDescriptor::instance_method("tryAdvance", "(Ljava/util/function/LongConsumer;)Z"),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/LongConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/LongConsumer;)V",
        ),
    );

    StubClass {
        name: "java/util/Spliterator$OfLong".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Spliterator$OfPrimitive".to_string()],
        methods,
    }
}

/// Create java.util.Spliterator$OfDouble interface stub
pub fn create_spliterator_of_double_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "trySplit:()Ljava/util/Spliterator$OfDouble;".to_string(),
        StubMethodDescriptor::instance_method("trySplit", "()Ljava/util/Spliterator$OfDouble;"),
    );
    methods.insert(
        "tryAdvance:(Ljava/util/function/DoubleConsumer;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "tryAdvance",
            "(Ljava/util/function/DoubleConsumer;)Z",
        ),
    );
    methods.insert(
        "forEachRemaining:(Ljava/util/function/DoubleConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "forEachRemaining",
            "(Ljava/util/function/DoubleConsumer;)V",
        ),
    );

    StubClass {
        name: "java/util/Spliterator$OfDouble".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Spliterator$OfPrimitive".to_string()],
        methods,
    }
}

/// Create java.util.stream.StreamSupport utility class stub
pub fn create_stream_support_class() -> StubClass {
    let mut methods = HashMap::new();

    // Stream factory methods
    methods.insert(
        "stream:(Ljava/util/Spliterator;Z)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "stream",
            "(Ljava/util/Spliterator;Z)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "stream:(Ljava/util/function/Supplier;IZ)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "stream",
            "(Ljava/util/function/Supplier;IZ)Ljava/util/stream/Stream;",
        ),
    );

    // IntStream factory methods
    methods.insert(
        "intStream:(Ljava/util/Spliterator$OfInt;Z)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method(
            "intStream",
            "(Ljava/util/Spliterator$OfInt;Z)Ljava/util/stream/IntStream;",
        ),
    );
    methods.insert(
        "intStream:(Ljava/util/function/Supplier;IZ)Ljava/util/stream/IntStream;".to_string(),
        StubMethodDescriptor::static_method(
            "intStream",
            "(Ljava/util/function/Supplier;IZ)Ljava/util/stream/IntStream;",
        ),
    );

    // LongStream factory methods
    methods.insert(
        "longStream:(Ljava/util/Spliterator$OfLong;Z)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method(
            "longStream",
            "(Ljava/util/Spliterator$OfLong;Z)Ljava/util/stream/LongStream;",
        ),
    );
    methods.insert(
        "longStream:(Ljava/util/function/Supplier;IZ)Ljava/util/stream/LongStream;".to_string(),
        StubMethodDescriptor::static_method(
            "longStream",
            "(Ljava/util/function/Supplier;IZ)Ljava/util/stream/LongStream;",
        ),
    );

    // DoubleStream factory methods
    methods.insert(
        "doubleStream:(Ljava/util/Spliterator$OfDouble;Z)Ljava/util/stream/DoubleStream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "doubleStream",
            "(Ljava/util/Spliterator$OfDouble;Z)Ljava/util/stream/DoubleStream;",
        ),
    );
    methods.insert(
        "doubleStream:(Ljava/util/function/Supplier;IZ)Ljava/util/stream/DoubleStream;".to_string(),
        StubMethodDescriptor::static_method(
            "doubleStream",
            "(Ljava/util/function/Supplier;IZ)Ljava/util/stream/DoubleStream;",
        ),
    );

    StubClass {
        name: "java/util/stream/StreamSupport".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}
