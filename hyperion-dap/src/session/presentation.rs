// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Conversions between source paths, JNI/JDWP identities, and DAP-facing display text.

use hyperion_jdwp::replies::{self, JdwpValue};

/// Map a JNI signature to the JDWP tag required when fetching a value.
pub(super) const fn sig_to_tag(signature: &str) -> u8 {
    match signature.as_bytes().first() {
        Some(b'Z') => b'Z',
        Some(b'B') => b'B',
        Some(b'C') => b'C',
        Some(b'S') => b'S',
        Some(b'I') => b'I',
        Some(b'J') => b'J',
        Some(b'F') => b'F',
        Some(b'D') => b'D',
        Some(b'L' | b'[') => b'L',
        _ => b'L',
    }
}

/// Format a value when no richer runtime class/string information is available.
pub(super) fn format_jdwp_value(value: &JdwpValue) -> String {
    match value {
        JdwpValue::Void => "void".to_owned(),
        JdwpValue::Byte(value) => value.to_string(),
        JdwpValue::Boolean(value) => value.to_string(),
        JdwpValue::Char(value) => char::from_u32(u32::from(*value)).map_or_else(
            || format!("'\\u{value:04x}'"),
            |character| format!("'{character}'"),
        ),
        JdwpValue::Short(value) => value.to_string(),
        JdwpValue::Int(value) => value.to_string(),
        JdwpValue::Long(value) => format!("{value}L"),
        JdwpValue::Float(value) => format!("{value}f"),
        JdwpValue::Double(value) => value.to_string(),
        JdwpValue::Object { tag, object_id } if *tag == replies::tag::STRING => {
            format!("String@{object_id}")
        }
        JdwpValue::Object { object_id, .. } => format!("Object@{object_id}"),
        JdwpValue::Null => "null".to_owned(),
    }
}

/// Convert a JNI signature to a Java-style source name.
pub(super) fn jni_sig_to_display(signature: &str) -> String {
    match signature {
        "Z" => "boolean".to_owned(),
        "B" => "byte".to_owned(),
        "C" => "char".to_owned(),
        "S" => "short".to_owned(),
        "I" => "int".to_owned(),
        "J" => "long".to_owned(),
        "F" => "float".to_owned(),
        "D" => "double".to_owned(),
        "V" => "void".to_owned(),
        signature if signature.starts_with('L') && signature.ends_with(';') => {
            signature[1..signature.len() - 1].replace('/', ".")
        }
        signature if signature.starts_with('[') => {
            format!("{}[]", jni_sig_to_display(&signature[1..]))
        }
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_map_to_jdwp_tags_and_java_display_names() {
        assert_eq!(sig_to_tag("I"), b'I');
        assert_eq!(sig_to_tag("Ljava/lang/String;"), b'L');
        assert_eq!(sig_to_tag("[I"), b'L');
        assert_eq!(jni_sig_to_display("Ljava/lang/String;"), "java.lang.String");
        assert_eq!(jni_sig_to_display("[[I"), "int[][]");
    }

    #[test]
    fn fallback_value_format_is_stable_and_source_like() {
        assert_eq!(format_jdwp_value(&JdwpValue::Long(7)), "7L");
        assert_eq!(format_jdwp_value(&JdwpValue::Char('€' as u16)), "'€'");
        assert_eq!(format_jdwp_value(&JdwpValue::Null), "null");
        assert_eq!(
            format_jdwp_value(&JdwpValue::Object {
                tag: replies::tag::STRING,
                object_id: 9
            }),
            "String@9"
        );
    }
}
