// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Java class file parsing using cafebabe
//!
//! This module provides a wrapper around the cafebabe library for parsing
//! Java .class files and extracting basic metadata.

use cafebabe::{attributes::AttributeData, parse_class, ClassFile, ParseError as CafebabeError};
use std::borrow::Cow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Failed to parse class file: {0}")]
    CafebabeError(#[from] CafebabeError),

    #[error("Invalid class name")]
    InvalidClassName,

    #[error("Missing super class for non-Object class")]
    MissingSuperClass,
}

/// Parse a Java class file from bytes
pub fn parse_class_file(bytes: &[u8]) -> Result<ClassFile<'_>, ParseError> {
    parse_class(bytes).map_err(ParseError::from)
}

/// Extract the binary class name from a ClassFile.
///
/// This only converts the JVM package separator (`/`) to `.`, deliberately
/// preserving the binary nested-class separator (`$`). The interpreter and
/// classloading code must retain that binary identity.
pub fn get_class_name(class: &ClassFile) -> Result<String, ParseError> {
    let internal_name = &class.this_class;
    Ok(internal_name_to_binary_name(internal_name))
}

/// Extract the binary super class name from a ClassFile.
///
/// Returns None only for java.lang.Object (which has no superclass)
pub fn get_super_class(class: &ClassFile) -> Option<String> {
    class
        .super_class
        .as_ref()
        .map(|name| internal_name_to_binary_name(name))
}

/// Extract the binary names of the interfaces implemented by this class.
pub fn get_interfaces(class: &ClassFile) -> Vec<String> {
    class
        .interfaces
        .iter()
        .map(|name| internal_name_to_binary_name(name))
        .collect()
}

fn internal_name_to_binary_name(internal_name: &str) -> String {
    internal_name.replace('/', ".")
}

/// Convert an unambiguous binary member-class reference to source form.
///
/// Named member classes use `$` in bytecode and `.` in Java/Kotlin/Scala
/// source (`Map$Entry` → `Map.Entry`). A trailing `$` (notably Scala module
/// classes) and numeric components (anonymous/local classes) are deliberately
/// preserved because a bare binary name does not contain enough information
/// to reinterpret them safely.
pub fn binary_reference_to_source_name(binary_name: &str) -> Cow<'_, str> {
    let Some((prefix, suffix)) = binary_name.split_once('$') else {
        return Cow::Borrowed(binary_name);
    };
    let components: Vec<_> = suffix.split('$').collect();
    if prefix.is_empty()
        || components.iter().any(|component| {
            component.is_empty()
                || component
                    .chars()
                    .next()
                    .is_some_and(|first| first.is_ascii_digit())
        })
    {
        return Cow::Borrowed(binary_name);
    }
    Cow::Owned(binary_name.replace('$', "."))
}

/// Return the source-form name to use when indexing this class.
///
/// The `InnerClasses`/`EnclosingMethod` attributes, rather than `$` alone,
/// distinguish named member classes from anonymous/local classes. Named
/// members are returned in dotted source form. Anonymous and local class
/// definitions return `None` because they cannot be imported or referenced by
/// a workspace-wide source name. Names with `$` that are not classified by
/// those attributes are retained verbatim; this avoids conflating Scala module
/// classes (`Foo$`) and other language-generated artifacts with source types.
pub fn get_source_class_name(class: &ClassFile) -> Option<String> {
    let internal_name: &str = &class.this_class;

    let is_local_or_anonymous = class
        .attributes
        .iter()
        .any(|attribute| matches!(attribute.data, AttributeData::EnclosingMethod { .. }));
    if is_local_or_anonymous {
        return None;
    }

    fn member_source_name(
        class: &ClassFile,
        internal_name: &str,
        nesting_depth: usize,
    ) -> Option<String> {
        // Reject malformed/cyclic InnerClasses chains without risking
        // unbounded recursion while indexing an untrusted JAR.
        if nesting_depth > 64 {
            return None;
        }
        let inner_class = class.attributes.iter().find_map(|attribute| {
            let AttributeData::InnerClasses(entries) = &attribute.data else {
                return None;
            };
            entries
                .iter()
                .find(|entry| entry.inner_class_info.as_ref() == internal_name)
        });

        let Some(entry) = inner_class else {
            let binary_name = internal_name_to_binary_name(internal_name);
            if nesting_depth == 0 || !binary_name.contains('$') {
                return Some(binary_name);
            }
            let source_name = binary_reference_to_source_name(&binary_name);
            return (source_name.as_ref() != binary_name).then(|| source_name.into_owned());
        };
        let (Some(outer), Some(inner)) = (&entry.outer_class_info, &entry.inner_name) else {
            return None;
        };
        let outer = member_source_name(class, outer, nesting_depth + 1)?;
        Some(format!("{outer}.{inner}"))
    }

    member_source_name(class, internal_name, 0)
}

/// Extract the package name from a fully qualified class name
///
/// # Examples
/// ```
/// use hyperion_bytecode::parser::extract_package;
///
/// assert_eq!(extract_package("java.lang.String"), Some("java.lang".to_string()));
/// assert_eq!(extract_package("String"), None);
/// ```
pub fn extract_package(fqn: &str) -> Option<String> {
    fqn.rfind('.').map(|pos| fqn[..pos].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package() {
        assert_eq!(
            extract_package("java.lang.String"),
            Some("java.lang".to_string())
        );
        assert_eq!(
            extract_package("java.util.concurrent.ExecutionException"),
            Some("java.util.concurrent".to_string())
        );
        assert_eq!(extract_package("String"), None);
        assert_eq!(extract_package("MyClass"), None);
    }

    #[test]
    fn binary_reference_normalizes_named_member_classes() {
        assert_eq!(
            binary_reference_to_source_name("java.util.Map$Entry"),
            "java.util.Map.Entry"
        );
        assert_eq!(
            binary_reference_to_source_name("example.Outer$Middle$Inner"),
            "example.Outer.Middle.Inner"
        );
    }

    #[test]
    fn internal_name_conversion_keeps_jvm_nested_separator() {
        assert_eq!(
            internal_name_to_binary_name("java/util/Map$Entry"),
            "java.util.Map$Entry"
        );
    }

    #[test]
    fn binary_reference_preserves_anonymous_local_and_scala_module_names() {
        assert_eq!(
            binary_reference_to_source_name("example.Outer$1"),
            "example.Outer$1"
        );
        assert_eq!(
            binary_reference_to_source_name("example.Outer$Named$1"),
            "example.Outer$Named$1"
        );
        assert_eq!(
            binary_reference_to_source_name("example.ScalaObject$"),
            "example.ScalaObject$"
        );
    }
}
