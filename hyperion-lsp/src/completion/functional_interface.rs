// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Functional Interface Detection and Lambda Completion
//!
//! This module provides utilities for detecting functional interfaces and
//! generating lambda expression completions in Java code.

use hyperion_workspace::index::WorkspaceIndex;
use hyperion_workspace::FunctionalInterfaceQuery;

/// Information about a functional interface's Single Abstract Method (SAM)
#[derive(Debug, Clone)]
pub struct FunctionalInterfaceInfo {
    /// Fully qualified name of the functional interface
    pub interface_fqn: String,
    /// Parameter types of the SAM
    pub parameter_types: Vec<String>,
    /// Return type of the SAM
    pub return_type: String,
    /// Number of parameters
    pub param_count: usize,
}

/// Common Java functional interfaces from java.util.function
const STANDARD_FUNCTIONAL_INTERFACES: &[(&str, &[&str], &str)] = &[
    // Consumer family - single argument, void return
    ("java.util.function.Consumer", &["T"], "void"),
    ("java.util.function.BiConsumer", &["T", "U"], "void"),
    ("java.util.function.IntConsumer", &["int"], "void"),
    ("java.util.function.LongConsumer", &["long"], "void"),
    ("java.util.function.DoubleConsumer", &["double"], "void"),
    ("java.util.function.ObjIntConsumer", &["T", "int"], "void"),
    ("java.util.function.ObjLongConsumer", &["T", "long"], "void"),
    (
        "java.util.function.ObjDoubleConsumer",
        &["T", "double"],
        "void",
    ),
    // Function family - single argument, returns value
    ("java.util.function.Function", &["T"], "R"),
    ("java.util.function.BiFunction", &["T", "U"], "R"),
    ("java.util.function.IntFunction", &["int"], "R"),
    ("java.util.function.LongFunction", &["long"], "R"),
    ("java.util.function.DoubleFunction", &["double"], "R"),
    ("java.util.function.ToIntFunction", &["T"], "int"),
    ("java.util.function.ToLongFunction", &["T"], "long"),
    ("java.util.function.ToDoubleFunction", &["T"], "double"),
    ("java.util.function.IntToLongFunction", &["int"], "long"),
    ("java.util.function.IntToDoubleFunction", &["int"], "double"),
    ("java.util.function.LongToIntFunction", &["long"], "int"),
    (
        "java.util.function.LongToDoubleFunction",
        &["long"],
        "double",
    ),
    ("java.util.function.DoubleToIntFunction", &["double"], "int"),
    (
        "java.util.function.DoubleToLongFunction",
        &["double"],
        "long",
    ),
    ("java.util.function.ToIntBiFunction", &["T", "U"], "int"),
    ("java.util.function.ToLongBiFunction", &["T", "U"], "long"),
    (
        "java.util.function.ToDoubleBiFunction",
        &["T", "U"],
        "double",
    ),
    // Predicate family - boolean return
    ("java.util.function.Predicate", &["T"], "boolean"),
    ("java.util.function.BiPredicate", &["T", "U"], "boolean"),
    ("java.util.function.IntPredicate", &["int"], "boolean"),
    ("java.util.function.LongPredicate", &["long"], "boolean"),
    ("java.util.function.DoublePredicate", &["double"], "boolean"),
    // Supplier family - no arguments
    ("java.util.function.Supplier", &[], "T"),
    ("java.util.function.BooleanSupplier", &[], "boolean"),
    ("java.util.function.IntSupplier", &[], "int"),
    ("java.util.function.LongSupplier", &[], "long"),
    ("java.util.function.DoubleSupplier", &[], "double"),
    // Operator family - same type in/out
    ("java.util.function.UnaryOperator", &["T"], "T"),
    ("java.util.function.BinaryOperator", &["T", "T"], "T"),
    ("java.util.function.IntUnaryOperator", &["int"], "int"),
    ("java.util.function.LongUnaryOperator", &["long"], "long"),
    (
        "java.util.function.DoubleUnaryOperator",
        &["double"],
        "double",
    ),
    (
        "java.util.function.IntBinaryOperator",
        &["int", "int"],
        "int",
    ),
    (
        "java.util.function.LongBinaryOperator",
        &["long", "long"],
        "long",
    ),
    (
        "java.util.function.DoubleBinaryOperator",
        &["double", "double"],
        "double",
    ),
    // Other common functional interfaces
    ("java.lang.Runnable", &[], "void"),
    ("java.util.concurrent.Callable", &[], "V"),
    ("java.util.Comparator", &["T", "T"], "int"),
];

/// Check if a type is a known functional interface
pub fn is_functional_interface(fqn: &str) -> bool {
    STANDARD_FUNCTIONAL_INTERFACES
        .iter()
        .any(|(interface_fqn, _, _)| *interface_fqn == fqn)
}

/// Get functional interface information for a known standard interface
pub fn get_functional_interface_info(
    fqn: &str,
    type_arguments: &[String],
) -> Option<FunctionalInterfaceInfo> {
    let (_, param_templates, return_template) = STANDARD_FUNCTIONAL_INTERFACES
        .iter()
        .find(|(interface_fqn, _, _)| *interface_fqn == fqn)?;

    // Substitute type parameters if provided
    let parameter_types: Vec<String> = if type_arguments.is_empty() {
        // No type arguments - use generic parameter names
        param_templates.iter().map(|s| (*s).to_string()).collect()
    } else {
        // Substitute T, U, R, V with actual type arguments
        param_templates
            .iter()
            .map(|template| substitute_type_param(template, type_arguments))
            .collect()
    };

    let return_type = substitute_type_param(return_template, type_arguments);
    let param_count = parameter_types.len();

    Some(FunctionalInterfaceInfo {
        interface_fqn: fqn.to_string(),
        parameter_types,
        return_type,
        param_count,
    })
}

/// Substitute generic type parameters with concrete types
fn substitute_type_param(template: &str, type_args: &[String]) -> String {
    match template {
        "T" => type_args
            .first()
            .cloned()
            .unwrap_or_else(|| "T".to_string()),
        "U" => type_args.get(1).cloned().unwrap_or_else(|| "U".to_string()),
        // For interfaces like Function<T, R> / BiFunction<T, U, R>, the return slot
        // is the final type argument.
        "R" => type_args.last().cloned().unwrap_or_else(|| "R".to_string()),
        // Callable<V> also uses the final type slot.
        "V" => type_args.last().cloned().unwrap_or_else(|| "V".to_string()),
        other => other.to_string(), // Primitive types, void, etc.
    }
}

/// Try to detect functional interface from workspace index
///
/// This queries the workspace index to check if a type has exactly one abstract method,
/// which is the definition of a functional interface.
pub fn detect_functional_interface_from_index(
    workspace_index: &WorkspaceIndex,
    fqn: &str,
) -> Option<FunctionalInterfaceInfo> {
    detect_functional_interface_from_index_with_type_arguments(workspace_index, fqn, &[])
}

/// Detect an indexed functional interface and apply type arguments from the
/// receiver when its function method is declared directly on that interface.
///
/// The workspace currently stores raw supertype names without their generic
/// edge arguments. An inherited SAM that mentions its declaring interface's
/// type parameters therefore fails closed: mapping the child's type arguments
/// by position would silently associate unrelated parameters.
pub fn detect_functional_interface_from_index_with_type_arguments(
    workspace_index: &WorkspaceIndex,
    fqn: &str,
    type_arguments: &[String],
) -> Option<FunctionalInterfaceInfo> {
    use tracing::debug;

    debug!("Looking up functional interface: {}", fqn);
    match workspace_index.query_functional_interface(fqn, type_arguments) {
        FunctionalInterfaceQuery::Resolved(signature) => {
            let parameter_types = signature.parameter_types;
            let return_type = signature.return_type;
            let param_count = parameter_types.len();
            debug!(
                "Found indexed functional interface: {} with SAM: {} returns {}",
                fqn, signature.method.simple_name, return_type
            );
            Some(FunctionalInterfaceInfo {
                interface_fqn: fqn.to_string(),
                parameter_types,
                return_type,
                param_count,
            })
        }
        FunctionalInterfaceQuery::Missing => {
            // Startup and small tests may query standard interfaces before JDK
            // symbols load. The table is an absence fallback, never an
            // alternative hierarchy implementation.
            get_functional_interface_info(fqn, type_arguments)
        }
        FunctionalInterfaceQuery::NotFunctional | FunctionalInterfaceQuery::Unavailable => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_functional_interface() {
        assert!(is_functional_interface("java.util.function.Consumer"));
        assert!(is_functional_interface("java.util.function.Function"));
        assert!(is_functional_interface("java.util.function.Predicate"));
        assert!(is_functional_interface("java.lang.Runnable"));
        assert!(!is_functional_interface("java.lang.String"));
        assert!(!is_functional_interface("java.util.List"));
    }

    #[test]
    fn test_get_consumer_info() {
        let info =
            get_functional_interface_info("java.util.function.Consumer", &["String".to_string()])
                .expect("Should get Consumer info");

        assert_eq!(info.interface_fqn, "java.util.function.Consumer");
        assert_eq!(info.parameter_types, vec!["String"]);
        assert_eq!(info.return_type, "void");
        assert_eq!(info.param_count, 1);
    }

    #[test]
    fn test_get_function_info() {
        let info = get_functional_interface_info(
            "java.util.function.Function",
            &["String".to_string(), "Integer".to_string()],
        )
        .expect("Should get Function info");

        assert_eq!(info.parameter_types, vec!["String"]);
        assert_eq!(info.return_type, "Integer");
    }

    #[test]
    fn test_get_predicate_info() {
        let info =
            get_functional_interface_info("java.util.function.Predicate", &["User".to_string()])
                .expect("Should get Predicate info");

        assert_eq!(info.parameter_types, vec!["User"]);
        assert_eq!(info.return_type, "boolean");
    }

    #[test]
    fn test_get_supplier_info() {
        let info =
            get_functional_interface_info("java.util.function.Supplier", &["List".to_string()])
                .expect("Should get Supplier info");

        assert_eq!(info.parameter_types.len(), 0);
        assert_eq!(info.return_type, "List");
    }

    #[test]
    fn test_get_runnable_info() {
        let info = get_functional_interface_info("java.lang.Runnable", &[])
            .expect("Should get Runnable info");

        assert_eq!(info.parameter_types.len(), 0);
        assert_eq!(info.return_type, "void");
    }

    #[test]
    fn test_get_comparator_info() {
        let info = get_functional_interface_info("java.util.Comparator", &["String".to_string()])
            .expect("Should get Comparator info");

        assert_eq!(info.parameter_types, vec!["String", "String"]);
        assert_eq!(info.return_type, "int");
    }

    #[test]
    fn test_get_callable_info() {
        let info = get_functional_interface_info(
            "java.util.concurrent.Callable",
            &["Integer".to_string()],
        )
        .expect("Should get Callable info");

        assert_eq!(info.parameter_types.len(), 0);
        assert_eq!(info.return_type, "Integer");
    }

    #[test]
    fn standard_table_is_only_a_missing_index_fallback() {
        let temp = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        assert!(detect_functional_interface_from_index_with_type_arguments(
            &index,
            "java.util.function.Function",
            &["String".to_string(), "Integer".to_string()]
        )
        .is_some());

        index
            .index_file_with_source(
                &temp.path().join("java/util/function/Function.java"),
                concat!(
                    "package java.util.function; interface Function<T, R> { ",
                    "R apply(T value); R second(T value); }"
                ),
            )
            .unwrap();
        assert!(detect_functional_interface_from_index_with_type_arguments(
            &index,
            "java.util.function.Function",
            &["String".to_string(), "Integer".to_string()]
        )
        .is_none());
    }
}
