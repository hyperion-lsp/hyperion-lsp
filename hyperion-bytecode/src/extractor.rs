// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol extraction from parsed class files
//!
//! This module extracts structured symbol information (methods, fields, classes)
//! from parsed Java class files.

use crate::descriptor::DescriptorType;
use crate::parser::{
    binary_reference_to_source_name, extract_package, get_class_name, get_interfaces,
    get_super_class,
};
use cafebabe::{ClassFile, FieldInfo, MethodInfo};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("Failed to parse method descriptor: {0}")]
    MethodDescriptor(#[from] crate::descriptor::DescriptorError),

    #[error("Failed to parse field descriptor: {0}")]
    FieldDescriptor(String),

    #[error("Failed to extract class name: {0}")]
    ClassName(#[from] crate::parser::ParseError),
}

/// Complete symbol information extracted from a class file
#[derive(Debug, Clone)]
pub struct ClassSymbols {
    /// Fully qualified class name (e.g., "java.lang.String")
    pub fqn: String,

    /// Package name (e.g., "java.lang")
    pub package: Option<String>,

    /// Super class FQN
    pub super_class: Option<String>,

    /// Implemented interfaces
    pub interfaces: Vec<String>,

    /// Methods defined in this class
    pub methods: Vec<MethodSymbol>,

    /// Fields defined in this class
    pub fields: Vec<FieldSymbol>,

    /// Type parameters for generic classes (e.g., ["E"] for List<E>, ["K", "V"] for Map<K,V>)
    pub type_parameters: Vec<String>,

    /// Class modifiers
    pub is_interface: bool,
    pub is_abstract: bool,
    pub is_final: bool,
    pub is_public: bool,
    /// Marked `@Deprecated` (the class file carries a `Deprecated` attribute).
    pub is_deprecated: bool,
}

/// An explicitly parameterized nominal edge from a class Signature attribute.
///
/// This is deliberately separate from [`ClassSymbols`]: only generic classes
/// pay the allocation cost, while the erased superclass/interface identities
/// remain available through the ordinary fields above.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GenericSupertype {
    pub target: String,
    pub arguments: Vec<String>,
}

/// Method symbol information
#[derive(Debug, Clone)]
pub struct MethodSymbol {
    /// Method name (e.g., "substring", "<init>" for constructors)
    pub name: String,

    /// Parameter types
    pub parameters: Vec<DescriptorType>,

    /// Parameter names (from LocalVariableTable if available)
    /// Parallel to `parameters` - same length, None if name not available
    pub parameter_names: Vec<Option<String>>,

    /// Return type (Void for constructors)
    pub return_type: DescriptorType,

    /// Generic return type from Signature attribute (e.g., "V" for Map.get())
    /// None if method has no Signature attribute or return type matches descriptor
    pub generic_return_type: Option<String>,

    /// Generic parameter types from the Signature attribute.
    /// Empty when the method has no generic signature.
    pub generic_parameter_types: Vec<String>,

    /// Type parameters declared by the method (for example `T` in `<T> T id(T)`).
    pub type_parameters: Vec<String>,

    /// Exception classes named in the `Exceptions` attribute (the `throws`
    /// clause), as dotted binary names (`java.io.IOException`).
    pub exceptions: Vec<String>,

    /// Access modifiers
    pub is_public: bool,
    pub is_private: bool,
    pub is_protected: bool,
    pub is_static: bool,
    pub is_final: bool,
    pub is_abstract: bool,
    pub is_native: bool,
    /// Compiler-generated bridge used to preserve overriding after erasure.
    pub is_bridge: bool,
    /// Declaration not present in the source-language API.
    pub is_synthetic: bool,
    /// The final array parameter is declared with source-level varargs syntax.
    pub is_varargs: bool,
    /// Marked `@Deprecated`.
    pub is_deprecated: bool,
}

impl MethodSymbol {
    /// Check if this is a constructor
    pub fn is_constructor(&self) -> bool {
        self.name == "<init>"
    }

    /// Get a signature string for debugging
    pub fn signature(&self) -> String {
        let params: Vec<String> = self.parameters.iter().map(|p| p.to_string()).collect();
        format!("{} {}({})", self.return_type, self.name, params.join(", "))
    }
}

/// Field symbol information
#[derive(Debug, Clone)]
pub struct FieldSymbol {
    /// Field name
    pub name: String,

    /// Field type
    pub field_type: DescriptorType,

    /// Access modifiers
    pub is_public: bool,
    pub is_private: bool,
    pub is_protected: bool,
    pub is_static: bool,
    pub is_final: bool,
    /// Marked `@Deprecated`.
    pub is_deprecated: bool,
}

/// Whether an attribute list carries the class-file `Deprecated` attribute
/// (emitted by javac for `@Deprecated` and the `@deprecated` javadoc tag).
fn has_deprecated_attribute(attributes: &[cafebabe::attributes::AttributeInfo<'_>]) -> bool {
    attributes.iter().any(|attribute| {
        matches!(
            attribute.data,
            cafebabe::attributes::AttributeData::Deprecated
        )
    })
}

/// Extract all symbol information from a class file
pub fn extract_symbols(class: &ClassFile) -> Result<ClassSymbols, ExtractionError> {
    let fqn = get_class_name(class)?;
    let package = extract_package(&fqn);

    // Extract type parameters from Signature attribute (for generics)
    let type_parameters = extract_class_type_parameters(class);

    // Extract methods, keeping generic_return_type for ALL methods that have
    // a Signature attribute.  Method-level type variables (like <T> on
    // Optional.of) are needed by the semantic layer for type parameter
    // inference from call-site arguments.
    let methods = class
        .methods
        .iter()
        .map(extract_method)
        .collect::<Result<Vec<_>, _>>()?;

    // Extract fields
    let fields = class
        .fields
        .iter()
        .map(extract_field)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ClassSymbols {
        fqn,
        package,
        super_class: get_super_class(class),
        interfaces: get_interfaces(class),
        methods,
        fields,
        type_parameters,
        is_interface: class
            .access_flags
            .contains(cafebabe::ClassAccessFlags::INTERFACE),
        is_abstract: class
            .access_flags
            .contains(cafebabe::ClassAccessFlags::ABSTRACT),
        is_final: class
            .access_flags
            .contains(cafebabe::ClassAccessFlags::FINAL),
        is_public: class
            .access_flags
            .contains(cafebabe::ClassAccessFlags::PUBLIC),
        is_deprecated: has_deprecated_attribute(&class.attributes),
    })
}

/// Extract type parameters from a class's Signature attribute
///
/// JVM generic signatures start with type parameters in angle brackets:
/// - `<E:Ljava/lang/Object;>` for List<E>
/// - `<K:Ljava/lang/Object;V:Ljava/lang/Object;>` for Map<K,V>
///
/// This function parses these signatures and extracts just the parameter names.
fn extract_class_type_parameters(class: &ClassFile) -> Vec<String> {
    use cafebabe::attributes::AttributeData;

    // Find the Signature attribute
    let signature = class.attributes.iter().find_map(|attr| {
        if let AttributeData::Signature(sig) = &attr.data {
            Some(sig.as_ref())
        } else {
            None
        }
    });

    match signature {
        Some(sig) => parse_type_parameters_from_signature(sig),
        None => Vec::new(),
    }
}

/// Extract explicit superclass and interface type arguments from the class
/// Signature attribute.
///
/// The ordinary class-file superclass and interface table is erased. A class
/// such as `Child extends Base<String>` therefore needs this parallel metadata
/// if callers are to project `Base<T>` members into `Child` correctly.
pub fn extract_generic_supertypes(class: &ClassFile) -> Vec<GenericSupertype> {
    use cafebabe::attributes::AttributeData;

    let Some(signature) = class.attributes.iter().find_map(|attribute| {
        if let AttributeData::Signature(signature) = &attribute.data {
            Some(signature.as_ref())
        } else {
            None
        }
    }) else {
        return Vec::new();
    };

    parse_generic_supertypes_signature(signature)
}

/// Whether this class is a static member according to its InnerClasses entry.
pub fn is_static_member_class(class: &ClassFile) -> bool {
    use cafebabe::attributes::{AttributeData, InnerClassAccessFlags};

    let this_class: &str = &class.this_class;
    class.attributes.iter().any(|attribute| {
        let AttributeData::InnerClasses(entries) = &attribute.data else {
            return false;
        };
        entries.iter().any(|entry| {
            entry.inner_class_info.as_ref() == this_class
                && entry.access_flags.contains(InnerClassAccessFlags::STATIC)
        })
    })
}

fn parse_generic_supertypes_signature(signature: &str) -> Vec<GenericSupertype> {
    let mut offset = formal_type_parameter_end(signature).unwrap_or(0);
    let mut supertypes = Vec::new();

    while offset < signature.len() {
        let Some((rendered, consumed)) = parse_jvm_type_with_len(&signature[offset..]) else {
            // A malformed Signature attribute must not produce partial,
            // potentially misleading hierarchy metadata.
            return Vec::new();
        };
        if consumed == 0 {
            return Vec::new();
        }
        if let Some(supertype) = split_parameterized_nominal_type(&rendered) {
            supertypes.push(supertype);
        }
        offset += consumed;
    }

    supertypes
}

fn formal_type_parameter_end(signature: &str) -> Option<usize> {
    if !signature.starts_with('<') {
        return Some(0);
    }

    let mut depth = 0usize;
    for (offset, byte) in signature.bytes().enumerate() {
        match byte {
            b'<' => depth += 1,
            b'>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_parameterized_nominal_type(rendered: &str) -> Option<GenericSupertype> {
    let mut target = String::with_capacity(rendered.len());
    let mut depth = 0usize;
    let mut argument_start = None;
    let mut arguments = Vec::new();

    for (offset, ch) in rendered.char_indices() {
        match ch {
            '<' => {
                if depth == 0 {
                    argument_start = Some(offset + ch.len_utf8());
                }
                depth += 1;
            }
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    arguments.extend(split_top_level_type_arguments(
                        &rendered[argument_start.take()?..offset],
                    )?);
                }
            }
            _ if depth == 0 => target.push(ch),
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }

    if target.is_empty() || arguments.is_empty() {
        return None;
    }
    Some(GenericSupertype { target, arguments })
}

fn split_top_level_type_arguments(arguments: &str) -> Option<Vec<String>> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (offset, ch) in arguments.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                let argument = arguments[start..offset].trim();
                if argument.is_empty() {
                    return None;
                }
                result.push(argument.to_string());
                start = offset + ch.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let argument = arguments[start..].trim();
    if argument.is_empty() {
        return None;
    }
    result.push(argument.to_string());
    Some(result)
}

/// Parse type parameter names from a JVM generic signature
///
/// Signature format: `<TypeParam1:Bound1;TypeParam2:Bound2;>...`
/// Examples:
/// - `<E:Ljava/lang/Object;>` → ["E"]
/// - `<K:Ljava/lang/Object;V:Ljava/lang/Object;>` → ["K", "V"]
/// - `<T::Ljava/lang/Comparable<-TT;>;>` → ["T"] (with interface bound)
fn parse_type_parameters_from_signature(signature: &str) -> Vec<String> {
    // Must start with '<' for type parameters
    if !signature.starts_with('<') {
        return Vec::new();
    }

    let mut params = Vec::new();
    let mut chars = signature[1..].chars().peekable();
    let mut current_name = String::new();
    let mut depth = 1; // We're inside the initial '<'

    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                depth += 1;
            }
            '>' => {
                depth -= 1;
                if depth == 0 {
                    // End of type parameters section
                    break;
                }
            }
            ':' if depth == 1 => {
                // Colon separates param name from bound
                if !current_name.is_empty() {
                    params.push(current_name.clone());
                    current_name.clear();
                }
                // Skip the bound - read until next param or end
                // Bounds can be complex: `Ljava/lang/Object;` or `:Ljava/lang/Comparable<TT;>;`
                skip_type_bound(&mut chars, &mut depth);
            }
            ';' if depth == 1 => {
                // Semicolon ends a bound, next char starts new param (if any before '>')
            }
            _ if depth == 1 => {
                // Part of the parameter name
                current_name.push(ch);
            }
            _ => {
                // Inside a nested generic, skip
            }
        }
    }

    params
}

/// Skip over a type bound in a signature (helper for parse_type_parameters_from_signature)
fn skip_type_bound(chars: &mut std::iter::Peekable<std::str::Chars>, depth: &mut i32) {
    while let Some(&ch) = chars.peek() {
        match ch {
            '<' => {
                *depth += 1;
                chars.next();
            }
            '>' => {
                if *depth == 1 {
                    // Don't consume the closing '>' of the type params section
                    break;
                }
                *depth -= 1;
                chars.next();
            }
            ';' if *depth == 1 => {
                // End of this bound
                chars.next();
                break;
            }
            ':' if *depth == 1 => {
                // Another bound (interface bound after class bound)
                chars.next();
                // Continue reading the next bound
            }
            _ => {
                chars.next();
            }
        }
    }
}

/// Extract generic return type from a method's Signature attribute
///
/// JVM method signature format: `(<param_types>)<return_type>`
/// Examples:
///   (Ljava/lang/Object;)TV;           → "V"      (Map.get returns V)
///   (I)TE;                            → "E"      (List.get returns E)
///   ()Ljava/util/Iterator<TE;>;       → "java.util.Iterator<E>"
fn extract_method_generic_return_type(method: &MethodInfo) -> Option<String> {
    use cafebabe::attributes::AttributeData;

    let signature = method.attributes.iter().find_map(|attr| {
        if let AttributeData::Signature(sig) = &attr.data {
            Some(sig.as_ref())
        } else {
            None
        }
    })?;

    // Find the return type portion: everything after the last ')'
    let return_part = signature.rsplit_once(')')?.1;

    parse_jvm_type_to_string(return_part)
}

/// Extract declared variables and parameter types from a JVM method signature.
fn extract_method_generic_parameters(method: &MethodInfo) -> (Vec<String>, Vec<String>) {
    use cafebabe::attributes::AttributeData;

    let Some(signature) = method.attributes.iter().find_map(|attribute| {
        if let AttributeData::Signature(signature) = &attribute.data {
            Some(signature.as_ref())
        } else {
            None
        }
    }) else {
        return (Vec::new(), Vec::new());
    };
    parse_method_generic_parameters_signature(signature)
}

fn parse_method_generic_parameters_signature(signature: &str) -> (Vec<String>, Vec<String>) {
    let type_parameters = parse_type_parameters_from_signature(signature);
    let Some((_, parameters_and_return)) = signature.split_once('(') else {
        return (type_parameters, Vec::new());
    };
    let Some((parameters, _)) = parameters_and_return.split_once(')') else {
        return (type_parameters, Vec::new());
    };

    let mut parameter_types = Vec::new();
    let mut offset = 0;
    while offset < parameters.len() {
        let Some((parameter, consumed)) = parse_jvm_type_with_len(&parameters[offset..]) else {
            return (type_parameters, Vec::new());
        };
        parameter_types.push(parameter);
        offset += consumed;
    }
    (type_parameters, parameter_types)
}

/// Collect type variable references from a JVM signature return type.
///
/// Scans for `T<name>;` patterns in the raw JVM signature.
#[cfg(test)]
fn collect_type_variables_from_jvm_type(input: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'T' {
            let is_type_var_start =
                i == 0 || matches!(bytes[i - 1], b'(' | b')' | b'<' | b';' | b'[' | b'+' | b'-');
            if is_type_var_start {
                if let Some(semi) = input[i + 1..].find(';') {
                    let name = &input[i + 1..i + 1 + semi];
                    vars.push(name.to_string());
                    i += 2 + semi;
                    continue;
                }
            }
        }
        i += 1;
    }
    vars
}

/// Parse a JVM type signature into a readable type string, returning the parsed
/// string and how many bytes were consumed.
///
/// Returns None if parsing fails (falls back to erased descriptor type).
fn parse_jvm_type_with_len(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    match bytes[0] {
        // Type variable: TV; → "V"
        b'T' => {
            let semi = input[1..].find(';')?;
            let name = &input[1..1 + semi];
            Some((name.to_string(), 2 + semi))
        }
        // Object type: Ljava/lang/String; or Ljava/util/List<TE;>;
        b'L' => {
            let mut rendered = String::new();
            let mut pos = 1usize;
            while pos < input.len() {
                match input.as_bytes()[pos] {
                    b';' => return Some((rendered, pos + 1)),
                    // A dot in a Signature attribute is an explicit member-type
                    // separator. A dollar sign is merely part of a JVM binary
                    // identifier unless the shared conservative conversion can
                    // recognize named member components.
                    b'.' => {
                        rendered.push('.');
                        pos += 1;
                    }
                    b'<' => {
                        pos += 1;
                        let mut type_args = Vec::new();
                        loop {
                            match *input.as_bytes().get(pos)? {
                                b'>' => {
                                    pos += 1;
                                    break;
                                }
                                variance @ (b'+' | b'-') => {
                                    pos += 1;
                                    let (arg, len) = parse_jvm_type_with_len(&input[pos..])?;
                                    let bound = if variance == b'+' { "extends" } else { "super" };
                                    type_args.push(format!("? {bound} {arg}"));
                                    pos += len;
                                }
                                b'*' => {
                                    type_args.push("?".to_string());
                                    pos += 1;
                                }
                                _ => {
                                    let (arg, len) = parse_jvm_type_with_len(&input[pos..])?;
                                    type_args.push(arg);
                                    pos += len;
                                }
                            }
                        }
                        rendered.push('<');
                        rendered.push_str(&type_args.join(","));
                        rendered.push('>');
                    }
                    _ => {
                        let end = input[pos..]
                            .find([';', '<', '.'])
                            .map(|end| pos + end)
                            .unwrap_or(input.len());
                        if end == pos {
                            return None;
                        }
                        let binary_name = input[pos..end].replace('/', ".");
                        rendered.push_str(&binary_reference_to_source_name(&binary_name));
                        pos = end;
                    }
                }
            }
            None
        }
        // Array: [<element_type>
        b'[' => {
            let (element, len) = parse_jvm_type_with_len(&input[1..])?;
            Some((format!("{}[]", element), 1 + len))
        }
        // Primitives
        b'B' => Some(("byte".to_string(), 1)),
        b'C' => Some(("char".to_string(), 1)),
        b'D' => Some(("double".to_string(), 1)),
        b'F' => Some(("float".to_string(), 1)),
        b'I' => Some(("int".to_string(), 1)),
        b'J' => Some(("long".to_string(), 1)),
        b'S' => Some(("short".to_string(), 1)),
        b'Z' => Some(("boolean".to_string(), 1)),
        b'V' => Some(("void".to_string(), 1)),
        _ => None,
    }
}

/// Parse a JVM type signature into a readable type string.
pub(crate) fn parse_jvm_type_to_string(input: &str) -> Option<String> {
    parse_jvm_type_with_len(input).map(|(s, _)| s)
}

/// Extract method symbol from MethodInfo
fn extract_method(method: &MethodInfo) -> Result<MethodSymbol, ExtractionError> {
    use cafebabe::MethodAccessFlags;

    // Convert cafebabe's parameter descriptors to our DescriptorType
    let parameters: Vec<DescriptorType> = method
        .descriptor
        .parameters
        .iter()
        .map(convert_field_descriptor)
        .collect();

    // Convert return type
    let return_type = convert_return_descriptor(&method.descriptor.return_type);

    // Extract generic return type from Signature attribute
    let generic_return_type = extract_method_generic_return_type(method);
    let (type_parameters, generic_parameter_types) = extract_method_generic_parameters(method);

    // Extract parameter names from LocalVariableTable if available
    let is_static = method.access_flags.contains(MethodAccessFlags::STATIC);
    let parameter_names = extract_parameter_names(method, &parameters, is_static);

    let exceptions = extract_method_exceptions(method);

    Ok(MethodSymbol {
        name: method.name.to_string(),
        parameters,
        parameter_names,
        exceptions,
        return_type,
        generic_return_type,
        generic_parameter_types,
        type_parameters,
        is_public: method.access_flags.contains(MethodAccessFlags::PUBLIC),
        is_private: method.access_flags.contains(MethodAccessFlags::PRIVATE),
        is_protected: method.access_flags.contains(MethodAccessFlags::PROTECTED),
        is_static,
        is_final: method.access_flags.contains(MethodAccessFlags::FINAL),
        is_abstract: method.access_flags.contains(MethodAccessFlags::ABSTRACT),
        is_native: method.access_flags.contains(MethodAccessFlags::NATIVE),
        is_bridge: method.access_flags.contains(MethodAccessFlags::BRIDGE),
        is_synthetic: method.access_flags.contains(MethodAccessFlags::SYNTHETIC),
        is_varargs: method.access_flags.contains(MethodAccessFlags::VARARGS),
        is_deprecated: has_deprecated_attribute(&method.attributes),
    })
}

/// Extract the `throws` clause from the `Exceptions` attribute.
///
/// The attribute records `throws E` (E a type variable) as the erasure of E's
/// bound, indistinguishable from a declared `throws Exception`. The generic
/// `Signature` attribute keeps the truth (`^TE;`), so a thrown type variable is
/// reported by its *name* instead (`E`), which no class can be called by a
/// qualified spelling.
fn extract_method_exceptions(method: &MethodInfo) -> Vec<String> {
    use cafebabe::attributes::AttributeData;

    let mut exceptions: Vec<String> = method
        .attributes
        .iter()
        .find_map(|attribute| match &attribute.data {
            AttributeData::Exceptions(names) => Some(
                names
                    .iter()
                    .map(|name| name.replace('/', "."))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    if exceptions.is_empty() {
        return exceptions;
    }
    let signature = method
        .attributes
        .iter()
        .find_map(|attribute| match &attribute.data {
            AttributeData::Signature(signature) => Some(signature.as_ref().to_owned()),
            _ => None,
        });
    let Some(signature) = signature else {
        return exceptions;
    };
    let Some((_, after_parameters)) = signature.rsplit_once(')') else {
        return exceptions;
    };
    // `^` introduces each thrown type; nothing before them can contain one.
    let thrown: Vec<&str> = after_parameters.split('^').skip(1).collect();
    if thrown.len() == exceptions.len() {
        for (slot, signature_entry) in exceptions.iter_mut().zip(thrown) {
            if let Some(variable) = signature_entry.strip_prefix('T') {
                if let Some((name, _)) = variable.split_once(';') {
                    *slot = name.to_owned();
                }
            }
        }
    }
    exceptions
}

/// Extract parameter names from the LocalVariableTable attribute
///
/// For instance methods, slot 0 is 'this', so parameters start at slot 1
/// For static methods, parameters start at slot 0
fn extract_parameter_names(
    method: &MethodInfo,
    parameters: &[DescriptorType],
    is_static: bool,
) -> Vec<Option<String>> {
    use cafebabe::attributes::AttributeData;

    // Find the Code attribute
    let code_data = method.attributes.iter().find_map(|attr| {
        if let AttributeData::Code(code) = &attr.data {
            Some(code)
        } else {
            None
        }
    });

    let code_data = match code_data {
        Some(code) => code,
        None => {
            // No Code attribute (abstract/native methods) - return all None
            return vec![None; parameters.len()];
        }
    };

    // Find the LocalVariableTable attribute within the Code attribute
    let local_var_table = code_data.attributes.iter().find_map(|attr| {
        if let AttributeData::LocalVariableTable(table) = &attr.data {
            Some(table)
        } else {
            None
        }
    });

    let local_var_table = match local_var_table {
        Some(table) => table,
        None => {
            // No LocalVariableTable - return all None
            return vec![None; parameters.len()];
        }
    };

    // Build a map from slot index to variable name
    // We only care about variables at start_pc = 0 (method parameters)
    let mut slot_to_name = std::collections::HashMap::new();
    for entry in local_var_table {
        if entry.start_pc == 0 {
            slot_to_name.insert(entry.index, entry.name.to_string());
        }
    }

    // Map parameter slots to names
    // Non-static: slot 0 is 'this', params start at slot 1
    // Static: params start at slot 0
    let first_param_slot = if is_static { 0 } else { 1 };

    let mut param_names = Vec::with_capacity(parameters.len());
    let mut current_slot = first_param_slot;

    for param_type in parameters {
        let name = slot_to_name.get(&current_slot).cloned();
        param_names.push(name);

        // Wide types (long, double) take 2 slots
        let slot_width = match param_type {
            DescriptorType::Long | DescriptorType::Double => 2,
            _ => 1,
        };
        current_slot += slot_width;
    }

    param_names
}

/// Extract field symbol from FieldInfo
fn extract_field(field: &FieldInfo) -> Result<FieldSymbol, ExtractionError> {
    use cafebabe::FieldAccessFlags;

    let field_type = convert_field_descriptor(&field.descriptor);

    Ok(FieldSymbol {
        name: field.name.to_string(),
        field_type,
        is_public: field.access_flags.contains(FieldAccessFlags::PUBLIC),
        is_private: field.access_flags.contains(FieldAccessFlags::PRIVATE),
        is_protected: field.access_flags.contains(FieldAccessFlags::PROTECTED),
        is_static: field.access_flags.contains(FieldAccessFlags::STATIC),
        is_final: field.access_flags.contains(FieldAccessFlags::FINAL),
        is_deprecated: has_deprecated_attribute(&field.attributes),
    })
}

/// Convert cafebabe's FieldDescriptor to our DescriptorType
fn convert_field_descriptor(fd: &cafebabe::descriptors::FieldDescriptor) -> DescriptorType {
    use cafebabe::descriptors::FieldType;

    let base_type = match &fd.field_type {
        FieldType::Byte => DescriptorType::Byte,
        FieldType::Char => DescriptorType::Char,
        FieldType::Double => DescriptorType::Double,
        FieldType::Float => DescriptorType::Float,
        FieldType::Integer => DescriptorType::Int,
        FieldType::Long => DescriptorType::Long,
        FieldType::Short => DescriptorType::Short,
        FieldType::Boolean => DescriptorType::Boolean,
        FieldType::Object(class_name) => {
            // ClassName uses internal format (java/lang/String)
            // Convert to FQN (java.lang.String)
            let name_str = class_name.to_string();
            DescriptorType::Class {
                name: name_str.replace('/', "."),
            }
        }
    };

    // Wrap in array if dimensions > 0
    if fd.dimensions > 0 {
        DescriptorType::Array {
            element_type: Box::new(base_type),
            dimensions: fd.dimensions as usize,
        }
    } else {
        base_type
    }
}

/// Convert cafebabe's ReturnDescriptor to our DescriptorType
fn convert_return_descriptor(rd: &cafebabe::descriptors::ReturnDescriptor) -> DescriptorType {
    use cafebabe::descriptors::ReturnDescriptor;

    match rd {
        ReturnDescriptor::Void => DescriptorType::Void,
        ReturnDescriptor::Return(field_desc) => convert_field_descriptor(field_desc),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_signature() {
        let method = MethodSymbol {
            name: "substring".to_string(),
            parameters: vec![DescriptorType::Int, DescriptorType::Int],
            parameter_names: vec![Some("start".to_string()), Some("end".to_string())],
            return_type: DescriptorType::Class {
                name: "java.lang.String".to_string(),
            },
            generic_return_type: None,
            generic_parameter_types: Vec::new(),
            type_parameters: Vec::new(),
            exceptions: Vec::new(),
            is_public: true,
            is_private: false,
            is_protected: false,
            is_static: false,
            is_final: false,
            is_abstract: false,
            is_native: false,
            is_bridge: false,
            is_synthetic: false,
            is_varargs: false,
            is_deprecated: false,
        };

        assert_eq!(method.signature(), "java.lang.String substring(int, int)");
    }

    #[test]
    fn test_is_constructor() {
        let constructor = MethodSymbol {
            name: "<init>".to_string(),
            parameters: vec![],
            parameter_names: vec![],
            return_type: DescriptorType::Void,
            generic_return_type: None,
            generic_parameter_types: Vec::new(),
            type_parameters: Vec::new(),
            exceptions: Vec::new(),
            is_public: true,
            is_private: false,
            is_protected: false,
            is_static: false,
            is_final: false,
            is_abstract: false,
            is_native: false,
            is_bridge: false,
            is_synthetic: false,
            is_varargs: false,
            is_deprecated: false,
        };

        assert!(constructor.is_constructor());

        let regular_method = MethodSymbol {
            name: "toString".to_string(),
            parameters: vec![],
            parameter_names: vec![],
            return_type: DescriptorType::Class {
                name: "java.lang.String".to_string(),
            },
            generic_return_type: None,
            generic_parameter_types: Vec::new(),
            type_parameters: Vec::new(),
            exceptions: Vec::new(),
            is_public: true,
            is_private: false,
            is_protected: false,
            is_static: false,
            is_final: false,
            is_abstract: false,
            is_native: false,
            is_bridge: false,
            is_synthetic: false,
            is_varargs: false,
            is_deprecated: false,
        };

        assert!(!regular_method.is_constructor());
    }

    #[test]
    fn test_parse_type_parameters_single() {
        // List<E> signature: <E:Ljava/lang/Object;>...
        let sig = "<E:Ljava/lang/Object;>Ljava/lang/Object;Ljava/util/Collection<TE;>;";
        assert_eq!(parse_type_parameters_from_signature(sig), vec!["E"]);
    }

    #[test]
    fn test_parse_type_parameters_two() {
        // Map<K, V> signature
        let sig = "<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;";
        assert_eq!(parse_type_parameters_from_signature(sig), vec!["K", "V"]);
    }

    #[test]
    fn test_parse_type_parameters_with_interface_bound() {
        // Comparable<T extends Comparable<T>> style
        let sig = "<T::Ljava/lang/Comparable<TT;>;>Ljava/lang/Object;";
        assert_eq!(parse_type_parameters_from_signature(sig), vec!["T"]);
    }

    #[test]
    fn test_parse_type_parameters_no_generics() {
        // Non-generic class (e.g., String)
        let sig = "Ljava/lang/Object;";
        assert_eq!(
            parse_type_parameters_from_signature(sig),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_parse_type_parameters_complex() {
        // Function<T, R> signature
        let sig = "<T:Ljava/lang/Object;R:Ljava/lang/Object;>Ljava/lang/Object;";
        assert_eq!(parse_type_parameters_from_signature(sig), vec!["T", "R"]);
    }

    #[test]
    fn parses_declared_method_variables_and_generic_parameters() {
        let signature = "<Result:Ljava/lang/Object;>(TResult;)Ljava/util/Optional<TResult;>;";

        let (type_parameters, parameter_types) =
            parse_method_generic_parameters_signature(signature);

        assert_eq!(type_parameters, vec!["Result"]);
        assert_eq!(parameter_types, vec!["Result"]);
    }

    #[test]
    fn parses_nested_generic_method_parameters() {
        let signature =
            "<Element:Ljava/lang/Object;>(Ljava/util/List<TElement;>;[TElement;)TElement;";

        let (type_parameters, parameter_types) =
            parse_method_generic_parameters_signature(signature);

        assert_eq!(type_parameters, vec!["Element"]);
        assert_eq!(
            parameter_types,
            vec!["java.util.List<Element>", "Element[]"]
        );
    }

    #[test]
    fn test_parse_jvm_type_variable() {
        assert_eq!(parse_jvm_type_to_string("TV;"), Some("V".into()));
        assert_eq!(parse_jvm_type_to_string("TE;"), Some("E".into()));
        assert_eq!(parse_jvm_type_to_string("TK;"), Some("K".into()));
    }

    #[test]
    fn test_parse_jvm_type_class() {
        assert_eq!(
            parse_jvm_type_to_string("Ljava/lang/String;"),
            Some("java.lang.String".into())
        );
        assert_eq!(
            parse_jvm_type_to_string("Ljava/lang/Object;"),
            Some("java.lang.Object".into())
        );
    }

    #[test]
    fn test_parse_jvm_type_generic_class() {
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/List<TE;>;"),
            Some("java.util.List<E>".into())
        );
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/Map<TK;TV;>;"),
            Some("java.util.Map<K,V>".into())
        );
    }

    #[test]
    fn test_parse_jvm_type_primitives() {
        assert_eq!(parse_jvm_type_to_string("I"), Some("int".into()));
        assert_eq!(parse_jvm_type_to_string("J"), Some("long".into()));
        assert_eq!(parse_jvm_type_to_string("Z"), Some("boolean".into()));
        assert_eq!(parse_jvm_type_to_string("V"), Some("void".into()));
    }

    #[test]
    fn test_parse_jvm_type_array() {
        assert_eq!(parse_jvm_type_to_string("[TE;"), Some("E[]".into()));
        assert_eq!(parse_jvm_type_to_string("[I"), Some("int[]".into()));
        assert_eq!(
            parse_jvm_type_to_string("[Ljava/lang/String;"),
            Some("java.lang.String[]".into())
        );
    }

    #[test]
    fn test_parse_jvm_type_wildcard() {
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/List<+Ljava/lang/Number;>;"),
            Some("java.util.List<? extends java.lang.Number>".into())
        );
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/List<-Ljava/lang/Number;>;"),
            Some("java.util.List<? super java.lang.Number>".into())
        );
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/List<*>;"),
            Some("java.util.List<?>".into())
        );
    }

    #[test]
    fn test_parse_jvm_type_inner_class() {
        // `$` alone is not proof of a member class. Preserve Scala module and
        // numeric local/anonymous binary identities.
        assert_eq!(
            parse_jvm_type_to_string("Lpkg/Foo$;"),
            Some("pkg.Foo$".into())
        );
        assert_eq!(
            parse_jvm_type_to_string("Lpkg/Foo$1;"),
            Some("pkg.Foo$1".into())
        );
        // Map$Entry → java.util.Map.Entry (preserve outer class context)
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/Map$Entry;"),
            Some("java.util.Map.Entry".into())
        );
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/Map$Entry<TK;TV;>;"),
            Some("java.util.Map.Entry<K,V>".into())
        );
        // Generic inner class: Set<Map.Entry<K,V>>
        assert_eq!(
            parse_jvm_type_to_string("Ljava/util/Set<Ljava/util/Map$Entry<TK;TV;>;>;"),
            Some("java.util.Set<java.util.Map.Entry<K,V>>".into())
        );
        assert_eq!(
            parse_jvm_type_to_string("Lexample/Outer<TT;>.Inner<TU;>;"),
            Some("example.Outer<T>.Inner<U>".into())
        );
    }

    #[test]
    fn parses_generic_class_supertype_edges() {
        let signature = "<T:Ljava/lang/Object;>Lexample/Base<Ljava/lang/String;>;Lexample/Pair<TT;Ljava/util/List<+Ljava/lang/Number;>;>;";

        assert_eq!(
            parse_generic_supertypes_signature(signature),
            vec![
                GenericSupertype {
                    target: "example.Base".to_string(),
                    arguments: vec!["java.lang.String".to_string()],
                },
                GenericSupertype {
                    target: "example.Pair".to_string(),
                    arguments: vec![
                        "T".to_string(),
                        "java.util.List<? extends java.lang.Number>".to_string(),
                    ],
                },
            ]
        );
    }

    #[test]
    fn generic_inner_supertype_uses_the_inner_declarations_arguments() {
        // Although Inner declares only its innermost formal parameter, types
        // written inside it can still reference Outer<T>. Retain both segment
        // argument lists so the workspace can compose both lexical scopes.
        let signature = "Ljava/lang/Object;Lexample/Outer<TOuter;>.Inner<TInner;>;";

        assert_eq!(
            parse_generic_supertypes_signature(signature),
            vec![GenericSupertype {
                target: "example.Outer.Inner".to_string(),
                arguments: vec!["Outer".to_string(), "Inner".to_string()],
            }]
        );
    }

    #[test]
    fn malformed_generic_class_signature_fails_closed() {
        assert!(
            parse_generic_supertypes_signature("<T:Ljava/lang/Object;Lexample/Base<TT;>;")
                .is_empty()
        );
    }

    #[test]
    fn test_collect_type_variables() {
        // Bare type variable
        assert_eq!(collect_type_variables_from_jvm_type("TV;"), vec!["V"]);
        // Type variable inside generic
        assert_eq!(
            collect_type_variables_from_jvm_type("Ljava/util/List<TE;>;"),
            vec!["E"]
        );
        // Multiple type variables
        assert_eq!(
            collect_type_variables_from_jvm_type("Ljava/util/Map<TK;TV;>;"),
            vec!["K", "V"]
        );
        // No type variables (concrete type)
        assert!(collect_type_variables_from_jvm_type("Ljava/lang/String;").is_empty());
        // Array of type variable
        assert_eq!(collect_type_variables_from_jvm_type("[TT;"), vec!["T"]);
    }

    #[test]
    fn test_collect_type_variables_ignores_class_names_with_t() {
        // "Thread" starts with T but isn't a type variable reference
        // The T in "Ljava/lang/Thread;" is preceded by '/' not a delimiter
        assert!(collect_type_variables_from_jvm_type("Ljava/lang/Thread;").is_empty());
    }
}
