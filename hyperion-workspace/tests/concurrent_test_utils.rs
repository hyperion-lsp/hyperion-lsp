// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test utilities for concurrent operations and race condition testing
//!
//! Provides helpers for:
//! - Spawning concurrent index operations
//! - Detecting data races
//! - Verifying thread safety
//! - Checking for memory leaks

use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::timeout;

/// Helper for running concurrent operations and detecting races
pub struct ConcurrentTestHarness {
    task_count: usize,
    timeout_ms: u64,
    operation_count: Arc<AtomicUsize>,
    error_count: Arc<AtomicUsize>,
    completed: Arc<AtomicBool>,
}

impl ConcurrentTestHarness {
    /// Create a new concurrent test harness
    pub fn new(task_count: usize) -> Self {
        Self {
            task_count,
            timeout_ms: 30_000,
            operation_count: Arc::new(AtomicUsize::new(0)),
            error_count: Arc::new(AtomicUsize::new(0)),
            completed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set timeout in milliseconds
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    /// Run concurrent operations with the given closure
    ///
    /// Each task will execute the closure independently. The closure receives
    /// a task ID (0 to task_count-1) and should return a Result.
    pub async fn run<F, Fut, T>(&self, operation: F) -> ConcurrentTestResult<T>
    where
        F: Fn(usize) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send,
        T: Send + 'static,
    {
        let operation = Arc::new(operation);
        let mut join_set = JoinSet::new();

        let operation_count = self.operation_count.clone();
        let error_count = self.error_count.clone();

        for task_id in 0..self.task_count {
            let operation = operation.clone();
            let operation_count = operation_count.clone();
            let error_count = error_count.clone();

            join_set.spawn(async move {
                let result = operation(task_id).await;
                operation_count.fetch_add(1, Ordering::SeqCst);

                if result.is_err() {
                    error_count.fetch_add(1, Ordering::SeqCst);
                }

                (task_id, result)
            });
        }

        let timeout_duration = Duration::from_millis(self.timeout_ms);
        let results_future = async {
            let mut results = Vec::new();
            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((task_id, task_result)) => results.push((task_id, task_result)),
                    Err(e) => {
                        error_count.fetch_add(1, Ordering::SeqCst);
                        results.push((usize::MAX, Err(anyhow::anyhow!("Task panicked: {}", e))));
                    }
                }
            }
            results
        };

        let results = match timeout(timeout_duration, results_future).await {
            Ok(results) => results,
            Err(_) => {
                return ConcurrentTestResult {
                    results: Vec::new(),
                    total_operations: operation_count.load(Ordering::SeqCst),
                    error_count: error_count.load(Ordering::SeqCst),
                    timed_out: true,
                }
            }
        };

        self.completed.store(true, Ordering::SeqCst);

        ConcurrentTestResult {
            results,
            total_operations: operation_count.load(Ordering::SeqCst),
            error_count: error_count.load(Ordering::SeqCst),
            timed_out: false,
        }
    }

    /// Check if all tasks completed
    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::SeqCst)
    }

    /// Get current operation count
    pub fn operation_count(&self) -> usize {
        self.operation_count.load(Ordering::SeqCst)
    }

    /// Get current error count
    pub fn error_count(&self) -> usize {
        self.error_count.load(Ordering::SeqCst)
    }
}

impl Default for ConcurrentTestHarness {
    fn default() -> Self {
        Self::new(10)
    }
}

/// Results from a concurrent test run
pub struct ConcurrentTestResult<T> {
    pub results: Vec<(usize, Result<T>)>,
    pub total_operations: usize,
    pub error_count: usize,
    pub timed_out: bool,
}

impl<T> ConcurrentTestResult<T> {
    /// Assert that no errors occurred
    pub fn assert_no_errors(&self) -> Result<()> {
        if self.timed_out {
            anyhow::bail!("Test timed out");
        }

        if self.error_count > 0 {
            let error_messages: Vec<String> = self
                .results
                .iter()
                .filter_map(|(task_id, result)| {
                    result
                        .as_ref()
                        .err()
                        .map(|e| format!("Task {}: {}", task_id, e))
                })
                .collect();

            anyhow::bail!(
                "{} errors occurred:\n{}",
                self.error_count,
                error_messages.join("\n")
            );
        }

        Ok(())
    }

    /// Get all successful results
    pub fn successful_results(&self) -> Vec<&T> {
        self.results
            .iter()
            .filter_map(|(_, result)| result.as_ref().ok())
            .collect()
    }

    /// Get all errors
    pub fn errors(&self) -> Vec<&anyhow::Error> {
        self.results
            .iter()
            .filter_map(|(_, result)| result.as_ref().err())
            .collect()
    }

    /// Check if all tasks succeeded
    pub fn all_succeeded(&self) -> bool {
        !self.timed_out && self.error_count == 0
    }

    /// Get success rate (0.0 to 1.0)
    pub fn success_rate(&self) -> f64 {
        if self.total_operations == 0 {
            return 0.0;
        }
        let successes = self.total_operations - self.error_count;
        successes as f64 / self.total_operations as f64
    }
}

/// Helper for stress testing with high concurrency
pub async fn stress_test<F, Fut, T>(
    name: &str,
    task_count: usize,
    iterations_per_task: usize,
    operation: F,
) -> Result<()>
where
    F: Fn(usize, usize) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<T>> + Send,
    T: Send + 'static,
{
    tracing::info!(
        "Starting stress test '{}' with {} tasks and {} iterations per task",
        name,
        task_count,
        iterations_per_task
    );

    let harness = ConcurrentTestHarness::new(task_count).with_timeout_ms(60_000);
    let operation = Arc::new(operation);

    let result = harness
        .run(move |task_id| {
            let operation = operation.clone();
            let iterations = iterations_per_task;
            async move {
                for iteration in 0..iterations {
                    operation(task_id, iteration).await?;
                }
                Ok(())
            }
        })
        .await;

    result.assert_no_errors()?;

    tracing::info!(
        "Stress test '{}' completed: {} operations, {} errors",
        name,
        result.total_operations,
        result.error_count
    );

    Ok(())
}

/// Race condition detector that tries to trigger races by controlled timing
pub struct RaceDetector {
    barrier: Arc<tokio::sync::Barrier>,
    task_count: usize,
}

impl RaceDetector {
    /// Create a new race detector
    pub fn new(task_count: usize) -> Self {
        Self {
            barrier: Arc::new(tokio::sync::Barrier::new(task_count)),
            task_count,
        }
    }

    /// Run tasks that all start at the same time to maximize race potential
    pub async fn run_synchronized<F, Fut, T>(&self, operation: F) -> ConcurrentTestResult<T>
    where
        F: Fn(usize) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send,
        T: Send + 'static,
    {
        let operation = Arc::new(operation);
        let mut join_set = JoinSet::new();

        for task_id in 0..self.task_count {
            let operation = operation.clone();
            let barrier = self.barrier.clone();

            join_set.spawn(async move {
                barrier.wait().await;
                (task_id, operation(task_id).await)
            });
        }

        let mut results = Vec::new();
        let mut error_count = 0;

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((task_id, task_result)) => {
                    if task_result.is_err() {
                        error_count += 1;
                    }
                    results.push((task_id, task_result));
                }
                Err(e) => {
                    error_count += 1;
                    results.push((usize::MAX, Err(anyhow::anyhow!("Task panicked: {}", e))));
                }
            }
        }

        ConcurrentTestResult {
            total_operations: results.len(),
            error_count,
            results,
            timed_out: false,
        }
    }
}

/// Memory leak detector - tracks allocations that should be cleaned up
pub struct MemoryLeakDetector {
    initial_count: Arc<AtomicUsize>,
    current_count: Arc<AtomicUsize>,
}

impl MemoryLeakDetector {
    /// Create a new memory leak detector
    pub fn new() -> Self {
        Self {
            initial_count: Arc::new(AtomicUsize::new(0)),
            current_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Record allocation
    pub fn allocate(&self) {
        self.current_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Record deallocation
    pub fn deallocate(&self) {
        self.current_count.fetch_sub(1, Ordering::SeqCst);
    }

    /// Take snapshot of current allocation count
    pub fn snapshot(&self) {
        let current = self.current_count.load(Ordering::SeqCst);
        self.initial_count.store(current, Ordering::SeqCst);
    }

    /// Check for leaks since last snapshot
    pub fn check_leaks(&self) -> Result<()> {
        let initial = self.initial_count.load(Ordering::SeqCst);
        let current = self.current_count.load(Ordering::SeqCst);

        if current > initial {
            anyhow::bail!(
                "Memory leak detected: {} allocations not freed (initial: {}, current: {})",
                current - initial,
                initial,
                current
            );
        }

        Ok(())
    }

    /// Get current allocation count
    pub fn current_count(&self) -> usize {
        self.current_count.load(Ordering::SeqCst)
    }
}

impl Default for MemoryLeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_concurrent_harness_success() {
        let harness = ConcurrentTestHarness::new(10);

        let result = harness
            .run(|task_id| async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(task_id * 2)
            })
            .await;

        assert!(result.all_succeeded());
        assert_eq!(result.total_operations, 10);
        assert_eq!(result.error_count, 0);
    }

    #[tokio::test]
    async fn test_concurrent_harness_with_errors() {
        let harness = ConcurrentTestHarness::new(10);

        let result = harness
            .run(|task_id| async move {
                if task_id % 2 == 0 {
                    Ok(task_id)
                } else {
                    anyhow::bail!("Task {} failed", task_id)
                }
            })
            .await;

        assert!(!result.all_succeeded());
        assert_eq!(result.error_count, 5);
        assert_eq!(result.success_rate(), 0.5);
    }

    #[tokio::test]
    async fn test_race_detector() {
        let detector = RaceDetector::new(5);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = detector
            .run_synchronized(move |_task_id| {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await;

        assert!(result.all_succeeded());
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn test_memory_leak_detector() {
        let detector = MemoryLeakDetector::new();

        detector.snapshot();
        detector.allocate();
        detector.allocate();
        detector.allocate();

        assert!(detector.check_leaks().is_err());

        detector.deallocate();
        detector.deallocate();
        detector.deallocate();

        assert!(detector.check_leaks().is_ok());
    }

    #[tokio::test]
    async fn test_stress_test() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let result = stress_test("counter_increment", 10, 100, move |_task_id, _iteration| {
            let counter = counter_clone.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 1000);
    }
}
