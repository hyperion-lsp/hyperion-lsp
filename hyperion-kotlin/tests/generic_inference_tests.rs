// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for Kotlin generic type inference
//!
//! These tests verify that the type inference engine correctly resolves
//! generic type parameters for stdlib functions and collection methods.

use hyperion_kotlin::inference::{infer_generic_call, infer_method_return_type};
use hyperion_kotlin::types::KotlinType;

mod collection_constructor_tests {
    use super::*;

    #[test]
    fn test_listof_string() {
        let result = infer_generic_call("listOf", &[KotlinType::string()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<String>");
    }

    #[test]
    fn test_listof_int() {
        let result = infer_generic_call("listOf", &[KotlinType::int()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<Int>");
    }

    #[test]
    fn test_listof_multiple_same_type() {
        let result = infer_generic_call(
            "listOf",
            &[
                KotlinType::string(),
                KotlinType::string(),
                KotlinType::string(),
            ],
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<String>");
    }

    #[test]
    fn test_listof_empty() {
        let result = infer_generic_call("listOf", &[]);
        assert!(result.is_some());
        // With no arguments, T cannot be inferred, returns List<?>
        assert_eq!(result.unwrap().to_string(), "List<?>");
    }

    #[test]
    fn test_mutablelistof_string() {
        let result = infer_generic_call("mutableListOf", &[KotlinType::string()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "MutableList<String>");
    }

    #[test]
    fn test_arraylistof_int() {
        let result = infer_generic_call("arrayListOf", &[KotlinType::int()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "ArrayList<Int>");
    }

    #[test]
    fn test_setof_boolean() {
        let result = infer_generic_call("setOf", &[KotlinType::boolean()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Set<Boolean>");
    }

    #[test]
    fn test_mutablesetof_string() {
        let result = infer_generic_call("mutableSetOf", &[KotlinType::string()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "MutableSet<String>");
    }

    #[test]
    fn test_hashsetof_int() {
        let result = infer_generic_call("hashSetOf", &[KotlinType::int()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "HashSet<Int>");
    }

    #[test]
    fn test_arrayof_double() {
        let result = infer_generic_call("arrayOf", &[KotlinType::double()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Array<Double>");
    }

    #[test]
    fn test_sequenceof_string() {
        let result = infer_generic_call("sequenceOf", &[KotlinType::string()]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Sequence<String>");
    }
}

mod map_constructor_tests {
    use super::*;

    #[test]
    fn test_mapof_with_pair() {
        // mapOf takes Pair<K, V> arguments
        let pair_type =
            KotlinType::generic("kotlin.Pair", vec![KotlinType::string(), KotlinType::int()]);
        let result = infer_generic_call("mapOf", &[pair_type]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Map<String, Int>");
    }

    #[test]
    fn test_mutablemapof_with_pair() {
        let pair_type =
            KotlinType::generic("kotlin.Pair", vec![KotlinType::int(), KotlinType::string()]);
        let result = infer_generic_call("mutableMapOf", &[pair_type]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "MutableMap<Int, String>");
    }

    #[test]
    fn test_hashmapof_with_pair() {
        let pair_type = KotlinType::generic(
            "kotlin.Pair",
            vec![KotlinType::string(), KotlinType::boolean()],
        );
        let result = infer_generic_call("hashMapOf", &[pair_type]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "HashMap<String, Boolean>");
    }

    #[test]
    fn test_emptymap() {
        let result = infer_generic_call("emptyMap", &[]);
        assert!(result.is_some());
        // Empty map has unknown type params
        assert_eq!(result.unwrap().to_string(), "Map<?, ?>");
    }
}

mod method_inference_tests {
    use super::*;

    #[test]
    fn test_list_first() {
        let list_user = KotlinType::list(KotlinType::simple("User"));
        let result = infer_method_return_type(&list_user, "first", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "User");
    }

    #[test]
    fn test_list_firstornull() {
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_method_return_type(&list_string, "firstOrNull", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "String?");
    }

    #[test]
    fn test_list_last() {
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_method_return_type(&list_int, "last", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Int");
    }

    #[test]
    fn test_list_filter() {
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_method_return_type(&list_string, "filter", &[]);
        assert!(result.is_some());
        // filter preserves the element type
        assert_eq!(result.unwrap().to_string(), "List<String>");
    }

    #[test]
    fn test_list_sorted() {
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_method_return_type(&list_int, "sorted", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<Int>");
    }

    #[test]
    fn test_list_reversed() {
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_method_return_type(&list_string, "reversed", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<String>");
    }

    #[test]
    fn test_list_distinct() {
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_method_return_type(&list_int, "distinct", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<Int>");
    }

    #[test]
    fn test_list_take() {
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_method_return_type(&list_string, "take", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<String>");
    }

    #[test]
    fn test_list_find() {
        let list_user = KotlinType::list(KotlinType::simple("User"));
        let result = infer_method_return_type(&list_user, "find", &[]);
        assert!(result.is_some());
        // find returns nullable
        assert_eq!(result.unwrap().to_string(), "User?");
    }

    #[test]
    fn test_list_foreach() {
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_method_return_type(&list_int, "forEach", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Unit");
    }

    #[test]
    fn test_list_any() {
        let list_string = KotlinType::list(KotlinType::string());
        let result = infer_method_return_type(&list_string, "any", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Boolean");
    }

    #[test]
    fn test_list_all() {
        let list_int = KotlinType::list(KotlinType::int());
        let result = infer_method_return_type(&list_int, "all", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Boolean");
    }
}

mod nested_generic_tests {
    use super::*;

    #[test]
    fn test_list_of_lists() {
        // listOf(listOf("a"))
        let inner_list = KotlinType::list(KotlinType::string());
        let result = infer_generic_call("listOf", &[inner_list]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "List<List<String>>");
    }

    #[test]
    fn test_map_with_list_value() {
        // mapOf("key" to listOf(1, 2, 3))
        let list_int = KotlinType::list(KotlinType::int());
        let pair_type = KotlinType::generic("kotlin.Pair", vec![KotlinType::string(), list_int]);
        let result = infer_generic_call("mapOf", &[pair_type]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().to_string(), "Map<String, List<Int>>");
    }
}

mod kotlin_type_construction_tests {
    use super::*;

    #[test]
    fn test_simple_type() {
        let ty = KotlinType::simple("User");
        assert_eq!(ty.to_string(), "User");
        assert!(!ty.is_nullable());
    }

    #[test]
    fn test_nullable_type() {
        let ty = KotlinType::string().nullable();
        assert_eq!(ty.to_string(), "String?");
        assert!(ty.is_nullable());
    }

    #[test]
    fn test_generic_type() {
        let ty = KotlinType::list(KotlinType::int());
        assert_eq!(ty.to_string(), "List<Int>");
    }

    #[test]
    fn test_map_type() {
        let ty = KotlinType::map(KotlinType::string(), KotlinType::int());
        assert_eq!(ty.to_string(), "Map<String, Int>");
    }

    #[test]
    fn test_function_type() {
        let ty = KotlinType::function(
            vec![KotlinType::int(), KotlinType::string()],
            KotlinType::boolean(),
        );
        assert_eq!(ty.to_string(), "(Int, String) -> Boolean");
    }
}

mod type_substitution_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_substitute_single_var() {
        let list_t = KotlinType::generic("List", vec![KotlinType::type_var("T")]);

        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::string());

        let result = list_t.substitute(&subs);
        assert_eq!(result.to_string(), "List<String>");
    }

    #[test]
    fn test_substitute_multiple_vars() {
        let map_kv = KotlinType::generic(
            "Map",
            vec![KotlinType::type_var("K"), KotlinType::type_var("V")],
        );

        let mut subs = HashMap::new();
        subs.insert("K".to_string(), KotlinType::string());
        subs.insert("V".to_string(), KotlinType::int());

        let result = map_kv.substitute(&subs);
        assert_eq!(result.to_string(), "Map<String, Int>");
    }

    #[test]
    fn test_substitute_nested() {
        // List<Pair<K, V>>
        let pair_kv = KotlinType::generic(
            "Pair",
            vec![KotlinType::type_var("K"), KotlinType::type_var("V")],
        );
        let list_pair = KotlinType::generic("List", vec![pair_kv]);

        let mut subs = HashMap::new();
        subs.insert("K".to_string(), KotlinType::string());
        subs.insert("V".to_string(), KotlinType::int());

        let result = list_pair.substitute(&subs);
        assert_eq!(result.to_string(), "List<Pair<String, Int>>");
    }

    #[test]
    fn test_substitute_unbound_var() {
        let list_t = KotlinType::generic("List", vec![KotlinType::type_var("T")]);

        // Empty substitutions - T stays as a type variable
        let subs = HashMap::new();
        let result = list_t.substitute(&subs);
        assert_eq!(result.to_string(), "List<T>");
    }
}
