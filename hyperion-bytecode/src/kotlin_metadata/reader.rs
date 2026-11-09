// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Reader for the `kotlin.Metadata` class-file annotation.
//!
//! Kotlin stores a length-delimited JVM string-table description followed by a
//! protobuf `Class`, `Package`, or `Function` message in `d1`. The protobuf
//! schema is intentionally decoded here instead of pulling a full protobuf
//! runtime into the bytecode crate.

use cafebabe::{
    attributes::{Annotation, AnnotationElementValue, AttributeData},
    ClassFile,
};
use thiserror::Error;

const KOTLIN_METADATA_DESCRIPTOR: &str = "Lkotlin/Metadata;";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KotlinMetadataKind {
    Class,
    FileFacade,
    SyntheticClass,
    MultiFileClassFacade,
    MultiFileClassPart,
    Unknown(i32),
}

impl From<i32> for KotlinMetadataKind {
    fn from(value: i32) -> Self {
        match value {
            1 => Self::Class,
            2 => Self::FileFacade,
            3 => Self::SyntheticClass,
            4 => Self::MultiFileClassFacade,
            5 => Self::MultiFileClassPart,
            value => Self::Unknown(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinMetadataAnnotation {
    pub kind: KotlinMetadataKind,
    pub metadata_version: Vec<i32>,
    pub data1: Vec<String>,
    pub data2: Vec<String>,
    pub extra_string: Option<String>,
    /// Package name (`pn` in the actual annotation; `pi` is also accepted for
    /// compatibility with early/non-standard emitters).
    pub package_name: Option<String>,
    pub extra_int: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KotlinClassifier {
    Class(String),
    TypeParameter(u32),
    TypeParameterName(String),
    TypeAlias(String),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KotlinVariance {
    In,
    Out,
    Invariant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinTypeArgument {
    pub variance: KotlinVariance,
    /// `None` represents a star projection.
    pub ty: Option<KotlinTypeInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinTypeInfo {
    pub classifier: KotlinClassifier,
    pub arguments: Vec<KotlinTypeArgument>,
    pub nullable: bool,
    pub is_suspend: bool,
}

impl KotlinTypeInfo {
    pub fn class_name(&self) -> Option<&str> {
        match &self.classifier {
            KotlinClassifier::Class(name) | KotlinClassifier::TypeAlias(name) => Some(name),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinTypeParameterInfo {
    pub id: u32,
    pub name: String,
    pub variance: KotlinVariance,
    pub is_reified: bool,
    pub upper_bounds: Vec<KotlinTypeInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinParameterInfo {
    pub name: String,
    pub ty: KotlinTypeInfo,
    pub vararg_element_type: Option<KotlinTypeInfo>,
    pub has_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinJvmMethodSignature {
    pub name: String,
    pub descriptor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinJvmFieldSignature {
    pub name: String,
    pub descriptor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KotlinJvmPropertySignature {
    pub field: Option<KotlinJvmFieldSignature>,
    pub synthetic_method: Option<KotlinJvmMethodSignature>,
    pub getter: Option<KotlinJvmMethodSignature>,
    pub setter: Option<KotlinJvmMethodSignature>,
    pub delegate_method: Option<KotlinJvmMethodSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinFunctionInfo {
    pub name: String,
    pub parameters: Vec<KotlinParameterInfo>,
    pub return_type: KotlinTypeInfo,
    pub type_parameters: Vec<KotlinTypeParameterInfo>,
    pub extension_receiver_type: Option<KotlinTypeInfo>,
    pub is_suspend: bool,
    pub is_operator: bool,
    pub is_inline: bool,
    pub jvm_signature: Option<KotlinJvmMethodSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinPropertyInfo {
    pub name: String,
    pub ty: KotlinTypeInfo,
    pub extension_receiver_type: Option<KotlinTypeInfo>,
    pub is_val: bool,
    pub has_getter: bool,
    pub has_setter: bool,
    pub jvm_signature: Option<KotlinJvmPropertySignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinConstructorInfo {
    pub parameters: Vec<KotlinParameterInfo>,
    pub is_secondary: bool,
    pub jvm_signature: Option<KotlinJvmMethodSignature>,
}

/// A typealias declared at package level, or in a class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinTypeAliasInfo {
    /// Simple name of the alias, as written in its declaration.
    pub name: String,
    /// The alias's own type parameters, in declaration order: `E` for
    /// `typealias ArrayList<E> = java.util.ArrayList<E>`. The expanded type
    /// refers to them by id.
    pub type_parameters: Vec<KotlinTypeParameterInfo>,
    /// The type the alias expands to, for example `java.util.ArrayList<E>`.
    pub expanded_type: KotlinTypeInfo,
    /// Declared `public` (the default), so code in another module can name it. Private and
    /// internal aliases are not.
    pub is_public: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KotlinClassInfo {
    pub name: String,
    pub metadata: KotlinMetadataAnnotation,
    pub is_data_class: bool,
    pub is_sealed: bool,
    pub is_object: bool,
    pub is_companion_object: bool,
    pub properties: Vec<KotlinPropertyInfo>,
    pub functions: Vec<KotlinFunctionInfo>,
    pub constructors: Vec<KotlinConstructorInfo>,
    pub type_parameters: Vec<KotlinTypeParameterInfo>,
    pub supertypes: Vec<KotlinTypeInfo>,
    /// Typealiases declared by the package or class (see `KotlinTypeAliasInfo`).
    pub type_aliases: Vec<KotlinTypeAliasInfo>,
    /// Simple names of the nested classes, as written (`Map.Entry` is `Entry`).
    pub nested_classes: Vec<String>,
    /// Names of the enum's entries, in declaration order.
    pub enum_entries: Vec<String>,
    /// Dotted names of the direct subclasses of a sealed class.
    pub sealed_subclasses: Vec<String>,
    /// For kind 4, `d1` contains the internal names of the facade's parts
    /// rather than a protobuf message.
    pub multi_file_part_names: Vec<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum KotlinMetadataError {
    #[error("invalid kotlin.Metadata annotation: {0}")]
    InvalidAnnotation(&'static str),
    #[error("truncated Kotlin metadata protobuf")]
    Truncated,
    #[error("invalid Kotlin metadata protobuf wire type {0}")]
    InvalidWireType(u8),
    #[error("invalid UTF-8 in Kotlin metadata string table")]
    InvalidUtf8,
    #[error("Kotlin metadata string index {0} is out of bounds")]
    StringIndex(usize),
    #[error("Kotlin metadata type index {0} is out of bounds")]
    TypeIndex(usize),
}

pub struct KotlinClassMetadata;

impl KotlinClassMetadata {
    /// Read Kotlin metadata, returning `None` both for Java classes and for
    /// malformed metadata. Use [`read_strict`](Self::read_strict) when the
    /// distinction matters.
    pub fn read(class_file: &ClassFile<'_>) -> Option<KotlinClassInfo> {
        Self::read_strict(class_file).ok().flatten()
    }

    pub fn read_strict(
        class_file: &ClassFile<'_>,
    ) -> Result<Option<KotlinClassInfo>, KotlinMetadataError> {
        let Some(annotation) = find_metadata_annotation(class_file) else {
            return Ok(None);
        };
        let header = parse_metadata_annotation(annotation)?;
        decode_metadata(class_file, header).map(Some)
    }

    pub fn annotation(class_file: &ClassFile<'_>) -> Option<KotlinMetadataAnnotation> {
        let annotation = find_metadata_annotation(class_file)?;
        parse_metadata_annotation(annotation).ok()
    }
}

fn find_metadata_annotation<'a, 'b>(class_file: &'b ClassFile<'a>) -> Option<&'b Annotation<'a>> {
    class_file.attributes.iter().find_map(|attribute| {
        let annotations = match &attribute.data {
            AttributeData::RuntimeVisibleAnnotations(annotations)
            | AttributeData::RuntimeInvisibleAnnotations(annotations) => annotations,
            _ => return None,
        };
        annotations
            .iter()
            .find(|annotation| annotation.type_descriptor.to_string() == KOTLIN_METADATA_DESCRIPTOR)
    })
}

fn parse_metadata_annotation(
    annotation: &Annotation<'_>,
) -> Result<KotlinMetadataAnnotation, KotlinMetadataError> {
    let mut kind = 1;
    let mut metadata_version = Vec::new();
    let mut data1 = Vec::new();
    let mut data2 = Vec::new();
    let mut extra_string = None;
    let mut package_name = None;
    let mut extra_int = 0;

    for element in &annotation.elements {
        match element.name.as_ref() {
            "k" => kind = annotation_int(&element.value)?,
            "mv" => metadata_version = annotation_int_array(&element.value)?,
            "d1" => data1 = annotation_string_array(&element.value)?,
            "d2" => data2 = annotation_string_array(&element.value)?,
            "xs" => extra_string = Some(annotation_string(&element.value)?),
            "pn" | "pi" => package_name = Some(annotation_string(&element.value)?),
            "xi" => extra_int = annotation_int(&element.value)?,
            _ => {}
        }
    }

    Ok(KotlinMetadataAnnotation {
        kind: kind.into(),
        metadata_version,
        data1,
        data2,
        extra_string,
        package_name,
        extra_int,
    })
}

fn annotation_int(value: &AnnotationElementValue<'_>) -> Result<i32, KotlinMetadataError> {
    match value {
        AnnotationElementValue::IntConstant(value) => Ok(*value),
        _ => Err(KotlinMetadataError::InvalidAnnotation(
            "expected integer element",
        )),
    }
}

fn annotation_string(value: &AnnotationElementValue<'_>) -> Result<String, KotlinMetadataError> {
    match value {
        AnnotationElementValue::StringConstant(value) => Ok(value.to_string()),
        _ => Err(KotlinMetadataError::InvalidAnnotation(
            "expected string element",
        )),
    }
}

fn annotation_int_array(
    value: &AnnotationElementValue<'_>,
) -> Result<Vec<i32>, KotlinMetadataError> {
    match value {
        AnnotationElementValue::ArrayValue(values) => values.iter().map(annotation_int).collect(),
        _ => Err(KotlinMetadataError::InvalidAnnotation(
            "expected integer array element",
        )),
    }
}

fn annotation_string_array(
    value: &AnnotationElementValue<'_>,
) -> Result<Vec<String>, KotlinMetadataError> {
    match value {
        AnnotationElementValue::ArrayValue(values) => {
            values.iter().map(annotation_string).collect()
        }
        _ => Err(KotlinMetadataError::InvalidAnnotation(
            "expected string array element",
        )),
    }
}

#[derive(Clone, Default)]
struct StringRecord {
    range: usize,
    predefined_index: Option<usize>,
    string: Option<String>,
    operation: u32,
    substring: Vec<u32>,
    replace_char: Vec<u32>,
}

struct NameResolver {
    strings: Vec<String>,
    records: Vec<StringRecord>,
}

impl NameResolver {
    fn parse(
        bytes: &mut ProtoReader<'_>,
        strings: Vec<String>,
    ) -> Result<Self, KotlinMetadataError> {
        let table_bytes = bytes.read_len()?;
        let mut table = ProtoReader::new(table_bytes);
        let mut compact_records = Vec::new();
        while let Some((field, wire)) = table.next_key()? {
            match (field, wire) {
                (1, 2) => compact_records.push(parse_string_record(table.read_len()?)?),
                _ => table.skip(wire)?,
            }
        }

        let mut records = Vec::new();
        for record in compact_records {
            let range = record.range.max(1);
            records.extend(std::iter::repeat_n(record, range));
        }
        while records.len() < strings.len() {
            records.push(StringRecord {
                range: 1,
                ..StringRecord::default()
            });
        }

        Ok(Self { strings, records })
    }

    fn get(&self, index: u32) -> Result<String, KotlinMetadataError> {
        let index = index as usize;
        let record = self.records.get(index);
        let mut value = if let Some(value) = record.and_then(|record| record.string.clone()) {
            value
        } else if let Some(predefined) = record.and_then(|record| record.predefined_index) {
            PREDEFINED_STRINGS
                .get(predefined)
                .ok_or(KotlinMetadataError::StringIndex(index))?
                .to_string()
        } else {
            self.strings
                .get(index)
                .ok_or(KotlinMetadataError::StringIndex(index))?
                .clone()
        };

        if let Some(record) = record {
            if record.substring.len() >= 2 {
                let begin = record.substring[0] as usize;
                let end = record.substring[1] as usize;
                let chars: Vec<char> = value.chars().collect();
                if begin <= end && end <= chars.len() {
                    value = chars[begin..end].iter().collect();
                }
            }
            if record.replace_char.len() >= 2 {
                let from = char::from_u32(record.replace_char[0]);
                let to = char::from_u32(record.replace_char[1]);
                if let (Some(from), Some(to)) = (from, to) {
                    value = value.replace(from, &to.to_string());
                }
            }
            match record.operation {
                1 => value = value.replace('$', "."),
                2 => {
                    if value.len() >= 2 {
                        value = value[1..value.len() - 1].to_string();
                    }
                    value = value.replace('$', ".");
                }
                _ => {}
            }
        }
        Ok(value)
    }

    fn class_name(&self, index: u32) -> Result<String, KotlinMetadataError> {
        Ok(self.get(index)?.replace('/', "."))
    }
}

fn parse_string_record(bytes: &[u8]) -> Result<StringRecord, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut record = StringRecord {
        range: 1,
        ..StringRecord::default()
    };
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => record.range = reader.read_varint()? as usize,
            (2, 0) => record.predefined_index = Some(reader.read_varint()? as usize),
            (3, 0) => record.operation = reader.read_varint()? as u32,
            (4, 0) => record.substring.push(reader.read_varint()? as u32),
            (4, 2) => record
                .substring
                .extend(read_packed_varints(reader.read_len()?)?),
            (5, 0) => record.replace_char.push(reader.read_varint()? as u32),
            (5, 2) => record
                .replace_char
                .extend(read_packed_varints(reader.read_len()?)?),
            (6, 2) => {
                record.string = Some(
                    std::str::from_utf8(reader.read_len()?)
                        .map_err(|_| KotlinMetadataError::InvalidUtf8)?
                        .to_string(),
                )
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(record)
}

const PREDEFINED_STRINGS: &[&str] = &[
    "kotlin/Any",
    "kotlin/Nothing",
    "kotlin/Unit",
    "kotlin/Throwable",
    "kotlin/Number",
    "kotlin/Byte",
    "kotlin/Double",
    "kotlin/Float",
    "kotlin/Int",
    "kotlin/Long",
    "kotlin/Short",
    "kotlin/Boolean",
    "kotlin/Char",
    "kotlin/CharSequence",
    "kotlin/String",
    "kotlin/Comparable",
    "kotlin/Enum",
    "kotlin/Array",
    "kotlin/ByteArray",
    "kotlin/DoubleArray",
    "kotlin/FloatArray",
    "kotlin/IntArray",
    "kotlin/LongArray",
    "kotlin/ShortArray",
    "kotlin/BooleanArray",
    "kotlin/CharArray",
    "kotlin/Cloneable",
    "kotlin/Annotation",
    "kotlin/collections/Iterable",
    "kotlin/collections/MutableIterable",
    "kotlin/collections/Collection",
    "kotlin/collections/MutableCollection",
    "kotlin/collections/List",
    "kotlin/collections/MutableList",
    "kotlin/collections/Set",
    "kotlin/collections/MutableSet",
    "kotlin/collections/Map",
    "kotlin/collections/MutableMap",
    "kotlin/collections/Map.Entry",
    "kotlin/collections/MutableMap.MutableEntry",
    "kotlin/collections/Iterator",
    "kotlin/collections/MutableIterator",
    "kotlin/collections/ListIterator",
    "kotlin/collections/MutableListIterator",
];

#[derive(Clone, Default)]
struct RawType {
    flags: u32,
    arguments: Vec<RawTypeArgument>,
    nullable: bool,
    class_name: Option<u32>,
    type_parameter: Option<u32>,
    type_parameter_name: Option<u32>,
    type_alias_name: Option<u32>,
}

#[derive(Clone)]
struct RawTypeArgument {
    projection: u32,
    ty: Option<RawTypeRef>,
}

#[derive(Clone)]
enum RawTypeRef {
    Inline(RawType),
    Id(u32),
}

#[derive(Clone, Default)]
struct RawTypeTable {
    types: Vec<RawType>,
    first_nullable: Option<usize>,
}

#[derive(Clone, Default)]
struct RawTypeParameter {
    id: u32,
    name: u32,
    reified: bool,
    variance: u32,
    upper_bounds: Vec<RawTypeRef>,
}

#[derive(Clone, Default)]
struct RawValueParameter {
    flags: u32,
    name: u32,
    ty: Option<RawTypeRef>,
    vararg_element_type: Option<RawTypeRef>,
}

#[derive(Clone, Default)]
struct RawJvmMethodSignature {
    name: Option<u32>,
    descriptor: Option<u32>,
}

#[derive(Clone, Default)]
struct RawJvmFieldSignature {
    name: Option<u32>,
    descriptor: Option<u32>,
}

#[derive(Clone, Default)]
struct RawJvmPropertySignature {
    field: Option<RawJvmFieldSignature>,
    synthetic_method: Option<RawJvmMethodSignature>,
    getter: Option<RawJvmMethodSignature>,
    setter: Option<RawJvmMethodSignature>,
    delegate_method: Option<RawJvmMethodSignature>,
}

#[derive(Clone, Default)]
struct RawFunction {
    flags: Option<u32>,
    old_flags: Option<u32>,
    name: u32,
    return_type: Option<RawTypeRef>,
    type_parameters: Vec<RawTypeParameter>,
    receiver_type: Option<RawTypeRef>,
    value_parameters: Vec<RawValueParameter>,
    type_table: Option<RawTypeTable>,
    jvm_signature: Option<RawJvmMethodSignature>,
}

#[derive(Clone, Default)]
struct RawProperty {
    flags: Option<u32>,
    old_flags: Option<u32>,
    name: u32,
    return_type: Option<RawTypeRef>,
    receiver_type: Option<RawTypeRef>,
    type_parameters: Vec<RawTypeParameter>,
    jvm_signature: Option<RawJvmPropertySignature>,
}

#[derive(Clone, Default)]
struct RawConstructor {
    flags: u32,
    value_parameters: Vec<RawValueParameter>,
    jvm_signature: Option<RawJvmMethodSignature>,
}

#[derive(Default)]
struct RawClass {
    flags: u32,
    fq_name: Option<u32>,
    type_parameters: Vec<RawTypeParameter>,
    supertypes: Vec<RawTypeRef>,
    constructors: Vec<RawConstructor>,
    functions: Vec<RawFunction>,
    properties: Vec<RawProperty>,
    type_aliases: Vec<RawTypeAlias>,
    nested_class_names: Vec<u32>,
    enum_entry_names: Vec<u32>,
    sealed_subclass_names: Vec<u32>,
    type_table: RawTypeTable,
}

#[derive(Default)]
struct RawPackage {
    functions: Vec<RawFunction>,
    properties: Vec<RawProperty>,
    type_aliases: Vec<RawTypeAlias>,
    type_table: RawTypeTable,
}

/// A TypeAlias message. Field numbers are those of kotlin-metadata-jvm's `ProtoBuf.TypeAlias`.
#[derive(Default)]
struct RawTypeAlias {
    flags: u32,
    name: Option<u32>,
    type_parameters: Vec<RawTypeParameter>,
    expanded_type: Option<RawTypeRef>,
}

fn parse_type(bytes: &[u8]) -> Result<RawType, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut ty = RawType::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => ty.flags = reader.read_varint()? as u32,
            (2, 2) => ty.arguments.push(parse_type_argument(reader.read_len()?)?),
            (3, 0) => ty.nullable = reader.read_varint()? != 0,
            (6, 0) => ty.class_name = Some(reader.read_varint()? as u32),
            (7, 0) => ty.type_parameter = Some(reader.read_varint()? as u32),
            (9, 0) => ty.type_parameter_name = Some(reader.read_varint()? as u32),
            (12, 0) => ty.type_alias_name = Some(reader.read_varint()? as u32),
            _ => reader.skip(wire)?,
        }
    }
    Ok(ty)
}

fn parse_type_argument(bytes: &[u8]) -> Result<RawTypeArgument, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut projection = 2;
    let mut ty = None;
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => projection = reader.read_varint()? as u32,
            (2, 2) => ty = Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?)),
            (3, 0) => ty = Some(RawTypeRef::Id(reader.read_varint()? as u32)),
            _ => reader.skip(wire)?,
        }
    }
    Ok(RawTypeArgument { projection, ty })
}

fn parse_type_parameter(bytes: &[u8]) -> Result<RawTypeParameter, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut parameter = RawTypeParameter {
        variance: 2,
        ..RawTypeParameter::default()
    };
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => parameter.id = reader.read_varint()? as u32,
            (2, 0) => parameter.name = reader.read_varint()? as u32,
            (3, 0) => parameter.reified = reader.read_varint()? != 0,
            (4, 0) => parameter.variance = reader.read_varint()? as u32,
            (5, 2) => parameter
                .upper_bounds
                .push(RawTypeRef::Inline(parse_type(reader.read_len()?)?)),
            (6, 0) => parameter
                .upper_bounds
                .push(RawTypeRef::Id(reader.read_varint()? as u32)),
            (6, 2) => parameter.upper_bounds.extend(
                read_packed_varints(reader.read_len()?)?
                    .into_iter()
                    .map(RawTypeRef::Id),
            ),
            _ => reader.skip(wire)?,
        }
    }
    Ok(parameter)
}

fn parse_value_parameter(bytes: &[u8]) -> Result<RawValueParameter, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut parameter = RawValueParameter::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => parameter.flags = reader.read_varint()? as u32,
            (2, 0) => parameter.name = reader.read_varint()? as u32,
            (3, 2) => parameter.ty = Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?)),
            (4, 2) => {
                parameter.vararg_element_type =
                    Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?))
            }
            (5, 0) => parameter.ty = Some(RawTypeRef::Id(reader.read_varint()? as u32)),
            (6, 0) => {
                parameter.vararg_element_type = Some(RawTypeRef::Id(reader.read_varint()? as u32))
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(parameter)
}

fn parse_type_table(bytes: &[u8]) -> Result<RawTypeTable, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut table = RawTypeTable::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 2) => table.types.push(parse_type(reader.read_len()?)?),
            (2, 0) => table.first_nullable = Some(reader.read_varint()? as usize),
            _ => reader.skip(wire)?,
        }
    }
    Ok(table)
}

fn parse_jvm_method_signature(bytes: &[u8]) -> Result<RawJvmMethodSignature, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut signature = RawJvmMethodSignature::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => signature.name = Some(reader.read_varint()? as u32),
            (2, 0) => signature.descriptor = Some(reader.read_varint()? as u32),
            _ => reader.skip(wire)?,
        }
    }
    Ok(signature)
}

fn parse_jvm_field_signature(bytes: &[u8]) -> Result<RawJvmFieldSignature, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut signature = RawJvmFieldSignature::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => signature.name = Some(reader.read_varint()? as u32),
            (2, 0) => signature.descriptor = Some(reader.read_varint()? as u32),
            _ => reader.skip(wire)?,
        }
    }
    Ok(signature)
}

fn parse_jvm_property_signature(
    bytes: &[u8],
) -> Result<RawJvmPropertySignature, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut signature = RawJvmPropertySignature::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 2) => signature.field = Some(parse_jvm_field_signature(reader.read_len()?)?),
            (2, 2) => {
                signature.synthetic_method = Some(parse_jvm_method_signature(reader.read_len()?)?)
            }
            (3, 2) => signature.getter = Some(parse_jvm_method_signature(reader.read_len()?)?),
            (4, 2) => signature.setter = Some(parse_jvm_method_signature(reader.read_len()?)?),
            (5, 2) => {
                signature.delegate_method = Some(parse_jvm_method_signature(reader.read_len()?)?)
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(signature)
}

fn parse_function(bytes: &[u8]) -> Result<RawFunction, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut function = RawFunction::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => function.old_flags = Some(reader.read_varint()? as u32),
            (2, 0) => function.name = reader.read_varint()? as u32,
            (3, 2) => {
                function.return_type = Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?))
            }
            (4, 2) => function
                .type_parameters
                .push(parse_type_parameter(reader.read_len()?)?),
            (5, 2) => {
                function.receiver_type = Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?))
            }
            (6, 2) => function
                .value_parameters
                .push(parse_value_parameter(reader.read_len()?)?),
            (7, 0) => function.return_type = Some(RawTypeRef::Id(reader.read_varint()? as u32)),
            (8, 0) => function.receiver_type = Some(RawTypeRef::Id(reader.read_varint()? as u32)),
            (9, 0) => function.flags = Some(reader.read_varint()? as u32),
            (30, 2) => function.type_table = Some(parse_type_table(reader.read_len()?)?),
            (100, 2) => {
                function.jvm_signature = Some(parse_jvm_method_signature(reader.read_len()?)?)
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(function)
}

fn parse_property(bytes: &[u8]) -> Result<RawProperty, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut property = RawProperty::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => property.old_flags = Some(reader.read_varint()? as u32),
            (2, 0) => property.name = reader.read_varint()? as u32,
            (3, 2) => {
                property.return_type = Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?))
            }
            (4, 2) => property
                .type_parameters
                .push(parse_type_parameter(reader.read_len()?)?),
            (5, 2) => {
                property.receiver_type = Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?))
            }
            (9, 0) => property.return_type = Some(RawTypeRef::Id(reader.read_varint()? as u32)),
            (10, 0) => property.receiver_type = Some(RawTypeRef::Id(reader.read_varint()? as u32)),
            (11, 0) => property.flags = Some(reader.read_varint()? as u32),
            (100, 2) => {
                property.jvm_signature = Some(parse_jvm_property_signature(reader.read_len()?)?)
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(property)
}

fn parse_constructor(bytes: &[u8]) -> Result<RawConstructor, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut constructor = RawConstructor {
        flags: 6,
        ..RawConstructor::default()
    };
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => constructor.flags = reader.read_varint()? as u32,
            (2, 2) => constructor
                .value_parameters
                .push(parse_value_parameter(reader.read_len()?)?),
            (100, 2) => {
                constructor.jvm_signature = Some(parse_jvm_method_signature(reader.read_len()?)?)
            }
            _ => reader.skip(wire)?,
        }
    }
    Ok(constructor)
}

fn parse_class(bytes: &[u8]) -> Result<RawClass, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut class = RawClass {
        flags: 6,
        ..RawClass::default()
    };
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => class.flags = reader.read_varint()? as u32,
            (2, 0) => class
                .supertypes
                .push(RawTypeRef::Id(reader.read_varint()? as u32)),
            (2, 2) => class.supertypes.extend(
                read_packed_varints(reader.read_len()?)?
                    .into_iter()
                    .map(RawTypeRef::Id),
            ),
            (3, 0) => class.fq_name = Some(reader.read_varint()? as u32),
            (5, 2) => class
                .type_parameters
                .push(parse_type_parameter(reader.read_len()?)?),
            (6, 2) => class
                .supertypes
                .push(RawTypeRef::Inline(parse_type(reader.read_len()?)?)),
            (8, 2) => class
                .constructors
                .push(parse_constructor(reader.read_len()?)?),
            (9, 2) => class.functions.push(parse_function(reader.read_len()?)?),
            (10, 2) => class.properties.push(parse_property(reader.read_len()?)?),
            (11, 2) => class
                .type_aliases
                .push(parse_type_alias(reader.read_len()?)?),
            (7, 0) => class.nested_class_names.push(reader.read_varint()? as u32),
            (7, 2) => class
                .nested_class_names
                .extend(read_packed_varints(reader.read_len()?)?),
            (13, 2) => {
                if let Some(name) = parse_enum_entry(reader.read_len()?)? {
                    class.enum_entry_names.push(name);
                }
            }
            (16, 0) => class
                .sealed_subclass_names
                .push(reader.read_varint()? as u32),
            (16, 2) => class
                .sealed_subclass_names
                .extend(read_packed_varints(reader.read_len()?)?),
            (30, 2) => class.type_table = parse_type_table(reader.read_len()?)?,
            _ => reader.skip(wire)?,
        }
    }
    Ok(class)
}

/// Parses an EnumEntry message. Its name is a string index (field 1 in kotlin-metadata-jvm's
/// `ProtoBuf.EnumEntry`); annotations on the entry are not needed.
fn parse_enum_entry(bytes: &[u8]) -> Result<Option<u32>, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut name = None;
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => name = Some(reader.read_varint()? as u32),
            _ => reader.skip(wire)?,
        }
    }
    Ok(name)
}

/// Parses a TypeAlias message: its flags, its name (a string index), its type parameters
/// (field 3), and its expanded type, inline or by index into the type table. The underlying
/// type is the unexpanded form and is not needed. Absent flags default to 6, which is public
/// with no annotations.
fn parse_type_alias(bytes: &[u8]) -> Result<RawTypeAlias, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut alias = RawTypeAlias {
        flags: 6,
        ..RawTypeAlias::default()
    };
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (1, 0) => alias.flags = reader.read_varint()? as u32,
            (2, 0) => alias.name = Some(reader.read_varint()? as u32),
            (3, 2) => alias
                .type_parameters
                .push(parse_type_parameter(reader.read_len()?)?),
            (6, 2) => {
                alias.expanded_type = Some(RawTypeRef::Inline(parse_type(reader.read_len()?)?));
            }
            (7, 0) => alias.expanded_type = Some(RawTypeRef::Id(reader.read_varint()? as u32)),
            _ => reader.skip(wire)?,
        }
    }
    Ok(alias)
}

fn parse_package(bytes: &[u8]) -> Result<RawPackage, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut package = RawPackage::default();
    while let Some((field, wire)) = reader.next_key()? {
        match (field, wire) {
            (3, 2) => package.functions.push(parse_function(reader.read_len()?)?),
            (4, 2) => package.properties.push(parse_property(reader.read_len()?)?),
            (5, 2) => package
                .type_aliases
                .push(parse_type_alias(reader.read_len()?)?),
            (30, 2) => package.type_table = parse_type_table(reader.read_len()?)?,
            _ => reader.skip(wire)?,
        }
    }
    Ok(package)
}

fn decode_metadata(
    class_file: &ClassFile<'_>,
    header: KotlinMetadataAnnotation,
) -> Result<KotlinClassInfo, KotlinMetadataError> {
    let class_file_name = class_file.this_class.replace('/', ".");
    if header.kind == KotlinMetadataKind::MultiFileClassFacade {
        return Ok(empty_class_info(
            class_file_name,
            header.clone(),
            header.data1.clone(),
        ));
    }
    if header.data1.is_empty() {
        return Ok(empty_class_info(class_file_name, header, Vec::new()));
    }

    let decoded = decode_data1(&header.data1)?;
    let mut reader = ProtoReader::new(&decoded);
    let resolver = NameResolver::parse(&mut reader, header.data2.clone())?;
    let message = reader.remaining();

    match header.kind {
        KotlinMetadataKind::Class => {
            let raw = parse_class(message)?;
            class_info_from_raw(class_file_name, header, &resolver, raw)
        }
        KotlinMetadataKind::FileFacade | KotlinMetadataKind::MultiFileClassPart => {
            let raw = parse_package(message)?;
            package_info_from_raw(class_file_name, header, &resolver, raw)
        }
        KotlinMetadataKind::SyntheticClass => {
            let mut info = empty_class_info(class_file_name, header, Vec::new());
            if !message.is_empty() {
                let raw = parse_function(message)?;
                info.functions
                    .push(resolve_function(raw, &resolver, &RawTypeTable::default())?);
            }
            Ok(info)
        }
        KotlinMetadataKind::MultiFileClassFacade | KotlinMetadataKind::Unknown(_) => {
            Ok(empty_class_info(class_file_name, header, Vec::new()))
        }
    }
}

fn empty_class_info(
    name: String,
    metadata: KotlinMetadataAnnotation,
    multi_file_part_names: Vec<String>,
) -> KotlinClassInfo {
    KotlinClassInfo {
        name,
        metadata,
        is_data_class: false,
        is_sealed: false,
        is_object: false,
        is_companion_object: false,
        properties: Vec::new(),
        functions: Vec::new(),
        constructors: Vec::new(),
        type_parameters: Vec::new(),
        supertypes: Vec::new(),
        type_aliases: Vec::new(),
        nested_classes: Vec::new(),
        enum_entries: Vec::new(),
        sealed_subclasses: Vec::new(),
        multi_file_part_names,
    }
}

fn class_info_from_raw(
    class_file_name: String,
    header: KotlinMetadataAnnotation,
    resolver: &NameResolver,
    raw: RawClass,
) -> Result<KotlinClassInfo, KotlinMetadataError> {
    let name = match raw.fq_name {
        Some(index) => resolver.class_name(index)?,
        None => class_file_name,
    };
    let source_kind = (raw.flags >> 6) & 0x7;
    let type_parameters = raw
        .type_parameters
        .iter()
        .map(|parameter| resolve_type_parameter(parameter, resolver, &raw.type_table))
        .collect::<Result<Vec<_>, _>>()?;
    let supertypes = raw
        .supertypes
        .iter()
        .map(|ty| resolve_type_ref(ty, resolver, &raw.type_table))
        .collect::<Result<Vec<_>, _>>()?;
    let constructors = raw
        .constructors
        .into_iter()
        .map(|constructor| resolve_constructor(constructor, resolver, &raw.type_table))
        .collect::<Result<Vec<_>, _>>()?;
    let functions = raw
        .functions
        .into_iter()
        .map(|function| resolve_function(function, resolver, &raw.type_table))
        .collect::<Result<Vec<_>, _>>()?;
    let properties = raw
        .properties
        .into_iter()
        .map(|property| resolve_property(property, resolver, &raw.type_table))
        .collect::<Result<Vec<_>, _>>()?;
    let type_aliases = resolve_type_aliases(raw.type_aliases, resolver, &raw.type_table)?;
    let nested_classes = raw
        .nested_class_names
        .iter()
        .map(|&index| resolver.get(index))
        .collect::<Result<Vec<_>, _>>()?;
    let enum_entries = raw
        .enum_entry_names
        .iter()
        .map(|&index| resolver.get(index))
        .collect::<Result<Vec<_>, _>>()?;
    let sealed_subclasses = raw
        .sealed_subclass_names
        .iter()
        .map(|&index| resolver.class_name(index))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(KotlinClassInfo {
        name,
        metadata: header,
        is_data_class: raw.flags & (1 << 10) != 0,
        is_sealed: (raw.flags >> 4) & 0x3 == 3,
        is_object: source_kind == 5,
        is_companion_object: source_kind == 6,
        properties,
        functions,
        constructors,
        type_parameters,
        supertypes,
        type_aliases,
        nested_classes,
        enum_entries,
        sealed_subclasses,
        multi_file_part_names: Vec::new(),
    })
}

fn resolve_type_aliases(
    aliases: Vec<RawTypeAlias>,
    resolver: &NameResolver,
    type_table: &RawTypeTable,
) -> Result<Vec<KotlinTypeAliasInfo>, KotlinMetadataError> {
    aliases
        .into_iter()
        .map(|alias| {
            let name = alias.name.ok_or(KotlinMetadataError::InvalidAnnotation(
                "type alias without a name",
            ))?;
            let expanded_type =
                alias
                    .expanded_type
                    .ok_or(KotlinMetadataError::InvalidAnnotation(
                        "type alias without an expanded type",
                    ))?;
            Ok(KotlinTypeAliasInfo {
                name: resolver.get(name)?,
                type_parameters: alias
                    .type_parameters
                    .iter()
                    .map(|parameter| resolve_type_parameter(parameter, resolver, type_table))
                    .collect::<Result<Vec<_>, _>>()?,
                expanded_type: resolve_type_ref(&expanded_type, resolver, type_table)?,
                // Bit 0 is hasAnnotations; bits 1-3 are the visibility, where 3 is PUBLIC.
                is_public: (alias.flags >> 1) & 0x7 == 3,
            })
        })
        .collect()
}

fn package_info_from_raw(
    name: String,
    header: KotlinMetadataAnnotation,
    resolver: &NameResolver,
    raw: RawPackage,
) -> Result<KotlinClassInfo, KotlinMetadataError> {
    let functions = raw
        .functions
        .into_iter()
        .map(|function| resolve_function(function, resolver, &raw.type_table))
        .collect::<Result<Vec<_>, _>>()?;
    let properties = raw
        .properties
        .into_iter()
        .map(|property| resolve_property(property, resolver, &raw.type_table))
        .collect::<Result<Vec<_>, _>>()?;
    let mut info = empty_class_info(name, header, Vec::new());
    info.functions = functions;
    info.properties = properties;
    info.type_aliases = resolve_type_aliases(raw.type_aliases, resolver, &raw.type_table)?;
    Ok(info)
}

fn resolve_type_ref(
    type_ref: &RawTypeRef,
    resolver: &NameResolver,
    table: &RawTypeTable,
) -> Result<KotlinTypeInfo, KotlinMetadataError> {
    match type_ref {
        RawTypeRef::Inline(ty) => resolve_type(ty, resolver, table, false),
        RawTypeRef::Id(index) => {
            let index = *index as usize;
            let ty = table
                .types
                .get(index)
                .ok_or(KotlinMetadataError::TypeIndex(index))?;
            let nullable_from_table = table
                .first_nullable
                .is_some_and(|first_nullable| index >= first_nullable);
            resolve_type(ty, resolver, table, nullable_from_table)
        }
    }
}

fn resolve_type(
    raw: &RawType,
    resolver: &NameResolver,
    table: &RawTypeTable,
    nullable_from_table: bool,
) -> Result<KotlinTypeInfo, KotlinMetadataError> {
    let classifier = if let Some(index) = raw.class_name {
        KotlinClassifier::Class(resolver.class_name(index)?)
    } else if let Some(id) = raw.type_parameter {
        KotlinClassifier::TypeParameter(id)
    } else if let Some(index) = raw.type_parameter_name {
        KotlinClassifier::TypeParameterName(resolver.get(index)?)
    } else if let Some(index) = raw.type_alias_name {
        KotlinClassifier::TypeAlias(resolver.class_name(index)?)
    } else {
        KotlinClassifier::Unknown
    };
    let arguments = raw
        .arguments
        .iter()
        .map(|argument| {
            let variance = match argument.projection {
                0 => KotlinVariance::In,
                1 => KotlinVariance::Out,
                _ => KotlinVariance::Invariant,
            };
            let ty = argument
                .ty
                .as_ref()
                .map(|ty| resolve_type_ref(ty, resolver, table))
                .transpose()?;
            Ok(KotlinTypeArgument { variance, ty })
        })
        .collect::<Result<Vec<_>, KotlinMetadataError>>()?;
    Ok(KotlinTypeInfo {
        classifier,
        arguments,
        nullable: raw.nullable || nullable_from_table,
        is_suspend: raw.flags & 1 != 0,
    })
}

fn unknown_type() -> KotlinTypeInfo {
    KotlinTypeInfo {
        classifier: KotlinClassifier::Unknown,
        arguments: Vec::new(),
        nullable: false,
        is_suspend: false,
    }
}

fn resolve_type_parameter(
    raw: &RawTypeParameter,
    resolver: &NameResolver,
    table: &RawTypeTable,
) -> Result<KotlinTypeParameterInfo, KotlinMetadataError> {
    Ok(KotlinTypeParameterInfo {
        id: raw.id,
        name: resolver.get(raw.name)?,
        variance: match raw.variance {
            0 => KotlinVariance::In,
            1 => KotlinVariance::Out,
            _ => KotlinVariance::Invariant,
        },
        is_reified: raw.reified,
        upper_bounds: raw
            .upper_bounds
            .iter()
            .map(|ty| resolve_type_ref(ty, resolver, table))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn resolve_value_parameter(
    raw: &RawValueParameter,
    resolver: &NameResolver,
    table: &RawTypeTable,
) -> Result<KotlinParameterInfo, KotlinMetadataError> {
    Ok(KotlinParameterInfo {
        name: resolver.get(raw.name)?,
        ty: raw
            .ty
            .as_ref()
            .map(|ty| resolve_type_ref(ty, resolver, table))
            .transpose()?
            .unwrap_or_else(unknown_type),
        vararg_element_type: raw
            .vararg_element_type
            .as_ref()
            .map(|ty| resolve_type_ref(ty, resolver, table))
            .transpose()?,
        has_default: raw.flags & (1 << 1) != 0,
    })
}

fn resolve_function(
    raw: RawFunction,
    resolver: &NameResolver,
    parent_table: &RawTypeTable,
) -> Result<KotlinFunctionInfo, KotlinMetadataError> {
    let name = resolver.get(raw.name)?;
    let flags = raw.flags.or(raw.old_flags).unwrap_or(6);
    let table = raw.type_table.as_ref().unwrap_or(parent_table);
    let parameters = raw
        .value_parameters
        .iter()
        .map(|parameter| resolve_value_parameter(parameter, resolver, table))
        .collect::<Result<Vec<_>, _>>()?;
    let type_parameters = raw
        .type_parameters
        .iter()
        .map(|parameter| resolve_type_parameter(parameter, resolver, table))
        .collect::<Result<Vec<_>, _>>()?;
    let return_type = raw
        .return_type
        .as_ref()
        .map(|ty| resolve_type_ref(ty, resolver, table))
        .transpose()?
        .unwrap_or_else(unknown_type);
    let extension_receiver_type = raw
        .receiver_type
        .as_ref()
        .map(|ty| resolve_type_ref(ty, resolver, table))
        .transpose()?;
    let jvm_signature = raw
        .jvm_signature
        .as_ref()
        .map(|signature| resolve_method_signature(signature, &name, resolver))
        .transpose()?;

    Ok(KotlinFunctionInfo {
        name,
        parameters,
        return_type,
        type_parameters,
        extension_receiver_type,
        is_operator: flags & (1 << 8) != 0,
        is_inline: flags & (1 << 10) != 0,
        is_suspend: flags & (1 << 13) != 0,
        jvm_signature,
    })
}

fn resolve_property(
    raw: RawProperty,
    resolver: &NameResolver,
    table: &RawTypeTable,
) -> Result<KotlinPropertyInfo, KotlinMetadataError> {
    let name = resolver.get(raw.name)?;
    let flags = raw.flags.or(raw.old_flags).unwrap_or(518);
    let ty = raw
        .return_type
        .as_ref()
        .map(|ty| resolve_type_ref(ty, resolver, table))
        .transpose()?
        .unwrap_or_else(unknown_type);
    let extension_receiver_type = raw
        .receiver_type
        .as_ref()
        .map(|ty| resolve_type_ref(ty, resolver, table))
        .transpose()?;
    let jvm_signature = raw
        .jvm_signature
        .as_ref()
        .map(|signature| resolve_property_signature(signature, &name, resolver))
        .transpose()?;
    Ok(KotlinPropertyInfo {
        name,
        ty,
        extension_receiver_type,
        is_val: flags & (1 << 8) == 0,
        has_getter: flags & (1 << 9) != 0,
        has_setter: flags & (1 << 10) != 0,
        jvm_signature,
    })
}

fn resolve_constructor(
    raw: RawConstructor,
    resolver: &NameResolver,
    table: &RawTypeTable,
) -> Result<KotlinConstructorInfo, KotlinMetadataError> {
    let parameters = raw
        .value_parameters
        .iter()
        .map(|parameter| resolve_value_parameter(parameter, resolver, table))
        .collect::<Result<Vec<_>, _>>()?;
    let jvm_signature = raw
        .jvm_signature
        .as_ref()
        .map(|signature| resolve_method_signature(signature, "<init>", resolver))
        .transpose()?;
    Ok(KotlinConstructorInfo {
        parameters,
        is_secondary: raw.flags & (1 << 4) != 0,
        jvm_signature,
    })
}

fn resolve_method_signature(
    raw: &RawJvmMethodSignature,
    fallback_name: &str,
    resolver: &NameResolver,
) -> Result<KotlinJvmMethodSignature, KotlinMetadataError> {
    Ok(KotlinJvmMethodSignature {
        name: raw
            .name
            .map(|index| resolver.get(index))
            .transpose()?
            .unwrap_or_else(|| fallback_name.to_string()),
        descriptor: raw
            .descriptor
            .map(|index| resolver.get(index))
            .transpose()?,
    })
}

fn resolve_field_signature(
    raw: &RawJvmFieldSignature,
    fallback_name: &str,
    resolver: &NameResolver,
) -> Result<KotlinJvmFieldSignature, KotlinMetadataError> {
    Ok(KotlinJvmFieldSignature {
        name: raw
            .name
            .map(|index| resolver.get(index))
            .transpose()?
            .unwrap_or_else(|| fallback_name.to_string()),
        descriptor: raw
            .descriptor
            .map(|index| resolver.get(index))
            .transpose()?,
    })
}

fn resolve_property_signature(
    raw: &RawJvmPropertySignature,
    property_name: &str,
    resolver: &NameResolver,
) -> Result<KotlinJvmPropertySignature, KotlinMetadataError> {
    Ok(KotlinJvmPropertySignature {
        field: raw
            .field
            .as_ref()
            .map(|signature| resolve_field_signature(signature, property_name, resolver))
            .transpose()?,
        synthetic_method: raw
            .synthetic_method
            .as_ref()
            .map(|signature| resolve_method_signature(signature, property_name, resolver))
            .transpose()?,
        getter: raw
            .getter
            .as_ref()
            .map(|signature| resolve_method_signature(signature, property_name, resolver))
            .transpose()?,
        setter: raw
            .setter
            .as_ref()
            .map(|signature| resolve_method_signature(signature, property_name, resolver))
            .transpose()?,
        delegate_method: raw
            .delegate_method
            .as_ref()
            .map(|signature| resolve_method_signature(signature, property_name, resolver))
            .transpose()?,
    })
}

fn decode_data1(data: &[String]) -> Result<Vec<u8>, KotlinMetadataError> {
    let Some(first) = data.first() else {
        return Ok(Vec::new());
    };
    let marker = first.chars().next();
    if marker == Some('\0') {
        return Ok(strings_to_bytes(data, true));
    }

    let drop_marker = marker == Some('\u{ffff}');
    let mut bytes = strings_to_bytes(data, drop_marker);
    for byte in &mut bytes {
        *byte = byte.wrapping_add(0x7f) & 0x7f;
    }
    decode_7_to_8(&bytes)
}

fn strings_to_bytes(data: &[String], drop_marker: bool) -> Vec<u8> {
    let mut result = Vec::new();
    for (string_index, string) in data.iter().enumerate() {
        for (character_index, character) in string.chars().enumerate() {
            if drop_marker && string_index == 0 && character_index == 0 {
                continue;
            }
            result.push(character as u32 as u8);
        }
    }
    result
}

fn decode_7_to_8(data: &[u8]) -> Result<Vec<u8>, KotlinMetadataError> {
    let result_len = 7 * data.len() / 8;
    let mut result = Vec::with_capacity(result_len);
    let mut byte_index = 0;
    let mut bit = 0;

    for _ in 0..result_len {
        let first = *data.get(byte_index).ok_or(KotlinMetadataError::Truncated)? as usize >> bit;
        byte_index += 1;
        let second = (*data.get(byte_index).ok_or(KotlinMetadataError::Truncated)? as usize
            & ((1 << (bit + 1)) - 1))
            << (7 - bit);
        result.push((first + second) as u8);
        if bit == 6 {
            byte_index += 1;
            bit = 0;
        } else {
            bit += 1;
        }
    }
    Ok(result)
}

fn read_packed_varints(bytes: &[u8]) -> Result<Vec<u32>, KotlinMetadataError> {
    let mut reader = ProtoReader::new(bytes);
    let mut values = Vec::new();
    while !reader.is_empty() {
        values.push(reader.read_varint()? as u32);
    }
    Ok(values)
}

struct ProtoReader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ProtoReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn is_empty(&self) -> bool {
        self.position == self.bytes.len()
    }

    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.position..]
    }

    fn next_key(&mut self) -> Result<Option<(u32, u8)>, KotlinMetadataError> {
        if self.is_empty() {
            return Ok(None);
        }
        let key = self.read_varint()?;
        let field = (key >> 3) as u32;
        let wire = (key & 0x7) as u8;
        if field == 0 {
            return Err(KotlinMetadataError::InvalidWireType(wire));
        }
        Ok(Some((field, wire)))
    }

    fn read_varint(&mut self) -> Result<u64, KotlinMetadataError> {
        let mut result = 0_u64;
        for shift in (0..70).step_by(7) {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or(KotlinMetadataError::Truncated)?;
            self.position += 1;
            if shift == 63 && byte > 1 {
                return Err(KotlinMetadataError::Truncated);
            }
            result |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(result);
            }
        }
        Err(KotlinMetadataError::Truncated)
    }

    fn read_len(&mut self) -> Result<&'a [u8], KotlinMetadataError> {
        let length = self.read_varint()? as usize;
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(KotlinMetadataError::Truncated)?;
        let result = &self.bytes[self.position..end];
        self.position = end;
        Ok(result)
    }

    fn skip(&mut self, wire: u8) -> Result<(), KotlinMetadataError> {
        match wire {
            0 => {
                self.read_varint()?;
            }
            1 => self.advance(8)?,
            2 => {
                self.read_len()?;
            }
            5 => self.advance(4)?,
            wire => return Err(KotlinMetadataError::InvalidWireType(wire)),
        }
        Ok(())
    }

    fn advance(&mut self, amount: usize) -> Result<(), KotlinMetadataError> {
        self.position = self
            .position
            .checked_add(amount)
            .filter(|position| *position <= self.bytes.len())
            .ok_or(KotlinMetadataError::Truncated)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_legacy_7_bit_encoding() {
        // Encoded form of [0x08, 0x96, 0x01], generated with the algorithm in
        // JetBrains' BitEncoding.java (without an explicit 0xffff marker).
        let data = vec![
            char::from_u32(9).unwrap(),
            char::from_u32(45).unwrap(),
            char::from_u32(7).unwrap(),
            char::from_u32(1).unwrap(),
        ]
        .into_iter()
        .collect::<String>();
        assert_eq!(decode_data1(&[data]).unwrap(), vec![0x08, 0x96, 0x01]);
    }

    #[test]
    fn rejects_truncated_length_delimited_fields() {
        let mut reader = ProtoReader::new(&[0x0a, 0x04, 0x01]);
        assert_eq!(reader.next_key().unwrap(), Some((1, 2)));
        assert_eq!(reader.read_len(), Err(KotlinMetadataError::Truncated));
    }
}
