// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Integration tests for the entire workspace indexing workflow

use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::query::QueryEngine;
use hyperion_workspace::scanner::WorkspaceScanner;
use std::fs;
use tempfile::TempDir;

#[tokio::test]
async fn test_end_to_end_workspace_indexing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create a realistic Java workspace structure
    fs::create_dir_all(root.join("src/main/java/com/acme/models")).unwrap();
    fs::create_dir_all(root.join("src/main/java/com/acme/services")).unwrap();
    fs::create_dir_all(root.join("src/main/java/com/acme/controllers")).unwrap();
    fs::create_dir_all(root.join("src/main/java/com/acme/utils")).unwrap();

    // Create model files
    fs::write(
        root.join("src/main/java/com/acme/models/User.java"),
        r#"
package com.acme.models;

public class User {
    private String name;
    private String email;
}
"#,
    )
    .unwrap();

    fs::write(
        root.join("src/main/java/com/acme/models/Event.java"),
        r#"
package com.acme.models;

public class Event {
    private String title;
    private User organizer;
}
"#,
    )
    .unwrap();

    // Create service files
    fs::write(
        root.join("src/main/java/com/acme/services/UserService.java"),
        r#"
package com.acme.services;

import com.acme.models.User;
import com.acme.utils.ValidationUtil;

public class UserService {
    public User getUser(String id) {
        return null;
    }
}
"#,
    )
    .unwrap();

    fs::write(
        root.join("src/main/java/com/acme/services/EventService.java"),
        r#"
package com.acme.services;

import com.acme.models.Event;
import com.acme.models.User;
import java.util.List;

public class EventService {
    public List<Event> getEventsForUser(User user) {
        return null;
    }
}
"#,
    )
    .unwrap();

    // Create controller files
    fs::write(
        root.join("src/main/java/com/acme/controllers/UserController.java"),
        r#"
package com.acme.controllers;

import com.acme.services.UserService;
import com.acme.models.User;

public class UserController {
    private UserService userService;
}
"#,
    )
    .unwrap();

    // Create utility files
    fs::write(
        root.join("src/main/java/com/acme/utils/ValidationUtil.java"),
        r#"
package com.acme.utils;

public class ValidationUtil {
    public static boolean isValid(String str) {
        return str != null && !str.isEmpty();
    }
}
"#,
    )
    .unwrap();

    // Step 1: Scan workspace
    let scanner = WorkspaceScanner::new(root.clone());
    let files = scanner.scan().unwrap();
    assert_eq!(files.len(), 6);

    // Step 2: Build index
    let index = WorkspaceIndex::new(root.clone());
    let stats = index.build().unwrap();

    assert_eq!(stats.files_scanned, 6);
    assert_eq!(stats.files_indexed, 6);
    // 6 classes + 5 fields + 3 methods = 14 symbols
    assert_eq!(stats.symbols_indexed, 14);
    assert_eq!(stats.files_failed, 0);

    // Step 3: Verify all symbols are indexed
    assert!(index.find_symbol("com.acme.models.User").is_some());
    assert!(index.find_symbol("com.acme.models.Event").is_some());
    assert!(index.find_symbol("com.acme.services.UserService").is_some());
    assert!(index
        .find_symbol("com.acme.services.EventService")
        .is_some());
    assert!(index
        .find_symbol("com.acme.controllers.UserController")
        .is_some());
    assert!(index.find_symbol("com.acme.utils.ValidationUtil").is_some());

    // Step 4: Test cross-file navigation with QueryEngine
    let query_engine = QueryEngine::new(index);

    // Navigate from UserService to User
    let user_service_file = root.join("src/main/java/com/acme/services/UserService.java");
    let user_service_id = query_engine
        .index()
        .get_file_id(&user_service_file)
        .unwrap();
    let user_locations = query_engine
        .goto_definition("User", user_service_id)
        .unwrap();
    assert!(!user_locations.is_empty());
    assert_eq!(&*user_locations[0].name, "User");

    // Navigate from UserService to ValidationUtil
    let validation_locations = query_engine
        .goto_definition("ValidationUtil", user_service_id)
        .unwrap();
    assert!(!validation_locations.is_empty());

    // Test workspace symbol search
    let service_symbols = query_engine.workspace_symbols("Service").unwrap();
    assert!(service_symbols.len() >= 2); // UserService and EventService

    let user_symbols = query_engine.workspace_symbols("User").unwrap();
    assert!(user_symbols.len() >= 3); // User, UserService, UserController
}

#[tokio::test]
async fn test_workspace_modification_workflow() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Initial file
    let file_path = root.join("src/Service.java");
    fs::write(&file_path, "package com.example;\npublic class Service {}").unwrap();

    // Build index
    let index = WorkspaceIndex::new(root.clone());
    index.index_file(&file_path).await.unwrap();

    assert!(index.find_symbol("com.example.Service").is_some());

    // Simulate file modification
    fs::write(
        &file_path,
        "package com.example;\npublic class UpdatedService {}",
    )
    .unwrap();

    // Re-index
    index.index_file(&file_path).await.unwrap();

    // New symbol should be available
    assert!(index.find_symbol("com.example.UpdatedService").is_some());

    // Simulate file deletion
    let file_id = index.get_file_id(&file_path).unwrap();
    index.remove_file(file_id);

    // Symbols should be gone
    assert!(index.find_symbol("com.example.Service").is_none());
    assert!(index.find_symbol("com.example.UpdatedService").is_none());
}

#[tokio::test]
async fn test_large_workspace_indexing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();

    // Create 100 files
    for i in 0..100 {
        let file_path = root.join(format!("src/main/java/com/example/Class{}.java", i));
        fs::write(
            &file_path,
            format!("package com.example;\npublic class Class{} {{}}", i),
        )
        .unwrap();
    }

    // Build index
    let index = WorkspaceIndex::new(root);
    let stats = index.build().unwrap();

    assert_eq!(stats.files_scanned, 100);
    assert_eq!(stats.files_indexed, 100);
    assert_eq!(stats.symbols_indexed, 100);

    // Verify random symbols
    assert!(index.find_symbol("com.example.Class0").is_some());
    assert!(index.find_symbol("com.example.Class50").is_some());
    assert!(index.find_symbol("com.example.Class99").is_some());
}

#[tokio::test]
async fn test_multi_module_project() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create multi-module structure
    fs::create_dir_all(root.join("module-a/src/main/java/com/example/a")).unwrap();
    fs::create_dir_all(root.join("module-b/src/main/java/com/example/b")).unwrap();
    fs::create_dir_all(root.join("module-c/src/main/java/com/example/c")).unwrap();

    // Module A
    fs::write(
        root.join("module-a/src/main/java/com/example/a/ServiceA.java"),
        "package com.example.a;\npublic class ServiceA {}",
    )
    .unwrap();

    // Module B
    fs::write(
        root.join("module-b/src/main/java/com/example/b/ServiceB.java"),
        "package com.example.b;\nimport com.example.a.ServiceA;\npublic class ServiceB {}",
    )
    .unwrap();

    // Module C
    fs::write(
        root.join("module-c/src/main/java/com/example/c/ServiceC.java"),
        "package com.example.c;\nimport com.example.a.ServiceA;\nimport com.example.b.ServiceB;\npublic class ServiceC {}",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root.clone());
    let stats = index.build().unwrap();

    assert_eq!(stats.files_scanned, 3);
    assert_eq!(stats.symbols_indexed, 3);

    // Test cross-module navigation
    let query_engine = QueryEngine::new(index);

    let service_b_file = root.join("module-b/src/main/java/com/example/b/ServiceB.java");
    let service_b_id = query_engine.index().get_file_id(&service_b_file).unwrap();

    let service_a_locations = query_engine
        .goto_definition("ServiceA", service_b_id)
        .unwrap();
    assert!(!service_a_locations.is_empty());
}

#[tokio::test]
async fn test_workspace_with_build_artifacts() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create source files
    fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();
    fs::write(
        root.join("src/main/java/com/example/App.java"),
        "package com.example;\npublic class App {}",
    )
    .unwrap();

    // Create build artifacts (should be ignored)
    fs::create_dir_all(root.join("target/classes/com/example")).unwrap();
    fs::write(
        root.join("target/classes/com/example/App.java"),
        "package com.example;\npublic class App {}",
    )
    .unwrap();

    fs::create_dir_all(root.join("build/classes/com/example")).unwrap();
    fs::write(
        root.join("build/classes/com/example/Generated.java"),
        "package com.example;\npublic class Generated {}",
    )
    .unwrap();

    // Build index
    let index = WorkspaceIndex::new(root);
    let stats = index.build().unwrap();

    // Should only index source file, not build artifacts
    assert_eq!(stats.files_scanned, 1);
    assert_eq!(stats.files_indexed, 1);
    assert_eq!(stats.symbols_indexed, 1);
}

#[tokio::test]
async fn test_workspace_symbols_ranking() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Create files with similar names
    fs::write(
        root.join("src/Service.java"),
        "package com.example;\npublic class Service {}",
    )
    .unwrap();

    fs::write(
        root.join("src/UserService.java"),
        "package com.example;\npublic class UserService {}",
    )
    .unwrap();

    fs::write(
        root.join("src/ServiceImpl.java"),
        "package com.example;\npublic class ServiceImpl {}",
    )
    .unwrap();

    fs::write(
        root.join("src/ServiceFactory.java"),
        "package com.example;\npublic class ServiceFactory {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    index.build().unwrap();

    let query_engine = QueryEngine::new(index);
    let symbols = query_engine.workspace_symbols("Service").unwrap();

    // All four should be found
    assert_eq!(symbols.len(), 4);
}

#[tokio::test]
async fn test_incremental_indexing() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Start with two files
    let file1 = root.join("src/Class1.java");
    let file2 = root.join("src/Class2.java");

    fs::write(&file1, "package com.example;\npublic class Class1 {}").unwrap();
    fs::write(&file2, "package com.example;\npublic class Class2 {}").unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    assert_eq!(index.stats().symbols_indexed, 2);

    // Add a new file
    let file3 = root.join("src/Class3.java");
    fs::write(&file3, "package com.example;\npublic class Class3 {}").unwrap();

    // Index just the new file
    index.index_file(&file3).await.unwrap();

    assert_eq!(index.stats().symbols_indexed, 3);
    assert!(index.find_symbol("com.example.Class3").is_some());
}

#[tokio::test]
async fn test_goto_definition_chain() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Create a chain of dependencies: Controller -> Service -> Repository -> Entity

    fs::write(
        root.join("src/Entity.java"),
        "package com.example;\npublic class Entity {}",
    )
    .unwrap();

    fs::write(
        root.join("src/Repository.java"),
        "package com.example;\nimport com.example.Entity;\npublic class Repository {}",
    )
    .unwrap();

    fs::write(
        root.join("src/Service.java"),
        "package com.example;\nimport com.example.Repository;\npublic class Service {}",
    )
    .unwrap();

    fs::write(
        root.join("src/Controller.java"),
        "package com.example;\nimport com.example.Service;\npublic class Controller {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    let query_engine = QueryEngine::new(index);

    // Test navigation from Controller to Service
    let controller_file = root.join("src/Controller.java");
    let controller_id = query_engine.index().get_file_id(&controller_file).unwrap();
    let service_locations = query_engine
        .goto_definition("Service", controller_id)
        .unwrap();
    assert_eq!(service_locations.len(), 1);

    // Test navigation from Service to Repository
    let service_file = root.join("src/Service.java");
    let service_id = query_engine.index().get_file_id(&service_file).unwrap();
    let repo_locations = query_engine
        .goto_definition("Repository", service_id)
        .unwrap();
    assert_eq!(repo_locations.len(), 1);

    // Test navigation from Repository to Entity
    let repo_file = root.join("src/Repository.java");
    let repo_id = query_engine.index().get_file_id(&repo_file).unwrap();
    let entity_locations = query_engine.goto_definition("Entity", repo_id).unwrap();
    assert_eq!(entity_locations.len(), 1);
}

#[tokio::test]
async fn test_workspace_with_test_sources() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    // Create main and test sources
    fs::create_dir_all(root.join("src/main/java/com/example")).unwrap();
    fs::create_dir_all(root.join("src/test/java/com/example")).unwrap();

    fs::write(
        root.join("src/main/java/com/example/Service.java"),
        "package com.example;\npublic class Service {}",
    )
    .unwrap();

    fs::write(
        root.join("src/test/java/com/example/ServiceTest.java"),
        "package com.example;\nimport com.example.Service;\npublic class ServiceTest {}",
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    let stats = index.build().unwrap();

    // Both main and test sources should be indexed
    assert_eq!(stats.files_scanned, 2);
    assert_eq!(stats.symbols_indexed, 2);

    // Test navigation from test to main source
    let query_engine = QueryEngine::new(index);
    let test_file = root.join("src/test/java/com/example/ServiceTest.java");
    let test_id = query_engine.index().get_file_id(&test_file).unwrap();

    let service_locations = query_engine.goto_definition("Service", test_id).unwrap();
    assert!(!service_locations.is_empty());
}

#[tokio::test]
async fn test_complex_import_resolution() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Create classes with overlapping names in different packages
    fs::write(
        root.join("src/Helper1.java"),
        "package com.util.a;\npublic class Helper {}",
    )
    .unwrap();

    fs::write(
        root.join("src/Helper2.java"),
        "package com.util.b;\npublic class Helper {}",
    )
    .unwrap();

    fs::write(
        root.join("src/User.java"),
        r#"
package com.example;

import com.util.a.Helper;
import com.util.b.*;

public class User {}
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root.clone());
    index.build().unwrap();

    let query_engine = QueryEngine::new(index);

    let user_file = root.join("src/User.java");
    let user_id = query_engine.index().get_file_id(&user_file).unwrap();

    // Helper should resolve to com.util.a.Helper (explicit import takes precedence)
    let helper_locations = query_engine.goto_definition("Helper", user_id).unwrap();
    assert_eq!(helper_locations.len(), 1);
}

#[tokio::test]
async fn test_workspace_statistics() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    fs::create_dir_all(root.join("src")).unwrap();

    // Create various types of symbols
    fs::write(
        root.join("src/Types.java"),
        r#"
package com.example;

public class MyClass {}
interface MyInterface {}
enum MyEnum { A, B }
"#,
    )
    .unwrap();

    let index = WorkspaceIndex::new(root);
    let build_stats = index.build().unwrap();

    assert_eq!(build_stats.files_scanned, 1);
    assert_eq!(build_stats.files_indexed, 1);
    // 3 types (MyClass, MyInterface, MyEnum) + 2 enum members (A, B) + implicit enum methods
    // The symbol extractor also creates synthetic enum members and methods
    assert_eq!(build_stats.symbols_indexed, 9);

    let index_stats = index.stats();
    assert_eq!(index_stats.symbols_indexed, 9);
}
