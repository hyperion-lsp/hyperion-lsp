// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The one table of `java.lang` types whose binding holds without an index.
//!
//! `java.lang.*` is imported on demand into every compilation unit (JLS
//! §7.5.5), so these names resolve even before the JDK has been indexed.
//! Two independently maintained tables previously encoded that fact — one in
//! this crate's resolution context and one in the workspace import resolver —
//! and they drifted: `Comparable`, `Iterable` and `CharSequence` were missing
//! from this crate's copy, which is what made `Comparable value` resolve to a
//! fabricated same-package type.
//!
//! This list is deliberately not exhaustive: it is the set that must resolve
//! with no JDK on disk. An indexed JDK supplies the rest through ordinary
//! lookup, so adding a rarely written name here buys nothing and risks
//! shadowing a workspace type of the same name in the degraded, lookup-less
//! resolution mode.

/// `java.lang` simple names that bind without workspace evidence.
pub const CORE_JAVA_LANG_TYPES: &[&str] = &[
    // Core classes
    "Object",
    "String",
    "StringBuilder",
    "StringBuffer",
    "CharSequence",
    "Class",
    "ClassLoader",
    "System",
    "Math",
    "Thread",
    "ThreadLocal",
    "Package",
    "Process",
    "ProcessBuilder",
    "Record",
    "Enum",
    "Void",
    // Core interfaces. `Runnable`, `Comparable` and `Iterable` in particular
    // appear in ordinary code (`Runnable r = () -> {}`) and were reported as
    // unresolved whenever the JDK was not indexed.
    "Runnable",
    "Comparable",
    "Iterable",
    "Cloneable",
    "AutoCloseable",
    "Appendable",
    "Readable",
    // Wrapper types
    "Boolean",
    "Byte",
    "Character",
    "Short",
    "Integer",
    "Long",
    "Float",
    "Double",
    "Number",
    // Annotations
    "Override",
    "Deprecated",
    "SuppressWarnings",
    "FunctionalInterface",
    "SafeVarargs",
    // Throwables
    "Exception",
    "RuntimeException",
    "Error",
    "Throwable",
    "IllegalArgumentException",
    "IllegalStateException",
    "NullPointerException",
    "IndexOutOfBoundsException",
    "ArrayIndexOutOfBoundsException",
    "StringIndexOutOfBoundsException",
    "UnsupportedOperationException",
    "ClassCastException",
    "NumberFormatException",
    "ArithmeticException",
    "InterruptedException",
    "CloneNotSupportedException",
    "SecurityException",
    "ClassNotFoundException",
    "NoSuchFieldException",
    "NoSuchMethodException",
    "AssertionError",
    "StackOverflowError",
    "OutOfMemoryError",
];

/// Whether `name` is a `java.lang` type that binds without an indexed JDK.
pub fn is_core_java_lang_type(name: &str) -> bool {
    CORE_JAVA_LANG_TYPES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_core_java_lang_interfaces_without_a_jdk() {
        for name in ["Comparable", "Iterable", "CharSequence", "Runnable"] {
            assert!(
                is_core_java_lang_type(name),
                "java.lang.{name} must bind without an indexed JDK"
            );
        }
    }

    #[test]
    fn does_not_claim_arbitrary_names() {
        for name in ["Widget", "List", "Optional", "Stream"] {
            assert!(!is_core_java_lang_type(name));
        }
    }
}
