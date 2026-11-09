// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Concurrency Tests for LSP Operations
//!
//! Tests for parallel and concurrent LSP operations to ensure thread safety
//! and correct behavior under load. These tests verify that:
//!
//! - Multiple goto-definition requests can run in parallel
//! - Multiple hover requests can run in parallel
//! - Mixed request types work correctly in parallel
//! - Rapid file changes don't cause race conditions
//! - The system remains stable under load

use futures::future::join_all;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn build_shared_context() -> crate::lsp_test_framework::SharedLspContext {
    let mut test = crate::lsp_test_framework::LspTest::new();
    test.add_file(
        "com/example/Main.java",
        r"
            package com.example;
            public class Main {
                void test() {
                    Us<|>er user = null;
                }
            }
        ",
    );
    test.add_file(
        "com/example/User.java",
        r"
            package com.example;
            public class User {
                private String name;
            }
        ",
    );
    test.add_file(
        "com/example/Test.java",
        r"
            package com.example;
            public class Test {
                void te<|>st() {}
            }
        ",
    );
    test.add_file(
        "com/example/Comp.java",
        r"
            package com.example;
            public class Comp {
                void test() {
                    Widget widget = new Widget();
                    widget.<|>
                }
            }
        ",
    );
    test.add_file(
        "com/example/Widget.java",
        r#"
            package com.example;
            public class Widget {
                public String getValue() { return "ok"; }
            }
        "#,
    );

    test.into_shared_context_with_options(false)
        .expect("Failed to build shared context")
}

// ============================================================================
// Parallel Request Tests (Shared Workspace)
// ============================================================================

/// Test concurrent goto-definition requests against shared state
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_parallel_goto_definition() {
    let shared = Arc::new(build_shared_context());
    let mut handles = vec![];

    for _ in 0..10 {
        let shared = Arc::clone(&shared);
        handles.push(tokio::spawn(async move {
            shared.goto_definition("com/example/Main.java", "<|>")
        }));
    }

    let results = join_all(handles).await;
    for (i, result) in results.into_iter().enumerate() {
        let response = result.expect("Task panicked");
        assert!(
            matches!(response, Ok(Some(_))),
            "Task {}: goto_definition failed: {:?}",
            i,
            response
        );
    }
}

/// Test concurrent hover requests against shared state
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_hover_requests() {
    let shared = Arc::new(build_shared_context());
    let mut handles = vec![];

    for _ in 0..10 {
        let shared = Arc::clone(&shared);
        handles.push(tokio::spawn(async move {
            shared.hover("com/example/Test.java", "<|>")
        }));
    }

    let results = join_all(handles).await;
    for (i, result) in results.into_iter().enumerate() {
        let response = result.expect("Task panicked");
        assert!(
            matches!(response, Ok(Some(_))),
            "Task {}: hover failed: {:?}",
            i,
            response
        );
    }
}

/// Test mixed goto and hover requests against shared state
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_mixed_goto_and_hover() {
    let shared = Arc::new(build_shared_context());
    let mut goto_handles = vec![];
    let mut hover_handles = vec![];

    for _ in 0..10 {
        let shared = Arc::clone(&shared);
        goto_handles.push(tokio::spawn(async move {
            shared.goto_definition("com/example/Main.java", "<|>")
        }));
    }

    for _ in 0..10 {
        let shared = Arc::clone(&shared);
        hover_handles.push(tokio::spawn(async move {
            shared.hover("com/example/Test.java", "<|>")
        }));
    }

    let goto_results = join_all(goto_handles).await;
    for (i, result) in goto_results.into_iter().enumerate() {
        let response = result.expect("Task panicked");
        assert!(
            matches!(response, Ok(Some(_))),
            "Goto task {} failed: {:?}",
            i,
            response
        );
    }

    let hover_results = join_all(hover_handles).await;
    for (i, result) in hover_results.into_iter().enumerate() {
        let response = result.expect("Task panicked");
        assert!(
            matches!(response, Ok(Some(_))),
            "Hover task {} failed: {:?}",
            i,
            response
        );
    }
}

// ============================================================================
// Race Condition Tests
// ============================================================================

/// Test rapid sequential file changes don't cause issues
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "stress test: exercises repeated workspace rebuilds"]
async fn test_rapid_file_changes() {
    let success_count = Arc::new(AtomicUsize::new(0));

    // Simulate 50 sequential "didChange" events by creating tests with slight variations
    for i in 0..50 {
        let mut test = crate::lsp_test_framework::LspTest::new();

        // Vary the content slightly each iteration
        let content = format!(
            r"
                package com.example;
                public class Test {{
                    // Iteration {}
                    void method{}() {{
                        Us<|>er user = null;
                    }}
                }}
            ",
            i, i
        );

        test.add_file("com/example/Test.java", &content);
        test.add_file(
            "com/example/User.java",
            r"
                package com.example;
                public class User {}
            ",
        );

        if matches!(
            test.goto_definition("com/example/Test.java", "<|>"),
            Ok(Some(_))
        ) {
            success_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    let successes = success_count.load(Ordering::SeqCst);
    eprintln!("Rapid file changes: {} successes out of 50", successes);

    assert_eq!(successes, 50, "Every isolated navigation must resolve");
}

/// Test file change during an ongoing request (simulated)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "stress test: uses isolated workspaces per task"]
async fn test_change_during_request() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    // Launch multiple concurrent operations that simulate change-during-request scenarios
    for i in 0..5 {
        let success = Arc::clone(&success_count);

        handles.push(tokio::spawn(async move {
            // First request
            let mut test1 = crate::lsp_test_framework::LspTest::new();
            test1.add_file(
                "com/example/Main.java",
                r"
                    package com.example;
                    public class Main {
                        Us<|>er user;
                    }
                ",
            );
            test1.add_file(
                "com/example/User.java",
                r"
                    package com.example;
                    public class User { String name; }
                ",
            );

            // Simulate concurrent request (different test instance)
            let mut test2 = crate::lsp_test_framework::LspTest::new();
            test2.add_file(
                "com/example/Main.java",
                r"
                    package com.example;
                    public class Main {
                        // Changed content!
                        Us<|>er user;
                        String extra;
                    }
                ",
            );
            test2.add_file(
                "com/example/User.java",
                r"
                    package com.example;
                    public class User { String name; String email; }
                ",
            );

            // Both should work independently
            let r1 = test1.goto_definition("com/example/Main.java", "<|>");
            let r2 = test2.goto_definition("com/example/Main.java", "<|>");

            if matches!(r1, Ok(Some(_))) && matches!(r2, Ok(Some(_))) {
                success.fetch_add(1, Ordering::SeqCst);
            }
            i
        }));
    }

    let results = join_all(handles).await;

    for result in &results {
        assert!(result.is_ok(), "Task panicked");
    }

    let successes = success_count.load(Ordering::SeqCst);
    eprintln!("Change during request: {} successes out of 5", successes);

    assert_eq!(successes, 5, "Both workspace versions must resolve");
}

// ============================================================================
// Load Tests
// ============================================================================

/// Test 20 different files with parallel navigation
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "stress test: many isolated workspaces"]
async fn test_many_files_parallel() {
    let success_count = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    for i in 0..20 {
        let success = Arc::clone(&success_count);

        handles.push(tokio::spawn(async move {
            let mut test = crate::lsp_test_framework::LspTest::new();

            // Create unique file for this task
            let main_file = format!("com/example/Main{}.java", i);
            let user_file = format!("com/example/User{}.java", i);

            let main_content = format!(
                r"
                    package com.example;
                    public class Main{} {{
                        User{}<|> user;
                    }}
                ",
                i, i
            );

            let user_content = format!(
                r"
                    package com.example;
                    public class User{} {{
                        String name;
                    }}
                ",
                i
            );

            test.add_file(&main_file, &main_content);
            test.add_file(&user_file, &user_content);

            if let Ok(Some(response)) = test.goto_definition(&main_file, "<|>") {
                // Verify we got the right file
                let expected_suffix = format!("User{}.java", i);
                let uri = match response {
                    tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc) => {
                        loc.uri.to_string()
                    }
                    tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs) => {
                        locs.first().map(|l| l.uri.to_string()).unwrap_or_default()
                    }
                    _ => String::new(),
                };

                if uri.ends_with(&expected_suffix) {
                    success.fetch_add(1, Ordering::SeqCst);
                }
            }
            i
        }));
    }

    let results = join_all(handles).await;

    for result in &results {
        assert!(result.is_ok(), "Task panicked");
    }

    let successes = success_count.load(Ordering::SeqCst);
    eprintln!("Many files parallel: {} successes out of 20", successes);

    assert_eq!(successes, 20, "Every isolated workspace must resolve");
}

/// Test memory stability under sustained load (100 iterations)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "stress test: high iteration count"]
async fn test_memory_stability_under_load() {
    let success_count = Arc::new(AtomicUsize::new(0));

    // Run in batches to avoid overwhelming the system
    for batch in 0..10 {
        let mut handles = vec![];

        for i in 0..10 {
            let success = Arc::clone(&success_count);
            let batch_num = batch;

            handles.push(tokio::spawn(async move {
                let mut test = crate::lsp_test_framework::LspTest::new();

                test.add_file(
                    "com/example/Main.java",
                    r"
                        package com.example;
                        public class Main {
                            Us<|>er user;
                        }
                    ",
                );
                test.add_file(
                    "com/example/User.java",
                    r"
                        package com.example;
                        public class User {}
                    ",
                );

                if matches!(
                    test.goto_definition("com/example/Main.java", "<|>"),
                    Ok(Some(_))
                ) {
                    success.fetch_add(1, Ordering::SeqCst);
                }
                (batch_num, i)
            }));
        }

        // Wait for batch to complete before starting next
        let results = join_all(handles).await;
        for result in &results {
            assert!(result.is_ok(), "Task panicked in batch {}", batch);
        }
    }

    let successes = success_count.load(Ordering::SeqCst);
    eprintln!("Memory stability test: {} successes out of 100", successes);

    assert_eq!(successes, 100, "Every sustained-load request must resolve");
}

// ============================================================================
// Synchronization Tests
// ============================================================================

/// Test that all requests start at approximately the same time using a barrier
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "stress test: uses isolated workspaces per thread"]
async fn test_barrier_synchronized_requests() {
    use std::sync::Barrier;
    use std::thread;

    let barrier = Arc::new(Barrier::new(10));
    let success_count = Arc::new(AtomicUsize::new(0));

    // Use threads instead of async tasks to properly utilize the barrier
    let mut handles = vec![];

    for i in 0..10 {
        let barrier = Arc::clone(&barrier);
        let success = Arc::clone(&success_count);

        handles.push(thread::spawn(move || {
            // All threads wait here until all 10 are ready
            barrier.wait();

            // Now all threads proceed simultaneously
            let mut test = crate::lsp_test_framework::LspTest::new();

            test.add_file(
                "com/example/Main.java",
                r"
                    package com.example;
                    public class Main {
                        Us<|>er user;
                    }
                ",
            );
            test.add_file(
                "com/example/User.java",
                r"
                    package com.example;
                    public class User {}
                ",
            );

            if let Ok(Some(_)) = test.goto_definition("com/example/Main.java", "<|>") {
                success.fetch_add(1, Ordering::SeqCst);
            }
            i
        }));
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let successes = success_count.load(Ordering::SeqCst);
    eprintln!(
        "Barrier synchronized requests: {} successes out of 10",
        successes
    );

    assert_eq!(successes, 10, "Every synchronized request must resolve");
}

// ============================================================================
// Completion Concurrency Tests
// ============================================================================

/// Test concurrent completion requests against shared state
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_completion_requests() {
    let shared = Arc::new(build_shared_context());
    let mut handles = vec![];

    for _ in 0..10 {
        let shared = Arc::clone(&shared);
        handles.push(tokio::spawn(async move {
            shared.completion("com/example/Comp.java", "<|>")
        }));
    }

    let results = join_all(handles).await;
    for (i, result) in results.into_iter().enumerate() {
        let response = result.expect("Task panicked");
        assert!(
            matches!(response, Ok(ref items) if !items.is_empty()),
            "Task {}: completion failed: {:?}",
            i,
            response
        );
    }
}

/// Test mixed operations under heavy load
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "stress test: heavy mixed load"]
async fn test_heavy_mixed_load() {
    let goto_count = Arc::new(AtomicUsize::new(0));
    let hover_count = Arc::new(AtomicUsize::new(0));
    let completion_count = Arc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Spawn 30 tasks total: 10 goto, 10 hover, 10 completion
    for i in 0..10 {
        // Goto task
        let goto = Arc::clone(&goto_count);
        handles.push(tokio::spawn(async move {
            let mut test = crate::lsp_test_framework::LspTest::new();
            test.add_file(
                "com/example/Main.java",
                "package com.example; class Main { Us<|>er u; }",
            );
            test.add_file(
                "com/example/User.java",
                "package com.example; class User {}",
            );
            if matches!(
                test.goto_definition("com/example/Main.java", "<|>"),
                Ok(Some(_))
            ) {
                goto.fetch_add(1, Ordering::SeqCst);
            }
            ("goto", i)
        }));

        // Hover task
        let hover = Arc::clone(&hover_count);
        handles.push(tokio::spawn(async move {
            let mut test = crate::lsp_test_framework::LspTest::new();
            test.add_file(
                "com/example/Test.java",
                "package com.example; class Test { void te<|>st() {} }",
            );
            if matches!(test.hover("com/example/Test.java", "<|>"), Ok(Some(_))) {
                hover.fetch_add(1, Ordering::SeqCst);
            }
            ("hover", i)
        }));

        // Completion task
        let comp = Arc::clone(&completion_count);
        handles.push(tokio::spawn(async move {
            let mut test = crate::lsp_test_framework::LspTest::new();
            test.add_file(
                "com/example/Comp.java",
                "package com.example; class Comp { void m() { String s = \"\"; s.<|> } }",
            );
            if let Ok(items) = test.completion("com/example/Comp.java", "<|>") {
                if !items.is_empty() {
                    comp.fetch_add(1, Ordering::SeqCst);
                }
            }
            ("completion", i)
        }));
    }

    let results = join_all(handles).await;

    for result in &results {
        assert!(result.is_ok(), "Task panicked");
    }

    let goto = goto_count.load(Ordering::SeqCst);
    let hover = hover_count.load(Ordering::SeqCst);
    let comp = completion_count.load(Ordering::SeqCst);

    eprintln!(
        "Heavy mixed load: goto={}, hover={}, completion={}",
        goto, hover, comp
    );

    assert_eq!(goto, 10, "Every goto request must resolve");
    assert_eq!(hover, 10, "Every hover request must resolve");
    assert_eq!(comp, 10, "Every completion request must return candidates");
}
