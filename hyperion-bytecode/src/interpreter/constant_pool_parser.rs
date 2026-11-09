// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constant Pool Parser
//!
//! Parses the raw constant pool from JVM class file bytecode into
//! `ConstantPoolItem` format for use with the Executor.
//!
//! JVM Class File Constant Pool Format (JVMS 4.4):
//! - Bytes 0-3: Magic number (0xCAFEBABE)
//! - Bytes 4-5: Minor version
//! - Bytes 6-7: Major version
//! - Bytes 8-9: Constant pool count (number of entries + 1)
//! - Bytes 10+: Constant pool entries

use super::errors::{VMError, VMResult};
use crate::producer::ConstantPoolItem;

fn decode_modified_utf8(bytes: &[u8]) -> VMResult<String> {
    crate::modified_utf8::decode(bytes)
        .map_err(|error| VMError::ClassFormatError(format!("Invalid Modified UTF-8: {error}")))
}

/// Constant pool entry tags per JVMS 4.4
mod tags {
    pub const UTF8: u8 = 1;
    pub const INTEGER: u8 = 3;
    pub const FLOAT: u8 = 4;
    pub const LONG: u8 = 5;
    pub const DOUBLE: u8 = 6;
    pub const CLASS: u8 = 7;
    pub const STRING: u8 = 8;
    pub const FIELDREF: u8 = 9;
    pub const METHODREF: u8 = 10;
    pub const INTERFACE_METHODREF: u8 = 11;
    pub const NAME_AND_TYPE: u8 = 12;
    pub const METHOD_HANDLE: u8 = 15;
    pub const METHOD_TYPE: u8 = 16;
    pub const DYNAMIC: u8 = 17;
    pub const INVOKE_DYNAMIC: u8 = 18;
    pub const MODULE: u8 = 19;
    pub const PACKAGE: u8 = 20;
}

/// Parse the constant pool from raw class file bytecode.
///
/// Returns a Vec<ConstantPoolItem> where index 0 contains a placeholder
/// (since JVM constant pool indices are 1-based).
///
/// Long and Double entries occupy two slots in the constant pool,
/// so the second slot contains a placeholder Utf8("").
pub fn parse_constant_pool(bytecode: &[u8]) -> VMResult<Vec<ConstantPoolItem>> {
    if bytecode.len() < 10 {
        return Err(VMError::ClassFormatError(
            "Class file too short for constant pool".to_string(),
        ));
    }

    // Verify magic number
    if bytecode[0..4] != [0xCA, 0xFE, 0xBA, 0xBE] {
        return Err(VMError::ClassFormatError(
            "Invalid class file magic number".to_string(),
        ));
    }

    // Read constant pool count (1-indexed, so actual count is cp_count - 1)
    let cp_count = u16::from_be_bytes([bytecode[8], bytecode[9]]) as usize;

    // Start parsing at byte 10
    let mut pos = 10;
    let mut entries = Vec::with_capacity(cp_count);

    // Index 0 is unused (placeholder)
    entries.push(ConstantPoolItem::Utf8(String::new()));

    let mut i = 1;
    while i < cp_count {
        if pos >= bytecode.len() {
            return Err(VMError::ClassFormatError(format!(
                "Unexpected end of class file at constant pool entry {}",
                i
            )));
        }

        let tag = bytecode[pos];
        pos += 1;

        let entry = match tag {
            tags::UTF8 => {
                if pos + 2 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "UTF8 entry truncated".to_string(),
                    ));
                }
                let len = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]) as usize;
                pos += 2;
                if pos + len > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "UTF8 string data truncated".to_string(),
                    ));
                }
                let bytes = &bytecode[pos..pos + len];
                pos += len;
                let s = decode_modified_utf8(bytes)?;
                ConstantPoolItem::Utf8(s)
            }
            tags::INTEGER => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Integer entry truncated".to_string(),
                    ));
                }
                let value = i32::from_be_bytes([
                    bytecode[pos],
                    bytecode[pos + 1],
                    bytecode[pos + 2],
                    bytecode[pos + 3],
                ]);
                pos += 4;
                ConstantPoolItem::Integer(value)
            }
            tags::FLOAT => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Float entry truncated".to_string(),
                    ));
                }
                let bits = u32::from_be_bytes([
                    bytecode[pos],
                    bytecode[pos + 1],
                    bytecode[pos + 2],
                    bytecode[pos + 3],
                ]);
                pos += 4;
                ConstantPoolItem::Float(f32::from_bits(bits))
            }
            tags::LONG => {
                if pos + 8 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Long entry truncated".to_string(),
                    ));
                }
                let value = i64::from_be_bytes([
                    bytecode[pos],
                    bytecode[pos + 1],
                    bytecode[pos + 2],
                    bytecode[pos + 3],
                    bytecode[pos + 4],
                    bytecode[pos + 5],
                    bytecode[pos + 6],
                    bytecode[pos + 7],
                ]);
                pos += 8;
                // Long takes 2 slots - add placeholder for next slot
                entries.push(ConstantPoolItem::Long(value));
                entries.push(ConstantPoolItem::Utf8(String::new())); // Placeholder
                i += 2;
                continue;
            }
            tags::DOUBLE => {
                if pos + 8 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Double entry truncated".to_string(),
                    ));
                }
                let bits = u64::from_be_bytes([
                    bytecode[pos],
                    bytecode[pos + 1],
                    bytecode[pos + 2],
                    bytecode[pos + 3],
                    bytecode[pos + 4],
                    bytecode[pos + 5],
                    bytecode[pos + 6],
                    bytecode[pos + 7],
                ]);
                pos += 8;
                // Double takes 2 slots - add placeholder for next slot
                entries.push(ConstantPoolItem::Double(f64::from_bits(bits)));
                entries.push(ConstantPoolItem::Utf8(String::new())); // Placeholder
                i += 2;
                continue;
            }
            tags::CLASS => {
                if pos + 2 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Class entry truncated".to_string(),
                    ));
                }
                let name_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                pos += 2;
                ConstantPoolItem::Class { name_index }
            }
            tags::STRING => {
                if pos + 2 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "String entry truncated".to_string(),
                    ));
                }
                let string_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                pos += 2;
                ConstantPoolItem::String { string_index }
            }
            tags::FIELDREF => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Fieldref entry truncated".to_string(),
                    ));
                }
                let class_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                let name_and_type_index =
                    u16::from_be_bytes([bytecode[pos + 2], bytecode[pos + 3]]);
                pos += 4;
                ConstantPoolItem::FieldRef {
                    class_index,
                    name_and_type_index,
                }
            }
            tags::METHODREF => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Methodref entry truncated".to_string(),
                    ));
                }
                let class_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                let name_and_type_index =
                    u16::from_be_bytes([bytecode[pos + 2], bytecode[pos + 3]]);
                pos += 4;
                ConstantPoolItem::MethodRef {
                    class_index,
                    name_and_type_index,
                }
            }
            tags::INTERFACE_METHODREF => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "InterfaceMethodref entry truncated".to_string(),
                    ));
                }
                let class_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                let name_and_type_index =
                    u16::from_be_bytes([bytecode[pos + 2], bytecode[pos + 3]]);
                pos += 4;
                ConstantPoolItem::InterfaceMethodRef {
                    class_index,
                    name_and_type_index,
                }
            }
            tags::NAME_AND_TYPE => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "NameAndType entry truncated".to_string(),
                    ));
                }
                let name_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                let descriptor_index = u16::from_be_bytes([bytecode[pos + 2], bytecode[pos + 3]]);
                pos += 4;
                ConstantPoolItem::NameAndType {
                    name_index,
                    descriptor_index,
                }
            }
            tags::METHOD_HANDLE => {
                if pos + 3 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "MethodHandle entry truncated".to_string(),
                    ));
                }
                let reference_kind = bytecode[pos];
                let reference_index = u16::from_be_bytes([bytecode[pos + 1], bytecode[pos + 2]]);
                pos += 3;
                ConstantPoolItem::MethodHandle {
                    reference_kind,
                    reference_index,
                }
            }
            tags::METHOD_TYPE => {
                if pos + 2 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "MethodType entry truncated".to_string(),
                    ));
                }
                let descriptor_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                pos += 2;
                ConstantPoolItem::MethodType { descriptor_index }
            }
            tags::DYNAMIC => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Dynamic entry truncated".to_string(),
                    ));
                }
                let bootstrap_method_attr_index =
                    u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                let name_and_type_index =
                    u16::from_be_bytes([bytecode[pos + 2], bytecode[pos + 3]]);
                pos += 4;
                ConstantPoolItem::Dynamic {
                    bootstrap_method_attr_index,
                    name_and_type_index,
                }
            }
            tags::INVOKE_DYNAMIC => {
                if pos + 4 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "InvokeDynamic entry truncated".to_string(),
                    ));
                }
                let bootstrap_method_attr_index =
                    u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                let name_and_type_index =
                    u16::from_be_bytes([bytecode[pos + 2], bytecode[pos + 3]]);
                pos += 4;
                ConstantPoolItem::InvokeDynamic {
                    bootstrap_method_attr_index,
                    name_and_type_index,
                }
            }
            tags::MODULE => {
                if pos + 2 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Module entry truncated".to_string(),
                    ));
                }
                let name_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                pos += 2;
                ConstantPoolItem::Module { name_index }
            }
            tags::PACKAGE => {
                if pos + 2 > bytecode.len() {
                    return Err(VMError::ClassFormatError(
                        "Package entry truncated".to_string(),
                    ));
                }
                let name_index = u16::from_be_bytes([bytecode[pos], bytecode[pos + 1]]);
                pos += 2;
                ConstantPoolItem::Package { name_index }
            }
            _ => {
                return Err(VMError::ClassFormatError(format!(
                    "Unknown constant pool tag {} at index {}",
                    tag, i
                )));
            }
        };

        entries.push(entry);
        i += 1;
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal class file with a simple constant pool
    fn build_simple_class(cp_entries: &[u8]) -> Vec<u8> {
        let mut bytecode = Vec::new();

        // Magic number
        bytecode.extend_from_slice(&[0xCA, 0xFE, 0xBA, 0xBE]);

        // Minor version (0)
        bytecode.extend_from_slice(&[0x00, 0x00]);

        // Major version (65 = Java 21)
        bytecode.extend_from_slice(&[0x00, 0x41]);

        // Constant pool count is calculated from cp_entries
        // This is a simplified test - in real class files this would be computed
        bytecode.extend_from_slice(&[0x00, 0x05]); // 5 entries (indices 0-4, but 0 unused)

        // Append the constant pool entries
        bytecode.extend_from_slice(cp_entries);

        bytecode
    }

    #[test]
    fn decodes_java_modified_utf8_surrogates_and_null() {
        let encoded = [b'A', 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80, 0xc0, 0x80, b'B'];
        assert_eq!(decode_modified_utf8(&encoded).unwrap(), "A😀\0B");
        assert!(decode_modified_utf8(&[0xed, 0xa0]).is_err());
    }

    #[test]
    fn test_parse_utf8() {
        // CP entry: tag=1 (UTF8), length=5, "Hello"
        let cp = &[
            1, 0, 5, b'H', b'e', b'l', b'l', b'o', // Entry 1: UTF8 "Hello"
            1, 0, 5, b'W', b'o', b'r', b'l', b'd', // Entry 2: UTF8 "World"
            1, 0, 0, // Entry 3: Empty UTF8
            1, 0, 1, b'!', // Entry 4: UTF8 "!"
        ];

        let bytecode = build_simple_class(cp);
        let result = parse_constant_pool(&bytecode).unwrap();

        // Index 0 is placeholder
        assert!(matches!(&result[0], ConstantPoolItem::Utf8(s) if s.is_empty()));

        // Actual entries
        assert!(matches!(&result[1], ConstantPoolItem::Utf8(s) if s == "Hello"));
        assert!(matches!(&result[2], ConstantPoolItem::Utf8(s) if s == "World"));
        assert!(matches!(&result[3], ConstantPoolItem::Utf8(s) if s.is_empty()));
        assert!(matches!(&result[4], ConstantPoolItem::Utf8(s) if s == "!"));
    }

    #[test]
    fn test_parse_integer() {
        // CP entry: tag=3 (INTEGER), value=42 (big-endian)
        let cp = &[
            3, 0, 0, 0, 42, // Entry 1: Integer 42
            3, 0xFF, 0xFF, 0xFF, 0xFF, // Entry 2: Integer -1
            1, 0, 4, b't', b'e', b's', b't', // Entry 3: UTF8 "test"
            3, 0, 0, 1, 0, // Entry 4: Integer 256
        ];

        let bytecode = build_simple_class(cp);
        let result = parse_constant_pool(&bytecode).unwrap();

        assert!(matches!(&result[1], ConstantPoolItem::Integer(42)));
        assert!(matches!(&result[2], ConstantPoolItem::Integer(-1)));
        assert!(matches!(&result[4], ConstantPoolItem::Integer(256)));
    }

    #[test]
    fn test_parse_class_ref() {
        // CP entry: tag=7 (CLASS), name_index=2
        // Then UTF8 at index 2 with class name
        let cp = &[
            7, 0, 2, // Entry 1: Class, name_index=2
            1, 0, 16, b'j', b'a', b'v', b'a', b'/', b'l', b'a', b'n', b'g', b'/', b'O', b'b', b'j',
            b'e', b'c', b't', // Entry 2: UTF8 "java/lang/Object"
            8, 0, 4, // Entry 3: String, string_index=4
            1, 0, 5, b'H', b'e', b'l', b'l', b'o', // Entry 4: UTF8 "Hello"
        ];

        let bytecode = build_simple_class(cp);
        let result = parse_constant_pool(&bytecode).unwrap();

        assert!(matches!(
            &result[1],
            ConstantPoolItem::Class { name_index: 2 }
        ));
        assert!(matches!(&result[2], ConstantPoolItem::Utf8(s) if s == "java/lang/Object"));
        assert!(matches!(
            &result[3],
            ConstantPoolItem::String { string_index: 4 }
        ));
        assert!(matches!(&result[4], ConstantPoolItem::Utf8(s) if s == "Hello"));
    }

    #[test]
    fn test_invalid_magic() {
        let bytecode = vec![0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 65, 0, 1];
        let result = parse_constant_pool(&bytecode);
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_file() {
        let bytecode = vec![0xCA, 0xFE, 0xBA, 0xBE];
        let result = parse_constant_pool(&bytecode);
        assert!(result.is_err());
    }
}
