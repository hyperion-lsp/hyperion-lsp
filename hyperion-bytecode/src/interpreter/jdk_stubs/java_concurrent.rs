// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.util.concurrent package stub implementations
//!
//! Concurrency primitives: ExecutorService, Future, CompletableFuture, locks, etc.

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

/// Create java.util.concurrent.ExecutorService interface stub
pub fn create_executor_service_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Execution methods
    methods.insert(
        "execute:(Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::instance_method("execute", "(Ljava/lang/Runnable;)V"),
    );
    methods.insert(
        "submit:(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;",
        ),
    );
    methods.insert(
        "submit:(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;",
        ),
    );
    methods.insert(
        "submit:(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Future;",
        ),
    );

    // Shutdown methods
    methods.insert(
        "shutdown:()V".to_string(),
        StubMethodDescriptor::instance_method("shutdown", "()V"),
    );
    methods.insert(
        "shutdownNow:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("shutdownNow", "()Ljava/util/List;"),
    );
    methods.insert(
        "isShutdown:()Z".to_string(),
        StubMethodDescriptor::instance_method("isShutdown", "()Z"),
    );
    methods.insert(
        "isTerminated:()Z".to_string(),
        StubMethodDescriptor::instance_method("isTerminated", "()Z"),
    );
    methods.insert(
        "awaitTermination:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "awaitTermination",
            "(JLjava/util/concurrent/TimeUnit;)Z",
        ),
    );

    // Bulk operations
    methods.insert(
        "invokeAll:(Ljava/util/Collection;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAll",
            "(Ljava/util/Collection;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "invokeAll:(Ljava/util/Collection;JLjava/util/concurrent/TimeUnit;)Ljava/util/List;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAll",
            "(Ljava/util/Collection;JLjava/util/concurrent/TimeUnit;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "invokeAny:(Ljava/util/Collection;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAny",
            "(Ljava/util/Collection;)Ljava/lang/Object;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/ExecutorService".to_string(),
        superclass: None,
        interfaces: vec!["java/util/concurrent/Executor".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.Executor interface stub
pub fn create_executor_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "execute:(Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::instance_method("execute", "(Ljava/lang/Runnable;)V"),
    );

    StubClass {
        name: "java/util/concurrent/Executor".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.Executors factory class stub
pub fn create_executors_class() -> StubClass {
    let mut methods = HashMap::new();

    // Thread pool factories
    methods.insert(
        "newFixedThreadPool:(I)Ljava/util/concurrent/ExecutorService;".to_string(),
        StubMethodDescriptor::static_method(
            "newFixedThreadPool",
            "(I)Ljava/util/concurrent/ExecutorService;",
        ),
    );
    methods.insert(
        "newFixedThreadPool:(ILjava/util/concurrent/ThreadFactory;)Ljava/util/concurrent/ExecutorService;".to_string(),
        StubMethodDescriptor::static_method(
            "newFixedThreadPool",
            "(ILjava/util/concurrent/ThreadFactory;)Ljava/util/concurrent/ExecutorService;",
        ),
    );
    methods.insert(
        "newSingleThreadExecutor:()Ljava/util/concurrent/ExecutorService;".to_string(),
        StubMethodDescriptor::static_method(
            "newSingleThreadExecutor",
            "()Ljava/util/concurrent/ExecutorService;",
        ),
    );
    methods.insert(
        "newCachedThreadPool:()Ljava/util/concurrent/ExecutorService;".to_string(),
        StubMethodDescriptor::static_method(
            "newCachedThreadPool",
            "()Ljava/util/concurrent/ExecutorService;",
        ),
    );
    methods.insert(
        "newScheduledThreadPool:(I)Ljava/util/concurrent/ScheduledExecutorService;".to_string(),
        StubMethodDescriptor::static_method(
            "newScheduledThreadPool",
            "(I)Ljava/util/concurrent/ScheduledExecutorService;",
        ),
    );
    methods.insert(
        "newSingleThreadScheduledExecutor:()Ljava/util/concurrent/ScheduledExecutorService;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newSingleThreadScheduledExecutor",
            "()Ljava/util/concurrent/ScheduledExecutorService;",
        ),
    );
    methods.insert(
        "newWorkStealingPool:()Ljava/util/concurrent/ExecutorService;".to_string(),
        StubMethodDescriptor::static_method(
            "newWorkStealingPool",
            "()Ljava/util/concurrent/ExecutorService;",
        ),
    );
    methods.insert(
        "newWorkStealingPool:(I)Ljava/util/concurrent/ExecutorService;".to_string(),
        StubMethodDescriptor::static_method(
            "newWorkStealingPool",
            "(I)Ljava/util/concurrent/ExecutorService;",
        ),
    );

    // Callable/Runnable utilities
    methods.insert(
        "callable:(Ljava/lang/Runnable;)Ljava/util/concurrent/Callable;".to_string(),
        StubMethodDescriptor::static_method(
            "callable",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/Callable;",
        ),
    );
    methods.insert(
        "callable:(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Callable;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "callable",
            "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Callable;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/Executors".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.Future interface stub
pub fn create_future_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "cancel:(Z)Z".to_string(),
        StubMethodDescriptor::instance_method("cancel", "(Z)Z"),
    );
    methods.insert(
        "isCancelled:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCancelled", "()Z"),
    );
    methods.insert(
        "isDone:()Z".to_string(),
        StubMethodDescriptor::instance_method("isDone", "()Z"),
    );
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "get:(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "get",
            "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/Future".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.Callable interface stub
pub fn create_callable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "call:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("call", "()Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/concurrent/Callable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.CompletableFuture class stub
pub fn create_completable_future_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Static factory methods
    methods.insert(
        "completedFuture:(Ljava/lang/Object;)Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::static_method(
            "completedFuture",
            "(Ljava/lang/Object;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "supplyAsync:(Ljava/util/function/Supplier;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "supplyAsync",
            "(Ljava/util/function/Supplier;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "supplyAsync:(Ljava/util/function/Supplier;Ljava/util/concurrent/Executor;)Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::static_method(
            "supplyAsync",
            "(Ljava/util/function/Supplier;Ljava/util/concurrent/Executor;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "runAsync:(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::static_method(
            "runAsync",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "allOf:([Ljava/util/concurrent/CompletableFuture;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "allOf",
            "([Ljava/util/concurrent/CompletableFuture;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "anyOf:([Ljava/util/concurrent/CompletableFuture;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "anyOf",
            "([Ljava/util/concurrent/CompletableFuture;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );

    // Completion methods
    methods.insert(
        "complete:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("complete", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "completeExceptionally:(Ljava/lang/Throwable;)Z".to_string(),
        StubMethodDescriptor::instance_method("completeExceptionally", "(Ljava/lang/Throwable;)Z"),
    );

    // Retrieval methods
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "get:(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "get",
            "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "join:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("join", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getNow:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getNow", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );

    // Chaining methods
    methods.insert(
        "thenApply:(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "thenApply",
            "(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "thenApplyAsync:(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "thenApplyAsync",
            "(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "thenAccept:(Ljava/util/function/Consumer;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "thenAccept",
            "(Ljava/util/function/Consumer;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "thenAcceptAsync:(Ljava/util/function/Consumer;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "thenAcceptAsync",
            "(Ljava/util/function/Consumer;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "thenRun:(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "thenRun",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "thenRunAsync:(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "thenRunAsync",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "thenCompose:(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "thenCompose",
            "(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "thenCombine:(Ljava/util/concurrent/CompletionStage;Ljava/util/function/BiFunction;)Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "thenCombine",
            "(Ljava/util/concurrent/CompletionStage;Ljava/util/function/BiFunction;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );

    // Exception handling
    methods.insert(
        "exceptionally:(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "exceptionally",
            "(Ljava/util/function/Function;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "handle:(Ljava/util/function/BiFunction;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "handle",
            "(Ljava/util/function/BiFunction;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "whenComplete:(Ljava/util/function/BiConsumer;)Ljava/util/concurrent/CompletableFuture;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "whenComplete",
            "(Ljava/util/function/BiConsumer;)Ljava/util/concurrent/CompletableFuture;",
        ),
    );

    // Status methods
    methods.insert(
        "isDone:()Z".to_string(),
        StubMethodDescriptor::instance_method("isDone", "()Z"),
    );
    methods.insert(
        "isCancelled:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCancelled", "()Z"),
    );
    methods.insert(
        "isCompletedExceptionally:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCompletedExceptionally", "()Z"),
    );
    methods.insert(
        "cancel:(Z)Z".to_string(),
        StubMethodDescriptor::instance_method("cancel", "(Z)Z"),
    );

    StubClass {
        name: "java/util/concurrent/CompletableFuture".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/util/concurrent/Future".to_string(),
            "java/util/concurrent/CompletionStage".to_string(),
        ],
        methods,
    }
}

/// Create java.util.concurrent.locks.ReentrantLock class stub
pub fn create_reentrant_lock_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Z)V".to_string(),
        StubMethodDescriptor::constructor("(Z)V"),
    );

    // Lock methods
    methods.insert(
        "lock:()V".to_string(),
        StubMethodDescriptor::instance_method("lock", "()V"),
    );
    methods.insert(
        "unlock:()V".to_string(),
        StubMethodDescriptor::instance_method("unlock", "()V"),
    );
    methods.insert(
        "tryLock:()Z".to_string(),
        StubMethodDescriptor::instance_method("tryLock", "()Z"),
    );
    methods.insert(
        "tryLock:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method("tryLock", "(JLjava/util/concurrent/TimeUnit;)Z"),
    );
    methods.insert(
        "lockInterruptibly:()V".to_string(),
        StubMethodDescriptor::instance_method("lockInterruptibly", "()V"),
    );

    // Condition methods
    methods.insert(
        "newCondition:()Ljava/util/concurrent/locks/Condition;".to_string(),
        StubMethodDescriptor::instance_method(
            "newCondition",
            "()Ljava/util/concurrent/locks/Condition;",
        ),
    );

    // Query methods
    methods.insert(
        "isHeldByCurrentThread:()Z".to_string(),
        StubMethodDescriptor::instance_method("isHeldByCurrentThread", "()Z"),
    );
    methods.insert(
        "isLocked:()Z".to_string(),
        StubMethodDescriptor::instance_method("isLocked", "()Z"),
    );
    methods.insert(
        "isFair:()Z".to_string(),
        StubMethodDescriptor::instance_method("isFair", "()Z"),
    );
    methods.insert(
        "getHoldCount:()I".to_string(),
        StubMethodDescriptor::instance_method("getHoldCount", "()I"),
    );

    StubClass {
        name: "java/util/concurrent/locks/ReentrantLock".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/util/concurrent/locks/Lock".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.locks.Lock interface stub
pub fn create_lock_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "lock:()V".to_string(),
        StubMethodDescriptor::instance_method("lock", "()V"),
    );
    methods.insert(
        "unlock:()V".to_string(),
        StubMethodDescriptor::instance_method("unlock", "()V"),
    );
    methods.insert(
        "tryLock:()Z".to_string(),
        StubMethodDescriptor::instance_method("tryLock", "()Z"),
    );
    methods.insert(
        "tryLock:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method("tryLock", "(JLjava/util/concurrent/TimeUnit;)Z"),
    );
    methods.insert(
        "lockInterruptibly:()V".to_string(),
        StubMethodDescriptor::instance_method("lockInterruptibly", "()V"),
    );
    methods.insert(
        "newCondition:()Ljava/util/concurrent/locks/Condition;".to_string(),
        StubMethodDescriptor::instance_method(
            "newCondition",
            "()Ljava/util/concurrent/locks/Condition;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/locks/Lock".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.Semaphore class stub
pub fn create_semaphore_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(IZ)V".to_string(),
        StubMethodDescriptor::constructor("(IZ)V"),
    );

    // Acquire methods
    methods.insert(
        "acquire:()V".to_string(),
        StubMethodDescriptor::instance_method("acquire", "()V"),
    );
    methods.insert(
        "acquire:(I)V".to_string(),
        StubMethodDescriptor::instance_method("acquire", "(I)V"),
    );
    methods.insert(
        "acquireUninterruptibly:()V".to_string(),
        StubMethodDescriptor::instance_method("acquireUninterruptibly", "()V"),
    );
    methods.insert(
        "tryAcquire:()Z".to_string(),
        StubMethodDescriptor::instance_method("tryAcquire", "()Z"),
    );
    methods.insert(
        "tryAcquire:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("tryAcquire", "(I)Z"),
    );
    methods.insert(
        "tryAcquire:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method("tryAcquire", "(JLjava/util/concurrent/TimeUnit;)Z"),
    );

    // Release methods
    methods.insert(
        "release:()V".to_string(),
        StubMethodDescriptor::instance_method("release", "()V"),
    );
    methods.insert(
        "release:(I)V".to_string(),
        StubMethodDescriptor::instance_method("release", "(I)V"),
    );

    // Query methods
    methods.insert(
        "availablePermits:()I".to_string(),
        StubMethodDescriptor::instance_method("availablePermits", "()I"),
    );
    methods.insert(
        "drainPermits:()I".to_string(),
        StubMethodDescriptor::instance_method("drainPermits", "()I"),
    );
    methods.insert(
        "isFair:()Z".to_string(),
        StubMethodDescriptor::instance_method("isFair", "()Z"),
    );

    StubClass {
        name: "java/util/concurrent/Semaphore".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.CountDownLatch class stub
pub fn create_count_down_latch_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );

    // Core methods
    methods.insert(
        "await:()V".to_string(),
        StubMethodDescriptor::instance_method("await", "()V"),
    );
    methods.insert(
        "await:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method("await", "(JLjava/util/concurrent/TimeUnit;)Z"),
    );
    methods.insert(
        "countDown:()V".to_string(),
        StubMethodDescriptor::instance_method("countDown", "()V"),
    );
    methods.insert(
        "getCount:()J".to_string(),
        StubMethodDescriptor::instance_method("getCount", "()J"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/concurrent/CountDownLatch".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.ConcurrentHashMap class stub
pub fn create_concurrent_hash_map_class() -> StubClass {
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
        "<init>:(IFI)V".to_string(),
        StubMethodDescriptor::constructor("(IFI)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Map;)V"),
    );

    // Map methods
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
    methods.insert(
        "containsKey:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsKey", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "containsValue:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsValue", "(Ljava/lang/Object;)Z"),
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

    // Concurrent-specific methods
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
        "compute:(Ljava/lang/Object;Ljava/util/function/BiFunction;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "compute",
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

    // Iteration methods
    methods.insert(
        "forEach:(Ljava/util/function/BiConsumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEach", "(Ljava/util/function/BiConsumer;)V"),
    );
    methods.insert(
        "forEachKey:(JLjava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEachKey", "(JLjava/util/function/Consumer;)V"),
    );
    methods.insert(
        "forEachValue:(JLjava/util/function/Consumer;)V".to_string(),
        StubMethodDescriptor::instance_method("forEachValue", "(JLjava/util/function/Consumer;)V"),
    );

    StubClass {
        name: "java/util/concurrent/ConcurrentHashMap".to_string(),
        superclass: Some("java/util/AbstractMap".to_string()),
        interfaces: vec![
            "java/util/concurrent/ConcurrentMap".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.concurrent.BlockingQueue interface stub
pub fn create_blocking_queue_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Blocking operations
    methods.insert(
        "put:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("put", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "take:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("take", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "offer:(Ljava/lang/Object;JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "offer",
            "(Ljava/lang/Object;JLjava/util/concurrent/TimeUnit;)Z",
        ),
    );
    methods.insert(
        "poll:(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "poll",
            "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;",
        ),
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

    // Capacity methods
    methods.insert(
        "remainingCapacity:()I".to_string(),
        StubMethodDescriptor::instance_method("remainingCapacity", "()I"),
    );
    methods.insert(
        "drainTo:(Ljava/util/Collection;)I".to_string(),
        StubMethodDescriptor::instance_method("drainTo", "(Ljava/util/Collection;)I"),
    );
    methods.insert(
        "drainTo:(Ljava/util/Collection;I)I".to_string(),
        StubMethodDescriptor::instance_method("drainTo", "(Ljava/util/Collection;I)I"),
    );

    StubClass {
        name: "java/util/concurrent/BlockingQueue".to_string(),
        superclass: None,
        interfaces: vec!["java/util/Queue".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.TimeUnit enum stub
pub fn create_time_unit_class() -> StubClass {
    let mut methods = HashMap::new();

    // Enum constants (static fields - represented as methods for simplicity)
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/util/concurrent/TimeUnit;".to_string(),
        StubMethodDescriptor::static_method(
            "valueOf",
            "(Ljava/lang/String;)Ljava/util/concurrent/TimeUnit;",
        ),
    );
    methods.insert(
        "values:()[Ljava/util/concurrent/TimeUnit;".to_string(),
        StubMethodDescriptor::static_method("values", "()[Ljava/util/concurrent/TimeUnit;"),
    );

    // Conversion methods
    methods.insert(
        "convert:(JLjava/util/concurrent/TimeUnit;)J".to_string(),
        StubMethodDescriptor::instance_method("convert", "(JLjava/util/concurrent/TimeUnit;)J"),
    );
    methods.insert(
        "toNanos:(J)J".to_string(),
        StubMethodDescriptor::instance_method("toNanos", "(J)J"),
    );
    methods.insert(
        "toMicros:(J)J".to_string(),
        StubMethodDescriptor::instance_method("toMicros", "(J)J"),
    );
    methods.insert(
        "toMillis:(J)J".to_string(),
        StubMethodDescriptor::instance_method("toMillis", "(J)J"),
    );
    methods.insert(
        "toSeconds:(J)J".to_string(),
        StubMethodDescriptor::instance_method("toSeconds", "(J)J"),
    );
    methods.insert(
        "toMinutes:(J)J".to_string(),
        StubMethodDescriptor::instance_method("toMinutes", "(J)J"),
    );
    methods.insert(
        "toHours:(J)J".to_string(),
        StubMethodDescriptor::instance_method("toHours", "(J)J"),
    );
    methods.insert(
        "toDays:(J)J".to_string(),
        StubMethodDescriptor::instance_method("toDays", "(J)J"),
    );

    // Waiting methods
    methods.insert(
        "sleep:(J)V".to_string(),
        StubMethodDescriptor::instance_method("sleep", "(J)V"),
    );
    methods.insert(
        "timedWait:(Ljava/lang/Object;J)V".to_string(),
        StubMethodDescriptor::instance_method("timedWait", "(Ljava/lang/Object;J)V"),
    );
    methods.insert(
        "timedJoin:(Ljava/lang/Thread;J)V".to_string(),
        StubMethodDescriptor::instance_method("timedJoin", "(Ljava/lang/Thread;J)V"),
    );

    StubClass {
        name: "java/util/concurrent/TimeUnit".to_string(),
        superclass: Some("java/lang/Enum".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.AbstractExecutorService abstract class stub
pub fn create_abstract_executor_service_class() -> StubClass {
    let mut methods = HashMap::new();

    // ExecutorService methods (inherited/implemented)
    methods.insert(
        "submit:(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/Future;",
        ),
    );
    methods.insert(
        "submit:(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/Future;",
        ),
    );
    methods.insert(
        "submit:(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/Future;",
        ),
    );
    methods.insert(
        "invokeAll:(Ljava/util/Collection;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAll",
            "(Ljava/util/Collection;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "invokeAll:(Ljava/util/Collection;JLjava/util/concurrent/TimeUnit;)Ljava/util/List;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAll",
            "(Ljava/util/Collection;JLjava/util/concurrent/TimeUnit;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "invokeAny:(Ljava/util/Collection;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAny",
            "(Ljava/util/Collection;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeAny:(Ljava/util/Collection;JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAny",
            "(Ljava/util/Collection;JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/AbstractExecutorService".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/util/concurrent/ExecutorService".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.ThreadPoolExecutor class stub
pub fn create_thread_pool_executor_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;)V"
            .to_string(),
        StubMethodDescriptor::constructor(
            "(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;)V",
        ),
    );
    methods.insert(
        "<init>:(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/ThreadFactory;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/ThreadFactory;)V",
        ),
    );
    methods.insert(
        "<init>:(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/RejectedExecutionHandler;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/RejectedExecutionHandler;)V",
        ),
    );
    methods.insert(
        "<init>:(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/ThreadFactory;Ljava/util/concurrent/RejectedExecutionHandler;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;Ljava/util/concurrent/ThreadFactory;Ljava/util/concurrent/RejectedExecutionHandler;)V",
        ),
    );

    // Execute and shutdown
    methods.insert(
        "execute:(Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::instance_method("execute", "(Ljava/lang/Runnable;)V"),
    );
    methods.insert(
        "shutdown:()V".to_string(),
        StubMethodDescriptor::instance_method("shutdown", "()V"),
    );
    methods.insert(
        "shutdownNow:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("shutdownNow", "()Ljava/util/List;"),
    );
    methods.insert(
        "isShutdown:()Z".to_string(),
        StubMethodDescriptor::instance_method("isShutdown", "()Z"),
    );
    methods.insert(
        "isTerminating:()Z".to_string(),
        StubMethodDescriptor::instance_method("isTerminating", "()Z"),
    );
    methods.insert(
        "isTerminated:()Z".to_string(),
        StubMethodDescriptor::instance_method("isTerminated", "()Z"),
    );
    methods.insert(
        "awaitTermination:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "awaitTermination",
            "(JLjava/util/concurrent/TimeUnit;)Z",
        ),
    );

    // Pool size configuration
    methods.insert(
        "setCorePoolSize:(I)V".to_string(),
        StubMethodDescriptor::instance_method("setCorePoolSize", "(I)V"),
    );
    methods.insert(
        "getCorePoolSize:()I".to_string(),
        StubMethodDescriptor::instance_method("getCorePoolSize", "()I"),
    );
    methods.insert(
        "getMaximumPoolSize:()I".to_string(),
        StubMethodDescriptor::instance_method("getMaximumPoolSize", "()I"),
    );
    methods.insert(
        "setMaximumPoolSize:(I)V".to_string(),
        StubMethodDescriptor::instance_method("setMaximumPoolSize", "(I)V"),
    );
    methods.insert(
        "getPoolSize:()I".to_string(),
        StubMethodDescriptor::instance_method("getPoolSize", "()I"),
    );
    methods.insert(
        "getActiveCount:()I".to_string(),
        StubMethodDescriptor::instance_method("getActiveCount", "()I"),
    );
    methods.insert(
        "getLargestPoolSize:()I".to_string(),
        StubMethodDescriptor::instance_method("getLargestPoolSize", "()I"),
    );
    methods.insert(
        "getTaskCount:()J".to_string(),
        StubMethodDescriptor::instance_method("getTaskCount", "()J"),
    );
    methods.insert(
        "getCompletedTaskCount:()J".to_string(),
        StubMethodDescriptor::instance_method("getCompletedTaskCount", "()J"),
    );
    methods.insert(
        "getQueue:()Ljava/util/concurrent/BlockingQueue;".to_string(),
        StubMethodDescriptor::instance_method("getQueue", "()Ljava/util/concurrent/BlockingQueue;"),
    );

    // Keep alive configuration
    methods.insert(
        "setKeepAliveTime:(JLjava/util/concurrent/TimeUnit;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setKeepAliveTime",
            "(JLjava/util/concurrent/TimeUnit;)V",
        ),
    );
    methods.insert(
        "getKeepAliveTime:(Ljava/util/concurrent/TimeUnit;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "getKeepAliveTime",
            "(Ljava/util/concurrent/TimeUnit;)J",
        ),
    );
    methods.insert(
        "allowCoreThreadTimeOut:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("allowCoreThreadTimeOut", "(Z)V"),
    );
    methods.insert(
        "allowsCoreThreadTimeOut:()Z".to_string(),
        StubMethodDescriptor::instance_method("allowsCoreThreadTimeOut", "()Z"),
    );

    // Thread factory and rejection handler
    methods.insert(
        "setThreadFactory:(Ljava/util/concurrent/ThreadFactory;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setThreadFactory",
            "(Ljava/util/concurrent/ThreadFactory;)V",
        ),
    );
    methods.insert(
        "getThreadFactory:()Ljava/util/concurrent/ThreadFactory;".to_string(),
        StubMethodDescriptor::instance_method(
            "getThreadFactory",
            "()Ljava/util/concurrent/ThreadFactory;",
        ),
    );
    methods.insert(
        "setRejectedExecutionHandler:(Ljava/util/concurrent/RejectedExecutionHandler;)V"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "setRejectedExecutionHandler",
            "(Ljava/util/concurrent/RejectedExecutionHandler;)V",
        ),
    );
    methods.insert(
        "getRejectedExecutionHandler:()Ljava/util/concurrent/RejectedExecutionHandler;".to_string(),
        StubMethodDescriptor::instance_method(
            "getRejectedExecutionHandler",
            "()Ljava/util/concurrent/RejectedExecutionHandler;",
        ),
    );

    // Task management
    methods.insert(
        "remove:(Ljava/lang/Runnable;)Z".to_string(),
        StubMethodDescriptor::instance_method("remove", "(Ljava/lang/Runnable;)Z"),
    );
    methods.insert(
        "purge:()V".to_string(),
        StubMethodDescriptor::instance_method("purge", "()V"),
    );

    // Protected hooks (included for completeness)
    methods.insert(
        "beforeExecute:(Ljava/lang/Thread;Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "beforeExecute",
            "(Ljava/lang/Thread;Ljava/lang/Runnable;)V",
        ),
    );
    methods.insert(
        "afterExecute:(Ljava/lang/Runnable;Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "afterExecute",
            "(Ljava/lang/Runnable;Ljava/lang/Throwable;)V",
        ),
    );
    methods.insert(
        "terminated:()V".to_string(),
        StubMethodDescriptor::instance_method("terminated", "()V"),
    );

    StubClass {
        name: "java/util/concurrent/ThreadPoolExecutor".to_string(),
        superclass: Some("java/util/concurrent/AbstractExecutorService".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.ForkJoinPool class stub
pub fn create_fork_join_pool_class() -> StubClass {
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
        "<init>:(ILjava/util/concurrent/ForkJoinPool$ForkJoinWorkerThreadFactory;Ljava/lang/Thread$UncaughtExceptionHandler;Z)V".to_string(),
        StubMethodDescriptor::constructor(
            "(ILjava/util/concurrent/ForkJoinPool$ForkJoinWorkerThreadFactory;Ljava/lang/Thread$UncaughtExceptionHandler;Z)V",
        ),
    );

    // Static factory methods
    methods.insert(
        "commonPool:()Ljava/util/concurrent/ForkJoinPool;".to_string(),
        StubMethodDescriptor::static_method("commonPool", "()Ljava/util/concurrent/ForkJoinPool;"),
    );
    methods.insert(
        "getCommonPoolParallelism:()I".to_string(),
        StubMethodDescriptor::static_method("getCommonPoolParallelism", "()I"),
    );

    // Task execution
    methods.insert(
        "invoke:(Ljava/util/concurrent/ForkJoinTask;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invoke",
            "(Ljava/util/concurrent/ForkJoinTask;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "execute:(Ljava/util/concurrent/ForkJoinTask;)V".to_string(),
        StubMethodDescriptor::instance_method("execute", "(Ljava/util/concurrent/ForkJoinTask;)V"),
    );
    methods.insert(
        "execute:(Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::instance_method("execute", "(Ljava/lang/Runnable;)V"),
    );
    methods.insert(
        "submit:(Ljava/util/concurrent/ForkJoinTask;)Ljava/util/concurrent/ForkJoinTask;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/util/concurrent/ForkJoinTask;)Ljava/util/concurrent/ForkJoinTask;",
        ),
    );
    methods.insert(
        "submit:(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/ForkJoinTask;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/ForkJoinTask;",
        ),
    );
    methods.insert(
        "submit:(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/ForkJoinTask;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/ForkJoinTask;",
        ),
    );
    methods.insert(
        "submit:(Ljava/lang/Runnable;)Ljava/util/concurrent/ForkJoinTask;".to_string(),
        StubMethodDescriptor::instance_method(
            "submit",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/ForkJoinTask;",
        ),
    );
    methods.insert(
        "invokeAll:(Ljava/util/Collection;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeAll",
            "(Ljava/util/Collection;)Ljava/util/List;",
        ),
    );

    // Pool status
    methods.insert(
        "getParallelism:()I".to_string(),
        StubMethodDescriptor::instance_method("getParallelism", "()I"),
    );
    methods.insert(
        "getPoolSize:()I".to_string(),
        StubMethodDescriptor::instance_method("getPoolSize", "()I"),
    );
    methods.insert(
        "getActiveThreadCount:()I".to_string(),
        StubMethodDescriptor::instance_method("getActiveThreadCount", "()I"),
    );
    methods.insert(
        "getRunningThreadCount:()I".to_string(),
        StubMethodDescriptor::instance_method("getRunningThreadCount", "()I"),
    );
    methods.insert(
        "getQueuedSubmissionCount:()I".to_string(),
        StubMethodDescriptor::instance_method("getQueuedSubmissionCount", "()I"),
    );
    methods.insert(
        "getQueuedTaskCount:()J".to_string(),
        StubMethodDescriptor::instance_method("getQueuedTaskCount", "()J"),
    );
    methods.insert(
        "getStealCount:()J".to_string(),
        StubMethodDescriptor::instance_method("getStealCount", "()J"),
    );
    methods.insert(
        "isQuiescent:()Z".to_string(),
        StubMethodDescriptor::instance_method("isQuiescent", "()Z"),
    );

    // Shutdown
    methods.insert(
        "shutdown:()V".to_string(),
        StubMethodDescriptor::instance_method("shutdown", "()V"),
    );
    methods.insert(
        "shutdownNow:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("shutdownNow", "()Ljava/util/List;"),
    );
    methods.insert(
        "isShutdown:()Z".to_string(),
        StubMethodDescriptor::instance_method("isShutdown", "()Z"),
    );
    methods.insert(
        "isTerminating:()Z".to_string(),
        StubMethodDescriptor::instance_method("isTerminating", "()Z"),
    );
    methods.insert(
        "isTerminated:()Z".to_string(),
        StubMethodDescriptor::instance_method("isTerminated", "()Z"),
    );
    methods.insert(
        "awaitTermination:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "awaitTermination",
            "(JLjava/util/concurrent/TimeUnit;)Z",
        ),
    );
    methods.insert(
        "awaitQuiescence:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "awaitQuiescence",
            "(JLjava/util/concurrent/TimeUnit;)Z",
        ),
    );

    StubClass {
        name: "java/util/concurrent/ForkJoinPool".to_string(),
        superclass: Some("java/util/concurrent/AbstractExecutorService".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.ForkJoinTask abstract class stub
pub fn create_fork_join_task_class() -> StubClass {
    let mut methods = HashMap::new();

    // Core fork/join operations
    methods.insert(
        "fork:()Ljava/util/concurrent/ForkJoinTask;".to_string(),
        StubMethodDescriptor::instance_method("fork", "()Ljava/util/concurrent/ForkJoinTask;"),
    );
    methods.insert(
        "join:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("join", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "invoke:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("invoke", "()Ljava/lang/Object;"),
    );

    // Static invokeAll variants
    methods.insert(
        "invokeAll:(Ljava/util/concurrent/ForkJoinTask;Ljava/util/concurrent/ForkJoinTask;)V"
            .to_string(),
        StubMethodDescriptor::static_method(
            "invokeAll",
            "(Ljava/util/concurrent/ForkJoinTask;Ljava/util/concurrent/ForkJoinTask;)V",
        ),
    );
    methods.insert(
        "invokeAll:([Ljava/util/concurrent/ForkJoinTask;)V".to_string(),
        StubMethodDescriptor::static_method("invokeAll", "([Ljava/util/concurrent/ForkJoinTask;)V"),
    );
    methods.insert(
        "invokeAll:(Ljava/util/Collection;)Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::static_method(
            "invokeAll",
            "(Ljava/util/Collection;)Ljava/util/Collection;",
        ),
    );

    // Future interface methods
    methods.insert(
        "cancel:(Z)Z".to_string(),
        StubMethodDescriptor::instance_method("cancel", "(Z)Z"),
    );
    methods.insert(
        "isDone:()Z".to_string(),
        StubMethodDescriptor::instance_method("isDone", "()Z"),
    );
    methods.insert(
        "isCancelled:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCancelled", "()Z"),
    );
    methods.insert(
        "isCompletedAbnormally:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCompletedAbnormally", "()Z"),
    );
    methods.insert(
        "isCompletedNormally:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCompletedNormally", "()Z"),
    );
    methods.insert(
        "getException:()Ljava/lang/Throwable;".to_string(),
        StubMethodDescriptor::instance_method("getException", "()Ljava/lang/Throwable;"),
    );
    methods.insert(
        "complete:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("complete", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "completeExceptionally:(Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::instance_method("completeExceptionally", "(Ljava/lang/Throwable;)V"),
    );

    // Get methods
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "get:(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "get",
            "(JLjava/util/concurrent/TimeUnit;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getRawResult:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getRawResult", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "setRawResult:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("setRawResult", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "exec:()Z".to_string(),
        StubMethodDescriptor::instance_method("exec", "()Z"),
    );

    // Tag operations
    methods.insert(
        "getForkJoinTaskTag:()S".to_string(),
        StubMethodDescriptor::instance_method("getForkJoinTaskTag", "()S"),
    );
    methods.insert(
        "setForkJoinTaskTag:(S)S".to_string(),
        StubMethodDescriptor::instance_method("setForkJoinTaskTag", "(S)S"),
    );
    methods.insert(
        "compareAndSetForkJoinTaskTag:(SS)Z".to_string(),
        StubMethodDescriptor::instance_method("compareAndSetForkJoinTaskTag", "(SS)Z"),
    );

    // Static utility methods
    methods.insert(
        "getPool:()Ljava/util/concurrent/ForkJoinPool;".to_string(),
        StubMethodDescriptor::static_method("getPool", "()Ljava/util/concurrent/ForkJoinPool;"),
    );
    methods.insert(
        "inForkJoinPool:()Z".to_string(),
        StubMethodDescriptor::static_method("inForkJoinPool", "()Z"),
    );
    methods.insert(
        "tryUnfork:()Z".to_string(),
        StubMethodDescriptor::instance_method("tryUnfork", "()Z"),
    );
    methods.insert(
        "getQueuedTaskCount:()I".to_string(),
        StubMethodDescriptor::static_method("getQueuedTaskCount", "()I"),
    );
    methods.insert(
        "getSurplusQueuedTaskCount:()I".to_string(),
        StubMethodDescriptor::static_method("getSurplusQueuedTaskCount", "()I"),
    );

    // Quiet completion methods
    methods.insert(
        "quietlyComplete:()V".to_string(),
        StubMethodDescriptor::instance_method("quietlyComplete", "()V"),
    );
    methods.insert(
        "quietlyInvoke:()V".to_string(),
        StubMethodDescriptor::instance_method("quietlyInvoke", "()V"),
    );
    methods.insert(
        "quietlyJoin:()V".to_string(),
        StubMethodDescriptor::instance_method("quietlyJoin", "()V"),
    );
    methods.insert(
        "helpQuiesce:()V".to_string(),
        StubMethodDescriptor::static_method("helpQuiesce", "()V"),
    );
    methods.insert(
        "reinitialize:()V".to_string(),
        StubMethodDescriptor::instance_method("reinitialize", "()V"),
    );

    // Adapt methods
    methods.insert(
        "adapt:(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/ForkJoinTask;".to_string(),
        StubMethodDescriptor::static_method(
            "adapt",
            "(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/ForkJoinTask;",
        ),
    );
    methods.insert(
        "adapt:(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/ForkJoinTask;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "adapt",
            "(Ljava/lang/Runnable;Ljava/lang/Object;)Ljava/util/concurrent/ForkJoinTask;",
        ),
    );
    methods.insert(
        "adapt:(Ljava/lang/Runnable;)Ljava/util/concurrent/ForkJoinTask;".to_string(),
        StubMethodDescriptor::static_method(
            "adapt",
            "(Ljava/lang/Runnable;)Ljava/util/concurrent/ForkJoinTask;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/ForkJoinTask".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/util/concurrent/Future".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create java.util.concurrent.ScheduledExecutorService interface stub
pub fn create_scheduled_executor_service_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "schedule:(Ljava/lang/Runnable;JLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "schedule",
            "(Ljava/lang/Runnable;JLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;",
        ),
    );
    methods.insert(
        "schedule:(Ljava/util/concurrent/Callable;JLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "schedule",
            "(Ljava/util/concurrent/Callable;JLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;",
        ),
    );
    methods.insert(
        "scheduleAtFixedRate:(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "scheduleAtFixedRate",
            "(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;",
        ),
    );
    methods.insert(
        "scheduleWithFixedDelay:(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "scheduleWithFixedDelay",
            "(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/ScheduledExecutorService".to_string(),
        superclass: None,
        interfaces: vec!["java/util/concurrent/ExecutorService".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.ScheduledFuture interface stub
pub fn create_scheduled_future_interface() -> StubClass {
    let methods = HashMap::new();
    // ScheduledFuture inherits all methods from Delayed and Future
    // No new methods declared in the interface itself

    StubClass {
        name: "java/util/concurrent/ScheduledFuture".to_string(),
        superclass: None,
        interfaces: vec![
            "java/util/concurrent/Delayed".to_string(),
            "java/util/concurrent/Future".to_string(),
        ],
        methods,
    }
}

/// Create java.util.concurrent.Delayed interface stub
pub fn create_delayed_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "getDelay:(Ljava/util/concurrent/TimeUnit;)J".to_string(),
        StubMethodDescriptor::instance_method("getDelay", "(Ljava/util/concurrent/TimeUnit;)J"),
    );

    StubClass {
        name: "java/util/concurrent/Delayed".to_string(),
        superclass: None,
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.locks.Condition interface stub
pub fn create_condition_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "await:()V".to_string(),
        StubMethodDescriptor::instance_method("await", "()V"),
    );
    methods.insert(
        "awaitUninterruptibly:()V".to_string(),
        StubMethodDescriptor::instance_method("awaitUninterruptibly", "()V"),
    );
    methods.insert(
        "awaitNanos:(J)J".to_string(),
        StubMethodDescriptor::instance_method("awaitNanos", "(J)J"),
    );
    methods.insert(
        "await:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method("await", "(JLjava/util/concurrent/TimeUnit;)Z"),
    );
    methods.insert(
        "awaitUntil:(Ljava/util/Date;)Z".to_string(),
        StubMethodDescriptor::instance_method("awaitUntil", "(Ljava/util/Date;)Z"),
    );
    methods.insert(
        "signal:()V".to_string(),
        StubMethodDescriptor::instance_method("signal", "()V"),
    );
    methods.insert(
        "signalAll:()V".to_string(),
        StubMethodDescriptor::instance_method("signalAll", "()V"),
    );

    StubClass {
        name: "java/util/concurrent/locks/Condition".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.ThreadFactory interface stub
pub fn create_thread_factory_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "newThread:(Ljava/lang/Runnable;)Ljava/lang/Thread;".to_string(),
        StubMethodDescriptor::instance_method(
            "newThread",
            "(Ljava/lang/Runnable;)Ljava/lang/Thread;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/ThreadFactory".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.RejectedExecutionHandler interface stub
pub fn create_rejected_execution_handler_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "rejectedExecution:(Ljava/lang/Runnable;Ljava/util/concurrent/ThreadPoolExecutor;)V"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "rejectedExecution",
            "(Ljava/lang/Runnable;Ljava/util/concurrent/ThreadPoolExecutor;)V",
        ),
    );

    StubClass {
        name: "java/util/concurrent/RejectedExecutionHandler".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.ForkJoinPool$ForkJoinWorkerThreadFactory interface stub
pub fn create_fork_join_worker_thread_factory_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "newThread:(Ljava/util/concurrent/ForkJoinPool;)Ljava/util/concurrent/ForkJoinWorkerThread;".to_string(),
        StubMethodDescriptor::instance_method(
            "newThread",
            "(Ljava/util/concurrent/ForkJoinPool;)Ljava/util/concurrent/ForkJoinWorkerThread;",
        ),
    );

    StubClass {
        name: "java/util/concurrent/ForkJoinPool$ForkJoinWorkerThreadFactory".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.ForkJoinWorkerThread class stub
pub fn create_fork_join_worker_thread_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/util/concurrent/ForkJoinPool;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/concurrent/ForkJoinPool;)V"),
    );

    // Instance methods
    methods.insert(
        "getPool:()Ljava/util/concurrent/ForkJoinPool;".to_string(),
        StubMethodDescriptor::instance_method("getPool", "()Ljava/util/concurrent/ForkJoinPool;"),
    );
    methods.insert(
        "getPoolIndex:()I".to_string(),
        StubMethodDescriptor::instance_method("getPoolIndex", "()I"),
    );

    // Protected lifecycle hooks
    methods.insert(
        "onStart:()V".to_string(),
        StubMethodDescriptor::instance_method("onStart", "()V"),
    );
    methods.insert(
        "onTermination:(Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::instance_method("onTermination", "(Ljava/lang/Throwable;)V"),
    );

    StubClass {
        name: "java/util/concurrent/ForkJoinWorkerThread".to_string(),
        superclass: Some("java/lang/Thread".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.atomic.AtomicInteger class stub
pub fn create_atomic_integer_class() -> StubClass {
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

    // Core atomic operations
    methods.insert(
        "get:()I".to_string(),
        StubMethodDescriptor::instance_method("get", "()I"),
    );
    methods.insert(
        "set:(I)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(I)V"),
    );
    methods.insert(
        "lazySet:(I)V".to_string(),
        StubMethodDescriptor::instance_method("lazySet", "(I)V"),
    );
    methods.insert(
        "getAndSet:(I)I".to_string(),
        StubMethodDescriptor::instance_method("getAndSet", "(I)I"),
    );
    methods.insert(
        "compareAndSet:(II)Z".to_string(),
        StubMethodDescriptor::instance_method("compareAndSet", "(II)Z"),
    );
    methods.insert(
        "weakCompareAndSet:(II)Z".to_string(),
        StubMethodDescriptor::instance_method("weakCompareAndSet", "(II)Z"),
    );

    // Increment/decrement operations
    methods.insert(
        "getAndIncrement:()I".to_string(),
        StubMethodDescriptor::instance_method("getAndIncrement", "()I"),
    );
    methods.insert(
        "getAndDecrement:()I".to_string(),
        StubMethodDescriptor::instance_method("getAndDecrement", "()I"),
    );
    methods.insert(
        "getAndAdd:(I)I".to_string(),
        StubMethodDescriptor::instance_method("getAndAdd", "(I)I"),
    );
    methods.insert(
        "incrementAndGet:()I".to_string(),
        StubMethodDescriptor::instance_method("incrementAndGet", "()I"),
    );
    methods.insert(
        "decrementAndGet:()I".to_string(),
        StubMethodDescriptor::instance_method("decrementAndGet", "()I"),
    );
    methods.insert(
        "addAndGet:(I)I".to_string(),
        StubMethodDescriptor::instance_method("addAndGet", "(I)I"),
    );

    // Functional update operations
    methods.insert(
        "getAndUpdate:(Ljava/util/function/IntUnaryOperator;)I".to_string(),
        StubMethodDescriptor::instance_method(
            "getAndUpdate",
            "(Ljava/util/function/IntUnaryOperator;)I",
        ),
    );
    methods.insert(
        "updateAndGet:(Ljava/util/function/IntUnaryOperator;)I".to_string(),
        StubMethodDescriptor::instance_method(
            "updateAndGet",
            "(Ljava/util/function/IntUnaryOperator;)I",
        ),
    );
    methods.insert(
        "getAndAccumulate:(ILjava/util/function/IntBinaryOperator;)I".to_string(),
        StubMethodDescriptor::instance_method(
            "getAndAccumulate",
            "(ILjava/util/function/IntBinaryOperator;)I",
        ),
    );
    methods.insert(
        "accumulateAndGet:(ILjava/util/function/IntBinaryOperator;)I".to_string(),
        StubMethodDescriptor::instance_method(
            "accumulateAndGet",
            "(ILjava/util/function/IntBinaryOperator;)I",
        ),
    );

    // Number interface methods
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/concurrent/atomic/AtomicInteger".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.atomic.AtomicLong class stub
pub fn create_atomic_long_class() -> StubClass {
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

    // Core atomic operations
    methods.insert(
        "get:()J".to_string(),
        StubMethodDescriptor::instance_method("get", "()J"),
    );
    methods.insert(
        "set:(J)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(J)V"),
    );
    methods.insert(
        "lazySet:(J)V".to_string(),
        StubMethodDescriptor::instance_method("lazySet", "(J)V"),
    );
    methods.insert(
        "getAndSet:(J)J".to_string(),
        StubMethodDescriptor::instance_method("getAndSet", "(J)J"),
    );
    methods.insert(
        "compareAndSet:(JJ)Z".to_string(),
        StubMethodDescriptor::instance_method("compareAndSet", "(JJ)Z"),
    );
    methods.insert(
        "weakCompareAndSet:(JJ)Z".to_string(),
        StubMethodDescriptor::instance_method("weakCompareAndSet", "(JJ)Z"),
    );

    // Increment/decrement operations
    methods.insert(
        "getAndIncrement:()J".to_string(),
        StubMethodDescriptor::instance_method("getAndIncrement", "()J"),
    );
    methods.insert(
        "getAndDecrement:()J".to_string(),
        StubMethodDescriptor::instance_method("getAndDecrement", "()J"),
    );
    methods.insert(
        "getAndAdd:(J)J".to_string(),
        StubMethodDescriptor::instance_method("getAndAdd", "(J)J"),
    );
    methods.insert(
        "incrementAndGet:()J".to_string(),
        StubMethodDescriptor::instance_method("incrementAndGet", "()J"),
    );
    methods.insert(
        "decrementAndGet:()J".to_string(),
        StubMethodDescriptor::instance_method("decrementAndGet", "()J"),
    );
    methods.insert(
        "addAndGet:(J)J".to_string(),
        StubMethodDescriptor::instance_method("addAndGet", "(J)J"),
    );

    // Functional update operations
    methods.insert(
        "getAndUpdate:(Ljava/util/function/LongUnaryOperator;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "getAndUpdate",
            "(Ljava/util/function/LongUnaryOperator;)J",
        ),
    );
    methods.insert(
        "updateAndGet:(Ljava/util/function/LongUnaryOperator;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "updateAndGet",
            "(Ljava/util/function/LongUnaryOperator;)J",
        ),
    );
    methods.insert(
        "getAndAccumulate:(JLjava/util/function/LongBinaryOperator;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "getAndAccumulate",
            "(JLjava/util/function/LongBinaryOperator;)J",
        ),
    );
    methods.insert(
        "accumulateAndGet:(JLjava/util/function/LongBinaryOperator;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "accumulateAndGet",
            "(JLjava/util/function/LongBinaryOperator;)J",
        ),
    );

    // Number interface methods
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/concurrent/atomic/AtomicLong".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.atomic.AtomicBoolean class stub
pub fn create_atomic_boolean_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Z)V".to_string(),
        StubMethodDescriptor::constructor("(Z)V"),
    );

    // Core atomic operations
    methods.insert(
        "get:()Z".to_string(),
        StubMethodDescriptor::instance_method("get", "()Z"),
    );
    methods.insert(
        "set:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(Z)V"),
    );
    methods.insert(
        "lazySet:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("lazySet", "(Z)V"),
    );
    methods.insert(
        "getAndSet:(Z)Z".to_string(),
        StubMethodDescriptor::instance_method("getAndSet", "(Z)Z"),
    );
    methods.insert(
        "compareAndSet:(ZZ)Z".to_string(),
        StubMethodDescriptor::instance_method("compareAndSet", "(ZZ)Z"),
    );
    methods.insert(
        "weakCompareAndSet:(ZZ)Z".to_string(),
        StubMethodDescriptor::instance_method("weakCompareAndSet", "(ZZ)Z"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/concurrent/atomic/AtomicBoolean".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.atomic.AtomicReference class stub
pub fn create_atomic_reference_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Object;)V"),
    );

    // Core atomic operations
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "lazySet:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("lazySet", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "getAndSet:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAndSet",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "compareAndSet:(Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "compareAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        ),
    );
    methods.insert(
        "weakCompareAndSet:(Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "weakCompareAndSet",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        ),
    );

    // Functional update operations
    methods.insert(
        "getAndUpdate:(Ljava/util/function/UnaryOperator;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAndUpdate",
            "(Ljava/util/function/UnaryOperator;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "updateAndGet:(Ljava/util/function/UnaryOperator;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "updateAndGet",
            "(Ljava/util/function/UnaryOperator;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getAndAccumulate:(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getAndAccumulate", "(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;"),
    );
    methods.insert(
        "accumulateAndGet:(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("accumulateAndGet", "(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/concurrent/atomic/AtomicReference".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.atomic.AtomicIntegerArray class stub
pub fn create_atomic_integer_array_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:([I)V".to_string(),
        StubMethodDescriptor::constructor("([I)V"),
    );

    // Core array operations
    methods.insert(
        "length:()I".to_string(),
        StubMethodDescriptor::instance_method("length", "()I"),
    );
    methods.insert(
        "get:(I)I".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)I"),
    );
    methods.insert(
        "set:(II)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(II)V"),
    );
    methods.insert(
        "lazySet:(II)V".to_string(),
        StubMethodDescriptor::instance_method("lazySet", "(II)V"),
    );
    methods.insert(
        "getAndSet:(II)I".to_string(),
        StubMethodDescriptor::instance_method("getAndSet", "(II)I"),
    );
    methods.insert(
        "compareAndSet:(III)Z".to_string(),
        StubMethodDescriptor::instance_method("compareAndSet", "(III)Z"),
    );

    // Increment/decrement operations
    methods.insert(
        "getAndIncrement:(I)I".to_string(),
        StubMethodDescriptor::instance_method("getAndIncrement", "(I)I"),
    );
    methods.insert(
        "getAndDecrement:(I)I".to_string(),
        StubMethodDescriptor::instance_method("getAndDecrement", "(I)I"),
    );
    methods.insert(
        "getAndAdd:(II)I".to_string(),
        StubMethodDescriptor::instance_method("getAndAdd", "(II)I"),
    );
    methods.insert(
        "incrementAndGet:(I)I".to_string(),
        StubMethodDescriptor::instance_method("incrementAndGet", "(I)I"),
    );
    methods.insert(
        "decrementAndGet:(I)I".to_string(),
        StubMethodDescriptor::instance_method("decrementAndGet", "(I)I"),
    );
    methods.insert(
        "addAndGet:(II)I".to_string(),
        StubMethodDescriptor::instance_method("addAndGet", "(II)I"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/concurrent/atomic/AtomicIntegerArray".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.concurrent.CyclicBarrier class stub
pub fn create_cyclic_barrier_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(ILjava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::constructor("(ILjava/lang/Runnable;)V"),
    );

    // Core barrier operations
    methods.insert(
        "getParties:()I".to_string(),
        StubMethodDescriptor::instance_method("getParties", "()I"),
    );
    methods.insert(
        "await:()I".to_string(),
        StubMethodDescriptor::instance_method("await", "()I"),
    );
    methods.insert(
        "await:(JLjava/util/concurrent/TimeUnit;)I".to_string(),
        StubMethodDescriptor::instance_method("await", "(JLjava/util/concurrent/TimeUnit;)I"),
    );

    // State query methods
    methods.insert(
        "isBroken:()Z".to_string(),
        StubMethodDescriptor::instance_method("isBroken", "()Z"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "getNumberWaiting:()I".to_string(),
        StubMethodDescriptor::instance_method("getNumberWaiting", "()I"),
    );

    StubClass {
        name: "java/util/concurrent/CyclicBarrier".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.Phaser class stub
pub fn create_phaser_class() -> StubClass {
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
        "<init>:(Ljava/util/concurrent/Phaser;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/concurrent/Phaser;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/concurrent/Phaser;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/concurrent/Phaser;I)V"),
    );

    // Registration methods
    methods.insert(
        "register:()I".to_string(),
        StubMethodDescriptor::instance_method("register", "()I"),
    );
    methods.insert(
        "bulkRegister:(I)I".to_string(),
        StubMethodDescriptor::instance_method("bulkRegister", "(I)I"),
    );

    // Phase arrival methods
    methods.insert(
        "arrive:()I".to_string(),
        StubMethodDescriptor::instance_method("arrive", "()I"),
    );
    methods.insert(
        "arriveAndDeregister:()I".to_string(),
        StubMethodDescriptor::instance_method("arriveAndDeregister", "()I"),
    );
    methods.insert(
        "arriveAndAwaitAdvance:()I".to_string(),
        StubMethodDescriptor::instance_method("arriveAndAwaitAdvance", "()I"),
    );

    // Await methods
    methods.insert(
        "awaitAdvance:(I)I".to_string(),
        StubMethodDescriptor::instance_method("awaitAdvance", "(I)I"),
    );
    methods.insert(
        "awaitAdvanceInterruptibly:(I)I".to_string(),
        StubMethodDescriptor::instance_method("awaitAdvanceInterruptibly", "(I)I"),
    );
    methods.insert(
        "awaitAdvanceInterruptibly:(IJLjava/util/concurrent/TimeUnit;)I".to_string(),
        StubMethodDescriptor::instance_method(
            "awaitAdvanceInterruptibly",
            "(IJLjava/util/concurrent/TimeUnit;)I",
        ),
    );

    // Control methods
    methods.insert(
        "forceTermination:()V".to_string(),
        StubMethodDescriptor::instance_method("forceTermination", "()V"),
    );

    // Query methods
    methods.insert(
        "getPhase:()I".to_string(),
        StubMethodDescriptor::instance_method("getPhase", "()I"),
    );
    methods.insert(
        "getRegisteredParties:()I".to_string(),
        StubMethodDescriptor::instance_method("getRegisteredParties", "()I"),
    );
    methods.insert(
        "getArrivedParties:()I".to_string(),
        StubMethodDescriptor::instance_method("getArrivedParties", "()I"),
    );
    methods.insert(
        "getUnarrivedParties:()I".to_string(),
        StubMethodDescriptor::instance_method("getUnarrivedParties", "()I"),
    );
    methods.insert(
        "getParent:()Ljava/util/concurrent/Phaser;".to_string(),
        StubMethodDescriptor::instance_method("getParent", "()Ljava/util/concurrent/Phaser;"),
    );
    methods.insert(
        "getRoot:()Ljava/util/concurrent/Phaser;".to_string(),
        StubMethodDescriptor::instance_method("getRoot", "()Ljava/util/concurrent/Phaser;"),
    );
    methods.insert(
        "isTerminated:()Z".to_string(),
        StubMethodDescriptor::instance_method("isTerminated", "()Z"),
    );

    // Protected hook
    methods.insert(
        "onAdvance:(II)Z".to_string(),
        StubMethodDescriptor::instance_method("onAdvance", "(II)Z"),
    );

    StubClass {
        name: "java/util/concurrent/Phaser".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.concurrent.BrokenBarrierException class stub
pub fn create_broken_barrier_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    StubClass {
        name: "java/util/concurrent/BrokenBarrierException".to_string(),
        superclass: Some("java/lang/Exception".to_string()),
        interfaces: vec![],
        methods,
    }
}
