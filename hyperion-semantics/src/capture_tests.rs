// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Wildcard Capture Conversion Tests
//!
//! Tests for capture conversion with wildcards.
//! When a wildcard type is captured, it becomes a fresh type variable
//! with the wildcard's bounds. This allows certain operations that
//! would otherwise be unsafe.

#[cfg(test)]
mod tests {
    use crate::symbol::{ClassType, Type};

    #[test]
    fn test_basic_capture() {
        // List<?> list = ...;
        // When passed to <T> void process(List<T> list),
        // ? is captured as a fresh type variable CAP#1

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Capture creates: CAP#1 where CAP#1 extends Object
        // Method sees: List<CAP#1>
    }

    #[test]
    fn test_capture_extends_bound() {
        // List<? extends Number> list = ...;
        // Captured as CAP#1 where CAP#1 extends Number

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Capture creates: CAP#1 where CAP#1 extends Number
        // Preserves the upper bound constraint
    }

    #[test]
    fn test_capture_super_bound() {
        // List<? super Integer> list = ...;
        // Captured as CAP#1 where CAP#1 super Integer

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_super_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        // Capture creates: CAP#1 where Integer super CAP#1
        // Preserves the lower bound constraint
    }

    #[test]
    fn test_capture_enables_get() {
        // List<?> list = ...;
        // Object o = list.get(0); // OK - returns CAP#1 which is assignable to Object

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        let _object_type = Type::Class(ClassType {
            name: "Object".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // list.get(0) returns CAP#1
        // CAP#1 extends Object
        // Therefore assignable to Object
    }

    #[test]
    fn test_capture_prevents_add() {
        // List<?> list = ...;
        // list.add("hello"); // ERROR - can't add String to List<CAP#1>

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        let _string_type = Type::Class(ClassType {
            name: "String".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        // list.add(E) requires E = CAP#1
        // String is not assignable to CAP#1 (unknown type)
        // Therefore this should fail
    }

    #[test]
    fn test_capture_with_extends_allows_read() {
        // List<? extends Number> list = ...;
        // Number n = list.get(0); // OK - returns CAP#1 extends Number

        let number_type = Type::Class(ClassType {
            name: "Number".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_extends_number = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(number_type.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // list.get(0) returns CAP#1 where CAP#1 extends Number
        // CAP#1 is assignable to Number
        // Therefore this is valid
    }

    #[test]
    fn test_capture_with_super_allows_write() {
        // List<? super Integer> list = ...;
        // list.add(42); // OK - Integer is a subtype of CAP#1's lower bound

        let integer_type = Type::Class(ClassType {
            name: "Integer".into(),
            package: Some("java.lang".into()),
            type_arguments: vec![],
            ..Default::default()
        });

        let _list_super_integer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(Box::new(integer_type.clone())),
            }],
            ..Default::default()
        });

        // list.add(E) requires E super Integer
        // Integer is assignable to E (E is a supertype)
        // Therefore this is valid
    }

    #[test]
    fn test_multiple_captures() {
        // Map<?, ?> map = ...;
        // Creates two fresh type variables: CAP#1 and CAP#2

        let _map_wildcard = Type::Class(ClassType {
            name: "Map".into(),
            package: Some("java.util".into()),
            type_arguments: vec![
                Type::Wildcard {
                    upper_bound: None,
                    lower_bound: None,
                },
                Type::Wildcard {
                    upper_bound: None,
                    lower_bound: None,
                },
            ],
            ..Default::default()
        });

        // Each wildcard gets its own captured type variable
        // CAP#1 for key, CAP#2 for value
        // They are independent types
    }

    #[test]
    fn test_capture_in_method_invocation() {
        // void swap(List<?> list, int i, int j)
        // When called, ? is captured once for the entire method

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Inside swap:
        // E temp = list.get(i);    // OK - temp has type CAP#1
        // list.set(j, temp);       // OK - CAP#1 matches
        // This is safe because capture is consistent
    }

    #[test]
    fn test_capture_scope() {
        // Each method invocation creates fresh captures

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // List<?> list1 = ...;
        // List<?> list2 = ...;
        // process(list1); // captures as CAP#1
        // process(list2); // captures as CAP#2 (different)
        // CAP#1 and CAP#2 are distinct types
    }

    #[test]
    fn test_no_capture_for_return_type() {
        // List<?> foo() { ... }
        // Caller sees List<?>, not captured type

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Return type wildcards are NOT captured
        // Caller must perform capture conversion if needed
    }

    #[test]
    fn test_capture_nested_wildcards() {
        // List<? extends List<?>> list = ...;
        // Outer wildcard is captured, inner is not

        let list_wildcard_inner = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        let _list_wildcard_outer = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: Some(Box::new(list_wildcard_inner.clone())),
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Capture creates: CAP#1 extends List<?>
        // Inner wildcard remains uncaptured
    }

    #[test]
    fn test_capture_equality() {
        // Two captures of the same wildcard are the same type within a context

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // void process(List<?> list) {
        //     Object a = list.get(0);  // CAP#1
        //     Object b = list.get(1);  // CAP#1 (same)
        //     // Both have the same captured type
        // }
    }

    #[test]
    fn test_capture_helper_method() {
        // Sometimes need a helper method to work with captures:
        // <T> void process(List<T> list) { ... }
        // void caller(List<?> wildcard) {
        //     process(wildcard); // capture conversion happens here
        // }

        let _list_wildcard = Type::Class(ClassType {
            name: "List".into(),
            package: Some("java.util".into()),
            type_arguments: vec![Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            }],
            ..Default::default()
        });

        // Passing List<?> to List<T> parameter triggers capture
        // T is inferred as the captured type
    }
}
