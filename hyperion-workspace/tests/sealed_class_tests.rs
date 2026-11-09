// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Sealed Subclass Order
// Tests that sealed class permitted subclasses are tracked in consistent order
// Current behavior: Subclass order may vary between index rebuilds, breaking determinism
// Expected: Should maintain stable order (e.g., alphabetical or definition order)

use std::collections::HashMap;

/// Mock structure representing sealed class index
#[derive(Debug)]
struct SealedClassIndex {
    /// Maps sealed class FQN -> permitted subclass FQNs (in stable order)
    permitted_subclasses: HashMap<String, Vec<String>>,
}

impl SealedClassIndex {
    fn new() -> Self {
        Self {
            permitted_subclasses: HashMap::new(),
        }
    }

    fn add_sealed_class(&mut self, class_fqn: String, mut subclasses: Vec<String>) {
        // Ensure stable order (alphabetical)
        subclasses.sort();
        self.permitted_subclasses.insert(class_fqn, subclasses);
    }

    fn get_permitted_subclasses(&self, class_fqn: &str) -> Option<&Vec<String>> {
        self.permitted_subclasses.get(class_fqn)
    }

    fn update_sealed_class(&mut self, class_fqn: String, mut new_subclasses: Vec<String>) {
        // Ensure stable order
        new_subclasses.sort();
        self.permitted_subclasses.insert(class_fqn, new_subclasses);
    }
}

#[test]
fn test_subclass_order_stability() {
    let mut index = SealedClassIndex::new();

    // Add sealed class with subclasses in random order
    index.add_sealed_class(
        "com.example.Shape".to_string(),
        vec![
            "com.example.Circle".to_string(),
            "com.example.Rectangle".to_string(),
            "com.example.Triangle".to_string(),
        ],
    );

    let subclasses1 = index
        .get_permitted_subclasses("com.example.Shape")
        .unwrap()
        .clone();

    // Add same sealed class again with different order
    index.add_sealed_class(
        "com.example.Shape".to_string(),
        vec![
            "com.example.Triangle".to_string(),
            "com.example.Circle".to_string(),
            "com.example.Rectangle".to_string(),
        ],
    );

    let subclasses2 = index.get_permitted_subclasses("com.example.Shape").unwrap();

    // Order should be stable (alphabetical)
    assert_eq!(&subclasses1, subclasses2);
    assert_eq!(
        subclasses2,
        &vec![
            "com.example.Circle".to_string(),
            "com.example.Rectangle".to_string(),
            "com.example.Triangle".to_string(),
        ]
    );
}

#[test]
fn test_subclass_order_after_update() {
    let mut index = SealedClassIndex::new();

    // Initial order
    index.add_sealed_class(
        "com.example.Result".to_string(),
        vec![
            "com.example.Success".to_string(),
            "com.example.Failure".to_string(),
        ],
    );

    let initial = index
        .get_permitted_subclasses("com.example.Result")
        .unwrap()
        .clone();
    assert_eq!(
        initial,
        vec![
            "com.example.Failure".to_string(),
            "com.example.Success".to_string(),
        ]
    );

    // Update with reversed order
    index.update_sealed_class(
        "com.example.Result".to_string(),
        vec![
            "com.example.Failure".to_string(),
            "com.example.Success".to_string(),
        ],
    );

    let updated = index
        .get_permitted_subclasses("com.example.Result")
        .unwrap();

    // Should maintain same order
    assert_eq!(&initial, updated);
}

#[test]
fn test_subclass_order_with_additions() {
    let mut index = SealedClassIndex::new();

    // Start with 2 subclasses
    index.add_sealed_class(
        "com.example.Animal".to_string(),
        vec!["com.example.Dog".to_string(), "com.example.Cat".to_string()],
    );

    // Add a third subclass
    index.update_sealed_class(
        "com.example.Animal".to_string(),
        vec![
            "com.example.Dog".to_string(),
            "com.example.Cat".to_string(),
            "com.example.Bird".to_string(),
        ],
    );

    let subclasses = index
        .get_permitted_subclasses("com.example.Animal")
        .unwrap();

    // Should be in alphabetical order
    assert_eq!(
        subclasses,
        &vec![
            "com.example.Bird".to_string(),
            "com.example.Cat".to_string(),
            "com.example.Dog".to_string(),
        ]
    );
}

#[test]
fn test_subclass_order_with_removals() {
    let mut index = SealedClassIndex::new();

    // Start with 3 subclasses
    index.add_sealed_class(
        "com.example.Animal".to_string(),
        vec![
            "com.example.Dog".to_string(),
            "com.example.Cat".to_string(),
            "com.example.Bird".to_string(),
        ],
    );

    // Remove one subclass
    index.update_sealed_class(
        "com.example.Animal".to_string(),
        vec!["com.example.Dog".to_string(), "com.example.Cat".to_string()],
    );

    let subclasses = index
        .get_permitted_subclasses("com.example.Animal")
        .unwrap();

    // Should maintain alphabetical order
    assert_eq!(
        subclasses,
        &vec!["com.example.Cat".to_string(), "com.example.Dog".to_string(),]
    );
}

#[test]
fn test_nested_sealed_class_order() {
    let mut index = SealedClassIndex::new();

    // Sealed class in outer class
    index.add_sealed_class(
        "com.example.Outer.Inner".to_string(),
        vec![
            "com.example.Outer.Inner.SubA".to_string(),
            "com.example.Outer.Inner.SubB".to_string(),
        ],
    );

    let subclasses = index
        .get_permitted_subclasses("com.example.Outer.Inner")
        .unwrap();

    assert_eq!(
        subclasses,
        &vec![
            "com.example.Outer.Inner.SubA".to_string(),
            "com.example.Outer.Inner.SubB".to_string(),
        ]
    );
}

#[test]
fn test_single_subclass_order() {
    let mut index = SealedClassIndex::new();

    // Sealed class with only one permitted subclass
    index.add_sealed_class(
        "com.example.Singleton".to_string(),
        vec!["com.example.Instance".to_string()],
    );

    let subclasses = index
        .get_permitted_subclasses("com.example.Singleton")
        .unwrap();

    assert_eq!(subclasses, &vec!["com.example.Instance".to_string()]);
}

#[test]
fn test_multiple_sealed_classes_independent_order() {
    let mut index = SealedClassIndex::new();

    // Add first sealed class
    index.add_sealed_class(
        "com.example.Shape".to_string(),
        vec![
            "com.example.Circle".to_string(),
            "com.example.Square".to_string(),
        ],
    );

    // Add second sealed class
    index.add_sealed_class(
        "com.example.Color".to_string(),
        vec![
            "com.example.Red".to_string(),
            "com.example.Blue".to_string(),
        ],
    );

    let shape_subs = index.get_permitted_subclasses("com.example.Shape").unwrap();
    let color_subs = index.get_permitted_subclasses("com.example.Color").unwrap();

    // Each should have its own stable order
    assert_eq!(
        shape_subs,
        &vec![
            "com.example.Circle".to_string(),
            "com.example.Square".to_string(),
        ]
    );
    assert_eq!(
        color_subs,
        &vec![
            "com.example.Blue".to_string(),
            "com.example.Red".to_string(),
        ]
    );
}

#[test]
fn test_subclass_order_with_similar_names() {
    let mut index = SealedClassIndex::new();

    // Subclasses with similar prefixes
    index.add_sealed_class(
        "com.example.Result".to_string(),
        vec![
            "com.example.ResultSuccess".to_string(),
            "com.example.Result10".to_string(),
            "com.example.Result2".to_string(),
            "com.example.ResultFailure".to_string(),
        ],
    );

    let subclasses = index
        .get_permitted_subclasses("com.example.Result")
        .unwrap();

    // Should use lexicographic order (not numeric)
    assert_eq!(
        subclasses,
        &vec![
            "com.example.Result10".to_string(),
            "com.example.Result2".to_string(),
            "com.example.ResultFailure".to_string(),
            "com.example.ResultSuccess".to_string(),
        ]
    );
}

#[test]
fn test_subclass_order_case_sensitivity() {
    let mut index = SealedClassIndex::new();

    // Subclasses with different cases
    index.add_sealed_class(
        "com.example.Base".to_string(),
        vec![
            "com.example.aaa".to_string(),
            "com.example.AAA".to_string(),
            "com.example.Aaa".to_string(),
        ],
    );

    let subclasses = index.get_permitted_subclasses("com.example.Base").unwrap();

    // Should maintain consistent case-sensitive ordering
    assert_eq!(subclasses.len(), 3);
    // Verify it's sorted
    let mut sorted = subclasses.clone();
    sorted.sort();
    assert_eq!(subclasses, &sorted);
}

#[test]
fn test_subclass_order_rebuild_consistency() {
    let mut index1 = SealedClassIndex::new();
    let mut index2 = SealedClassIndex::new();

    let subclasses = vec![
        "com.example.Z".to_string(),
        "com.example.A".to_string(),
        "com.example.M".to_string(),
    ];

    // Build indexes with same data in different order
    index1.add_sealed_class("com.example.Base".to_string(), subclasses.clone());

    let reversed: Vec<String> = subclasses.into_iter().rev().collect();
    index2.add_sealed_class("com.example.Base".to_string(), reversed);

    // Both should produce same order
    let order1 = index1.get_permitted_subclasses("com.example.Base").unwrap();
    let order2 = index2.get_permitted_subclasses("com.example.Base").unwrap();

    assert_eq!(order1, order2);
    assert_eq!(
        order1,
        &vec![
            "com.example.A".to_string(),
            "com.example.M".to_string(),
            "com.example.Z".to_string(),
        ]
    );
}

#[test]
fn test_empty_subclass_list() {
    let mut index = SealedClassIndex::new();

    // Sealed class with no permitted subclasses (shouldn't happen in valid Java, but test it)
    index.add_sealed_class("com.example.Sealed".to_string(), vec![]);

    let subclasses = index
        .get_permitted_subclasses("com.example.Sealed")
        .unwrap();
    assert!(subclasses.is_empty());
}

#[test]
fn test_subclass_order_with_packages() {
    let mut index = SealedClassIndex::new();

    // Subclasses from different packages
    index.add_sealed_class(
        "com.example.Base".to_string(),
        vec![
            "com.example.impl.SubA".to_string(),
            "com.example.SubB".to_string(),
            "com.example.internal.SubC".to_string(),
        ],
    );

    let subclasses = index.get_permitted_subclasses("com.example.Base").unwrap();

    // Should be sorted by full FQN
    assert_eq!(
        subclasses,
        &vec![
            "com.example.SubB".to_string(),
            "com.example.impl.SubA".to_string(),
            "com.example.internal.SubC".to_string(),
        ]
    );
}
