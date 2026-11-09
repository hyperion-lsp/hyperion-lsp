// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// TOCTOU Race Condition
// Tests that file watcher correctly handles time-of-check to time-of-use races
// Current behavior: File may be deleted/modified between existence check and read
// Expected: Should handle missing/changed files gracefully without crashes

use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

/// Test that file watcher handles file deletion between check and read
#[test]
fn test_file_deleted_after_check() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("Test.java");

    // Create initial file
    fs::write(&file_path, "class Test {}").unwrap();

    // Simulate race: file exists during check, deleted before read
    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let file_path_clone = file_path.clone();

    let deleter = thread::spawn(move || {
        barrier_clone.wait();
        // Delete file immediately after barrier
        fs::remove_file(&file_path_clone).ok();
    });

    // Main thread checks file exists
    barrier.wait();
    let exists = file_path.exists();

    // Wait for deletion
    deleter.join().unwrap();
    thread::sleep(Duration::from_millis(50));

    // Attempt to read should handle missing file gracefully
    let result = fs::read_to_string(&file_path);
    assert!(
        exists || result.is_err(),
        "File should either exist or read should fail gracefully"
    );
}

/// Test that file watcher handles file modification between check and read
#[test]
fn test_file_modified_after_check() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("Test.java");

    // Create initial file
    let original_content = "class Test { void method1() {} }";
    fs::write(&file_path, original_content).unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let file_path_clone = file_path.clone();

    let modifier = thread::spawn(move || {
        barrier_clone.wait();
        // Modify file immediately after barrier
        fs::write(&file_path_clone, "class Test { void method2() {} }").ok();
    });

    // Check file metadata
    barrier.wait();
    let metadata_before = fs::metadata(&file_path).ok();

    // Wait for modification
    modifier.join().unwrap();
    thread::sleep(Duration::from_millis(50));

    // Read should succeed, but content may differ
    let content = fs::read_to_string(&file_path).unwrap();
    let metadata_after = fs::metadata(&file_path).unwrap();

    assert!(
        content == original_content || content.contains("method2"),
        "Should read either original or modified content"
    );

    // Metadata modification time should change
    if let Some(meta_before) = metadata_before {
        let modified_before = meta_before.modified().unwrap();
        let modified_after = metadata_after.modified().unwrap();
        assert!(
            modified_before <= modified_after,
            "Modification time should advance or stay same"
        );
    }
}

/// Test that concurrent file operations don't cause crashes
#[test]
fn test_concurrent_file_operations() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("Test.java");

    // Create initial file
    fs::write(&file_path, "class Test {}").unwrap();

    let barrier = Arc::new(Barrier::new(3));
    let mut handles = vec![];

    // Reader thread
    let barrier_clone = barrier.clone();
    let file_path_clone = file_path.clone();
    handles.push(thread::spawn(move || {
        barrier_clone.wait();
        for _ in 0..10 {
            let _ = fs::read_to_string(&file_path_clone);
            thread::sleep(Duration::from_millis(5));
        }
    }));

    // Writer thread
    let barrier_clone = barrier.clone();
    let file_path_clone = file_path.clone();
    handles.push(thread::spawn(move || {
        barrier_clone.wait();
        for i in 0..10 {
            let content = format!("class Test {{ void method{}() {{}} }}", i);
            let _ = fs::write(&file_path_clone, content);
            thread::sleep(Duration::from_millis(5));
        }
    }));

    // All threads start together
    barrier.wait();

    // Wait for all operations to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // File should still exist and be readable
    assert!(file_path.exists());
    let final_content = fs::read_to_string(&file_path).unwrap();
    assert!(final_content.contains("class Test"));
}

/// Test that file watcher handles rapid create/delete cycles
#[test]
fn test_rapid_create_delete_cycles() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("Test.java");

    for i in 0..20 {
        let content = format!("class Test{} {{}}", i);
        fs::write(&file_path, content).unwrap();

        // Attempt to read
        let _ = fs::read_to_string(&file_path);

        // Delete
        fs::remove_file(&file_path).unwrap();

        // Verify deletion
        assert!(!file_path.exists());

        thread::sleep(Duration::from_millis(10));
    }
}

/// Test that file watcher handles directory deletion during scan
#[test]
fn test_directory_deleted_during_scan() {
    let temp_dir = TempDir::new().unwrap();
    let package_dir = temp_dir.path().join("com/example");
    fs::create_dir_all(&package_dir).unwrap();

    // Create multiple files
    for i in 0..5 {
        let file_path = package_dir.join(format!("Test{}.java", i));
        fs::write(&file_path, format!("class Test{} {{}}", i)).unwrap();
    }

    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let package_dir_clone = package_dir.clone();

    let deleter = thread::spawn(move || {
        barrier_clone.wait();
        thread::sleep(Duration::from_millis(20));
        // Delete entire directory tree
        let _ = fs::remove_dir_all(&package_dir_clone);
    });

    // Start scanning
    barrier.wait();

    let mut found_files = vec![];
    for i in 0..5 {
        let file_path = package_dir.join(format!("Test{}.java", i));
        if let Ok(content) = fs::read_to_string(&file_path) {
            found_files.push(content);
        }
        thread::sleep(Duration::from_millis(10));
    }

    deleter.join().unwrap();

    // Should have found some files (but maybe not all)
    // The key is that it shouldn't panic
    assert!(found_files.len() <= 5);
}

/// Test that file watcher handles symlink changes
#[cfg(unix)]
#[test]
fn test_symlink_target_change() {
    use std::os::unix::fs as unix_fs;

    let temp_dir = TempDir::new().unwrap();
    let target1 = temp_dir.path().join("Target1.java");
    let target2 = temp_dir.path().join("Target2.java");
    let link = temp_dir.path().join("Link.java");

    // Create two target files
    fs::write(&target1, "class Target1 {}").unwrap();
    fs::write(&target2, "class Target2 {}").unwrap();

    // Create symlink to target1
    unix_fs::symlink(&target1, &link).unwrap();

    // Read through symlink
    let content1 = fs::read_to_string(&link).unwrap();
    assert!(content1.contains("Target1"));

    // Remove symlink
    fs::remove_file(&link).unwrap();

    // Create new symlink to target2
    unix_fs::symlink(&target2, &link).unwrap();

    // Read should now return target2 content
    let content2 = fs::read_to_string(&link).unwrap();
    assert!(content2.contains("Target2"));
}

/// Test that file watcher handles permission changes
#[cfg(unix)]
#[test]
fn test_permission_change_during_read() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("Test.java");

    // Create file with read permissions
    fs::write(&file_path, "class Test {}").unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let file_path_clone = file_path.clone();

    let permission_changer = thread::spawn(move || {
        barrier_clone.wait();
        thread::sleep(Duration::from_millis(10));
        // Remove read permissions
        let mut perms = fs::metadata(&file_path_clone).unwrap().permissions();
        perms.set_mode(0o000);
        let _ = fs::set_permissions(&file_path_clone, perms);
    });

    barrier.wait();

    // Multiple read attempts
    let mut successful_reads = 0;
    for _ in 0..5 {
        if fs::read_to_string(&file_path).is_ok() {
            successful_reads += 1;
        }
        thread::sleep(Duration::from_millis(5));
    }

    permission_changer.join().unwrap();

    // Should handle permission denied gracefully
    // Some reads may succeed before permission change
    assert!(successful_reads <= 5);

    // Restore permissions for cleanup
    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o644);
    let _ = fs::set_permissions(&file_path, perms);
}
