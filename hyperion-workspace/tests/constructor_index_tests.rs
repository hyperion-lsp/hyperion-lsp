// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Constructor Index Cleanup
// Tests that constructor entries in symbol index are properly cleaned up when files change
// Current behavior: Old constructor signatures may persist in index after file modification
// Expected: Should remove old constructors and replace with new ones on file update

use std::collections::HashMap;

/// Mock structure representing constructor index
#[derive(Debug, Clone, PartialEq)]
struct ConstructorSignature {
    class_name: String,
    parameters: Vec<String>,
}

struct ConstructorIndex {
    /// Maps class FQN -> list of constructor signatures
    constructors: HashMap<String, Vec<ConstructorSignature>>,
    /// Maps file_id -> classes defined in that file
    file_classes: HashMap<usize, Vec<String>>,
}

impl ConstructorIndex {
    fn new() -> Self {
        Self {
            constructors: HashMap::new(),
            file_classes: HashMap::new(),
        }
    }

    fn add_constructor(
        &mut self,
        file_id: usize,
        class_fqn: String,
        signature: ConstructorSignature,
    ) {
        self.file_classes
            .entry(file_id)
            .or_default()
            .push(class_fqn.clone());

        self.constructors
            .entry(class_fqn)
            .or_default()
            .push(signature);
    }

    fn update_file(
        &mut self,
        file_id: usize,
        new_constructors: Vec<(String, ConstructorSignature)>,
    ) {
        // Remove old constructors for this file
        if let Some(classes) = self.file_classes.remove(&file_id) {
            for class in classes {
                self.constructors.remove(&class);
            }
        }

        // Add new constructors
        for (class_fqn, signature) in new_constructors {
            self.add_constructor(file_id, class_fqn, signature);
        }
    }

    fn get_constructors(&self, class_fqn: &str) -> Option<&Vec<ConstructorSignature>> {
        self.constructors.get(class_fqn)
    }

    fn total_classes(&self) -> usize {
        self.constructors.len()
    }

    fn total_constructors(&self) -> usize {
        self.constructors.values().map(|v| v.len()).sum()
    }
}

#[test]
fn test_constructor_cleanup_on_update() {
    let mut index = ConstructorIndex::new();

    // Initial version: User(String name)
    let sig1 = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string()],
    };
    index.add_constructor(1, "com.example.User".to_string(), sig1.clone());

    assert_eq!(index.total_constructors(), 1);
    assert_eq!(index.get_constructors("com.example.User").unwrap().len(), 1);

    // Update: User(String name, int age)
    let sig2 = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string(), "int".to_string()],
    };
    index.update_file(1, vec![("com.example.User".to_string(), sig2.clone())]);

    // Should have only the new constructor
    assert_eq!(index.total_constructors(), 1);
    let constructors = index.get_constructors("com.example.User").unwrap();
    assert_eq!(constructors.len(), 1);
    assert_eq!(constructors[0], sig2);
}

#[test]
fn test_multiple_constructors_cleanup() {
    let mut index = ConstructorIndex::new();

    // Initial version: 3 constructors
    let sig1 = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec![],
    };
    let sig2 = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string()],
    };
    let sig3 = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string(), "int".to_string()],
    };

    index.add_constructor(1, "com.example.User".to_string(), sig1);
    index.add_constructor(1, "com.example.User".to_string(), sig2);
    index.add_constructor(1, "com.example.User".to_string(), sig3);

    assert_eq!(index.total_constructors(), 3);

    // Update: only default constructor now
    let new_sig = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec![],
    };
    index.update_file(1, vec![("com.example.User".to_string(), new_sig.clone())]);

    // Should have only 1 constructor
    assert_eq!(index.total_constructors(), 1);
    let constructors = index.get_constructors("com.example.User").unwrap();
    assert_eq!(constructors.len(), 1);
    assert_eq!(constructors[0], new_sig);
}

#[test]
fn test_constructor_cleanup_multiple_classes() {
    let mut index = ConstructorIndex::new();

    // File defines two classes
    let user_sig = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string()],
    };
    let product_sig = ConstructorSignature {
        class_name: "Product".into(),
        parameters: vec!["String".to_string()],
    };

    index.add_constructor(1, "com.example.User".to_string(), user_sig);
    index.add_constructor(1, "com.example.Product".to_string(), product_sig);

    assert_eq!(index.total_classes(), 2);
    assert_eq!(index.total_constructors(), 2);

    // Update: now only defines User
    let new_user_sig = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string(), "int".to_string()],
    };
    index.update_file(1, vec![("com.example.User".to_string(), new_user_sig)]);

    // Product should be removed
    assert_eq!(index.total_classes(), 1);
    assert_eq!(index.get_constructors("com.example.Product"), None);
    assert!(index.get_constructors("com.example.User").is_some());
}

#[test]
fn test_constructor_cleanup_inner_class() {
    let mut index = ConstructorIndex::new();

    // Outer class constructor
    let outer_sig = ConstructorSignature {
        class_name: "Outer".into(),
        parameters: vec![],
    };
    // Inner class constructor
    let inner_sig = ConstructorSignature {
        class_name: "Inner".into(),
        parameters: vec!["String".to_string()],
    };

    index.add_constructor(1, "com.example.Outer".to_string(), outer_sig);
    index.add_constructor(1, "com.example.Outer.Inner".to_string(), inner_sig);

    assert_eq!(index.total_classes(), 2);

    // Update: remove inner class
    let new_outer_sig = ConstructorSignature {
        class_name: "Outer".into(),
        parameters: vec![],
    };
    index.update_file(1, vec![("com.example.Outer".to_string(), new_outer_sig)]);

    assert_eq!(index.total_classes(), 1);
    assert_eq!(index.get_constructors("com.example.Outer.Inner"), None);
}

#[test]
fn test_constructor_cleanup_no_constructors() {
    let mut index = ConstructorIndex::new();

    // Initial: has constructor
    let sig = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string()],
    };
    index.add_constructor(1, "com.example.User".to_string(), sig);

    assert_eq!(index.total_constructors(), 1);

    // Update: no constructors (maybe became interface)
    index.update_file(1, vec![]);

    assert_eq!(index.total_constructors(), 0);
    assert_eq!(index.get_constructors("com.example.User"), None);
}

#[test]
fn test_constructor_cleanup_generic_constructor() {
    let mut index = ConstructorIndex::new();

    // Initial: Container<T>()
    let sig1 = ConstructorSignature {
        class_name: "Container".into(),
        parameters: vec![],
    };
    index.add_constructor(1, "com.example.Container".to_string(), sig1);

    // Update: Container<T>(T value)
    let sig2 = ConstructorSignature {
        class_name: "Container".into(),
        parameters: vec!["T".to_string()],
    };
    index.update_file(1, vec![("com.example.Container".to_string(), sig2.clone())]);

    let constructors = index.get_constructors("com.example.Container").unwrap();
    assert_eq!(constructors.len(), 1);
    assert_eq!(constructors[0], sig2);
}

#[test]
fn test_constructor_cleanup_varargs() {
    let mut index = ConstructorIndex::new();

    // Initial: User(String... names)
    let sig1 = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String...".to_string()],
    };
    index.add_constructor(1, "com.example.User".to_string(), sig1);

    // Update: User(String name, String... aliases)
    let sig2 = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string(), "String...".to_string()],
    };
    index.update_file(1, vec![("com.example.User".to_string(), sig2.clone())]);

    let constructors = index.get_constructors("com.example.User").unwrap();
    assert_eq!(constructors.len(), 1);
    assert_eq!(constructors[0], sig2);
}

#[test]
fn test_constructor_cleanup_repeated_updates() {
    let mut index = ConstructorIndex::new();

    // Simulate rapid file changes
    for i in 0..10 {
        let sig = ConstructorSignature {
            class_name: "User".into(),
            parameters: vec![format!("param{}", i)],
        };
        index.update_file(1, vec![("com.example.User".to_string(), sig)]);

        // Should always have exactly 1 constructor
        assert_eq!(index.total_constructors(), 1);
    }
}

#[test]
fn test_constructor_cleanup_multiple_files() {
    let mut index = ConstructorIndex::new();

    // File 1: User
    let user_sig = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string()],
    };
    index.add_constructor(1, "com.example.User".to_string(), user_sig);

    // File 2: Product
    let product_sig = ConstructorSignature {
        class_name: "Product".into(),
        parameters: vec!["String".to_string()],
    };
    index.add_constructor(2, "com.example.Product".to_string(), product_sig);

    assert_eq!(index.total_classes(), 2);

    // Update file 1
    let new_user_sig = ConstructorSignature {
        class_name: "User".into(),
        parameters: vec!["String".to_string(), "int".to_string()],
    };
    index.update_file(1, vec![("com.example.User".to_string(), new_user_sig)]);

    // File 2 should be unaffected
    assert_eq!(index.total_classes(), 2);
    assert!(index.get_constructors("com.example.Product").is_some());
}

#[test]
fn test_constructor_cleanup_exception_types() {
    let mut index = ConstructorIndex::new();

    // Constructor with throws clause (parameters only, not exceptions in signature)
    let sig1 = ConstructorSignature {
        class_name: "FileReader".into(),
        parameters: vec!["String".to_string()],
    };
    index.add_constructor(1, "com.example.FileReader".to_string(), sig1);

    // Update: different exceptions (but same parameters)
    let sig2 = ConstructorSignature {
        class_name: "FileReader".into(),
        parameters: vec!["String".to_string()],
    };
    index.update_file(
        1,
        vec![("com.example.FileReader".to_string(), sig2.clone())],
    );

    let constructors = index.get_constructors("com.example.FileReader").unwrap();
    assert_eq!(constructors.len(), 1);
    assert_eq!(constructors[0], sig2);
}
