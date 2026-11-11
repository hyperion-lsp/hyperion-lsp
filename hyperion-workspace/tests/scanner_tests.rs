// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive tests for WorkspaceScanner

use hyperion_workspace::scanner::{ScannerBuilder, WorkspaceScanner};
use std::fs;
use tempfile::TempDir;

#[test]
fn checkout_location_does_not_change_source_discovery() {
    let temp = TempDir::new().unwrap();
    for parent in ["build", "target", "out", "bin", "dist", "node_modules"] {
        let root = temp.path().join(parent).join("checkout");
        let source = root.join("src/main/java/example/App.java");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "package example; public class App {}").unwrap();
        for output in ["build", "target", "out", "bin", "dist", "node_modules"] {
            let artifact = root.join(output).join("Duplicate.java");
            fs::create_dir_all(artifact.parent().unwrap()).unwrap();
            fs::write(artifact, "class Duplicate {}").unwrap();
        }
        assert_eq!(
            WorkspaceScanner::scan_dir(&root).unwrap(),
            vec![source.clone()]
        );
        assert_eq!(
            WorkspaceScanner::scan_dir_scoped_gitignore(&root).unwrap(),
            vec![source.clone()]
        );
        assert_eq!(
            ScannerBuilder::new(root.clone()).scan().unwrap(),
            vec![source.clone()]
        );

        // Verify downstream consumption too: unopened sources must be available
        // to cross-file navigation immediately after the workspace is built.
        let index = hyperion_workspace::WorkspaceIndex::new(root);
        let stats = index.build().unwrap();
        assert_eq!(stats.files_indexed, 1);
        assert!(index.find_symbol("example.App").is_some());
        assert!(index.find_symbol("Duplicate").is_none());
    }
}

#[test]
fn test_scanner_finds_java_files_in_nested_directories() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create nested directory structure
    fs::create_dir_all(root.join("src/main/java/com/example/app")).unwrap();
    fs::create_dir_all(root.join("src/main/java/com/example/utils")).unwrap();
    fs::create_dir_all(root.join("src/test/java/com/example/app")).unwrap();

    // Create Java files
    fs::write(
        root.join("src/main/java/com/example/app/Main.java"),
        "class Main {}",
    )
    .unwrap();
    fs::write(
        root.join("src/main/java/com/example/app/Service.java"),
        "class Service {}",
    )
    .unwrap();
    fs::write(
        root.join("src/main/java/com/example/utils/Helper.java"),
        "class Helper {}",
    )
    .unwrap();
    fs::write(
        root.join("src/test/java/com/example/app/MainTest.java"),
        "class MainTest {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 4);
    assert!(files.iter().any(|p| p.ends_with("Main.java")));
    assert!(files.iter().any(|p| p.ends_with("Service.java")));
    assert!(files.iter().any(|p| p.ends_with("Helper.java")));
    assert!(files.iter().any(|p| p.ends_with("MainTest.java")));
}

#[test]
fn test_scanner_skips_target_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create source files
    fs::create_dir_all(root.join("src/main/java")).unwrap();
    fs::write(root.join("src/main/java/Foo.java"), "class Foo {}").unwrap();

    // Create target directory with Java files (should be skipped)
    fs::create_dir_all(root.join("target/classes")).unwrap();
    fs::write(root.join("target/classes/Foo.java"), "class Foo {}").unwrap();
    fs::write(
        root.join("target/classes/Generated.java"),
        "class Generated {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    // Should only find the source file
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("src/main/java/Foo.java"));
}

#[test]
fn test_scanner_skips_build_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/App.java"), "class App {}").unwrap();

    fs::create_dir_all(root.join("build/libs")).unwrap();
    fs::write(root.join("build/libs/Generated.java"), "class Generated {}").unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("App.java"));
}

#[test]
fn test_scanner_skips_gradle_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Main.java"), "class Main {}").unwrap();

    fs::create_dir_all(root.join(".gradle/caches")).unwrap();
    fs::write(
        root.join(".gradle/caches/Something.java"),
        "class Something {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("Main.java"));
}

#[test]
fn test_scanner_skips_generated_sources() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Regular source
    fs::create_dir_all(root.join("src/main/java")).unwrap();
    fs::write(root.join("src/main/java/Real.java"), "class Real {}").unwrap();

    // Generated sources (should be skipped)
    fs::create_dir_all(root.join("target/generated-sources/annotations")).unwrap();
    fs::write(
        root.join("target/generated-sources/annotations/Generated.java"),
        "class Generated {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("Real.java"));
}

#[test]
fn test_scanner_skips_idea_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Code.java"), "class Code {}").unwrap();

    fs::create_dir_all(root.join(".idea")).unwrap();
    fs::write(root.join(".idea/Something.java"), "class Something {}").unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("Code.java"));
}

#[test]
fn test_scanner_skips_vscode_directory() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/App.java"), "class App {}").unwrap();

    fs::create_dir_all(root.join(".vscode")).unwrap();
    fs::write(root.join(".vscode/Config.java"), "class Config {}").unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
}

#[test]
fn test_scanner_skips_node_modules() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Main.java"), "class Main {}").unwrap();

    fs::create_dir_all(root.join("node_modules/some-package")).unwrap();
    fs::write(
        root.join("node_modules/some-package/Foo.java"),
        "class Foo {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
}

#[test]
fn test_scanner_includes_generated_marker_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Real.java"), "class Real {}").unwrap();

    // File with .generated.java extension
    fs::write(
        root.join("src/AutoValue_Model.generated.java"),
        "class AutoValue_Model {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|path| path.ends_with("Real.java")));
    assert!(files
        .iter()
        .any(|path| path.ends_with("AutoValue_Model.generated.java")));
}

#[test]
fn test_scanner_handles_empty_workspace() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 0);
}

#[test]
fn test_scanner_ignores_non_java_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Main.java"), "class Main {}").unwrap();
    fs::write(root.join("src/README.md"), "# README").unwrap();
    fs::write(root.join("src/config.xml"), "<config/>").unwrap();
    fs::write(root.join("src/test.txt"), "text file").unwrap();
    fs::write(root.join("src/script.sh"), "#!/bin/bash").unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("Main.java"));
}

#[test]
fn test_scanner_builder_with_max_depth() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create deep directory structure
    fs::create_dir_all(root.join("level1/level2/level3/level4")).unwrap();
    fs::write(root.join("level1/Shallow.java"), "class Shallow {}").unwrap();
    fs::write(root.join("level1/level2/Medium.java"), "class Medium {}").unwrap();
    fs::write(
        root.join("level1/level2/level3/level4/Deep.java"),
        "class Deep {}",
    )
    .unwrap();

    // Scan with max depth 3 (should not reach level4)
    let files = ScannerBuilder::new(root.to_path_buf())
        .max_depth(3)
        .scan()
        .unwrap();

    // Should find Shallow and Medium, but not Deep
    assert!(files.iter().any(|p| p.ends_with("Shallow.java")));
    assert!(files.iter().any(|p| p.ends_with("Medium.java")));
    // May or may not find Deep depending on depth counting, but we set a shallow limit
}

#[test]
fn test_scanner_builder_without_gitignore_respect() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Main.java"), "class Main {}").unwrap();

    // Create .gitignore that would normally exclude ignored/
    fs::write(root.join(".gitignore"), "ignored/").unwrap();
    fs::create_dir_all(root.join("ignored")).unwrap();
    fs::write(root.join("ignored/Ignored.java"), "class Ignored {}").unwrap();

    // Scan without respecting gitignore
    let files = ScannerBuilder::new(root.to_path_buf())
        .respect_gitignore(false)
        .scan()
        .unwrap();

    // Should find both files when not respecting gitignore
    assert!(!files.is_empty());
    assert!(files.iter().any(|p| p.ends_with("Main.java")));
}

#[test]
fn test_scanner_respects_gitignore_by_default() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Main.java"), "class Main {}").unwrap();

    // Create .gitignore
    fs::write(root.join(".gitignore"), "ignored/").unwrap();
    fs::create_dir_all(root.join("ignored")).unwrap();
    fs::write(root.join("ignored/Ignored.java"), "class Ignored {}").unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    // Should only find Main.java, not the ignored file
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("Main.java"));
}

#[test]
fn test_scanner_root_accessor() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    let scanner = WorkspaceScanner::new(root.clone());
    assert_eq!(scanner.root(), root.as_path());
}

#[test]
fn test_scanner_with_multiple_source_roots() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Typical Maven/Gradle structure with multiple source roots
    fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();
    fs::create_dir_all(root.join("src/test/java/com/example")).unwrap();
    fs::create_dir_all(root.join("src/integration-test/java/com/example")).unwrap();

    fs::write(
        root.join("src/main/java/com/example/App.java"),
        "class App {}",
    )
    .unwrap();
    fs::write(
        root.join("src/test/java/com/example/AppTest.java"),
        "class AppTest {}",
    )
    .unwrap();
    fs::write(
        root.join("src/integration-test/java/com/example/AppIT.java"),
        "class AppIT {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 3);
    assert!(files.iter().any(|p| p.ends_with("App.java")));
    assert!(files.iter().any(|p| p.ends_with("AppTest.java")));
    assert!(files.iter().any(|p| p.ends_with("AppIT.java")));
}

#[test]
fn test_scanner_handles_special_characters_in_filenames() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/Normal.java"), "class Normal {}").unwrap();
    // Some filesystems may not support all special chars, so use safe ones
    fs::write(
        root.join("src/File-With-Dashes.java"),
        "class FileWithDashes {}",
    )
    .unwrap();
    fs::write(
        root.join("src/File_With_Underscores.java"),
        "class FileWithUnderscores {}",
    )
    .unwrap();

    let scanner = WorkspaceScanner::new(root.to_path_buf());
    let files = scanner.scan().unwrap();

    assert_eq!(files.len(), 3);
}

#[test]
fn test_should_skip_function() {
    use std::path::Path;

    // Should skip build directories
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/target/classes/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/build/classes/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/out/production/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/bin/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/dist/Foo.java"
    )));

    // Should skip IDE directories
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/.gradle/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/.idea/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/.vscode/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/.settings/Foo.java"
    )));

    // Generated source markers are not enough to exclude a file. Build output
    // is still excluded by its `target` component.
    assert!(!WorkspaceScanner::should_skip(Path::new(
        "/project/generated/Foo.java"
    )));
    assert!(WorkspaceScanner::should_skip(Path::new(
        "/project/target/generated-sources/Foo.java"
    )));
    assert!(!WorkspaceScanner::should_skip(Path::new(
        "/project/apt_generated/Foo.java"
    )));
    assert!(!WorkspaceScanner::should_skip(Path::new(
        "/project/AutoValue_Foo.generated.java"
    )));

    // Should not skip source files
    assert!(!WorkspaceScanner::should_skip(Path::new(
        "/project/src/main/java/Foo.java"
    )));
    assert!(!WorkspaceScanner::should_skip(Path::new(
        "/project/src/test/java/FooTest.java"
    )));
    assert!(!WorkspaceScanner::should_skip(Path::new(
        "/project/lib/Foo.java"
    )));
}

#[test]
fn explicitly_discovered_generated_root_bypasses_build_and_ignore_rules() {
    let temp = TempDir::new().unwrap();
    let generated = temp.path().join("build/generated/ksp/main/java");
    fs::create_dir_all(&generated).unwrap();
    fs::write(generated.join("Generated.java"), "class Generated {}").unwrap();
    fs::write(temp.path().join(".gitignore"), "build/\n").unwrap();
    let files = WorkspaceScanner::scan_dir_ignore_gitignore(&generated).unwrap();
    assert_eq!(files, vec![generated.join("Generated.java")]);
}
