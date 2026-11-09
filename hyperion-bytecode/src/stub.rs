// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Readable Java stubs rendered from class files.
//!
//! Go-to-definition into a library without a `-sources` archive (and into a JDK
//! without `src.zip`) needs *something* to open. A stub lists a type's
//! declaration, fields (with constant values) and method signatures with the
//! generic information the class file keeps, and no bodies or documentation.
//! Nested member types are rendered inside their outer type, so one stub covers
//! everything that lives in the top-level class file's family.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cafebabe::attributes::{AttributeData, InnerClassAccessFlags};
use cafebabe::constant_pool::LiteralConstant;
use cafebabe::{ClassAccessFlags, ClassFile, FieldAccessFlags};

use crate::extractor::{
    extract_generic_supertypes, extract_symbols, parse_jvm_type_to_string, MethodSymbol,
};

/// One class file of a top-level type's family (the type itself or one of its
/// nested member types).
struct Member<'a> {
    class: ClassFile<'a>,
    /// Simple name and enclosing binary name for member types.
    inner: Option<(String, String, InnerClassAccessFlags)>,
}

/// Render `top_level` and the member types found in `nested` (the class files
/// named `Outer$Member...`) as a Java source stub. `None` when the top-level
/// class file cannot be parsed.
#[must_use]
pub fn render_class_stub(top_level: &[u8], nested: &[Vec<u8>]) -> Option<String> {
    let top = crate::parser::parse_class_file(top_level).ok()?;
    let top_name = top.this_class.to_string();
    let mut members: BTreeMap<String, Member<'_>> = BTreeMap::new();
    members.insert(
        top_name.clone(),
        Member {
            class: top,
            inner: None,
        },
    );
    for bytes in nested {
        let Ok(class) = crate::parser::parse_class_file(bytes) else {
            continue;
        };
        let name = class.this_class.to_string();
        let Some(inner) = inner_class_entry(&class, &name) else {
            // Anonymous and local classes have no member entry.
            continue;
        };
        members.insert(
            name,
            Member {
                class,
                inner: Some(inner),
            },
        );
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "// Generated from the compiled class {top_name}.class: signatures only.\n// No source is available for this type; bodies are placeholders and documentation is omitted."
    );
    let package = top_name.rsplit_once('/').map(|(package, _)| package);
    if let Some(package) = package {
        let _ = writeln!(out, "package {};\n", package.replace('/', "."));
    }
    render_type(&mut out, &members, &top_name, 0);
    Some(out)
}

fn inner_class_entry(
    class: &ClassFile<'_>,
    name: &str,
) -> Option<(String, String, InnerClassAccessFlags)> {
    class.attributes.iter().find_map(|attribute| {
        let AttributeData::InnerClasses(entries) = &attribute.data else {
            return None;
        };
        entries.iter().find_map(|entry| {
            if entry.inner_class_info.as_ref() != name {
                return None;
            }
            let simple = entry.inner_name.as_ref()?.to_string();
            let outer = entry.outer_class_info.as_ref()?.to_string();
            Some((simple, outer, entry.access_flags))
        })
    })
}

fn render_type(out: &mut String, members: &BTreeMap<String, Member<'_>>, name: &str, depth: usize) {
    let Some(member) = members.get(name) else {
        return;
    };
    let class = &member.class;
    let Ok(symbols) = extract_symbols(class) else {
        return;
    };
    let indent = "    ".repeat(depth);
    let simple = member.inner.as_ref().map_or_else(
        || name.rsplit('/').next().unwrap_or(name).to_owned(),
        |(simple, _, _)| simple.clone(),
    );
    let flags = member.inner.as_ref().map(|(_, _, flags)| *flags);
    let is_interface = class.access_flags.contains(ClassAccessFlags::INTERFACE);
    let is_annotation = class.access_flags.contains(ClassAccessFlags::ANNOTATION);
    let is_enum = class.access_flags.contains(ClassAccessFlags::ENUM);

    let mut header = String::new();
    let public = flags.map_or_else(
        || class.access_flags.contains(ClassAccessFlags::PUBLIC),
        |flags| flags.contains(InnerClassAccessFlags::PUBLIC),
    );
    let protected = flags.is_some_and(|flags| flags.contains(InnerClassAccessFlags::PROTECTED));
    let private = flags.is_some_and(|flags| flags.contains(InnerClassAccessFlags::PRIVATE));
    if public {
        header.push_str("public ");
    } else if protected {
        header.push_str("protected ");
    } else if private {
        header.push_str("private ");
    }
    if flags.is_some_and(|flags| flags.contains(InnerClassAccessFlags::STATIC))
        && !is_interface
        && !is_enum
    {
        header.push_str("static ");
    }
    if class.access_flags.contains(ClassAccessFlags::ABSTRACT) && !is_interface {
        header.push_str("abstract ");
    }
    if class.access_flags.contains(ClassAccessFlags::FINAL) && !is_enum {
        header.push_str("final ");
    }
    let kind = if is_annotation {
        "@interface"
    } else if is_interface {
        "interface"
    } else if is_enum {
        "enum"
    } else {
        "class"
    };
    let _ = write!(header, "{kind} {simple}");
    if !symbols.type_parameters.is_empty() {
        let _ = write!(header, "<{}>", symbols.type_parameters.join(", "));
    }
    // The erased supertype table loses type arguments; the Signature
    // attribute keeps them.
    let generic_supertypes = extract_generic_supertypes(class);
    let with_arguments = |name: &str| -> String {
        generic_supertypes
            .iter()
            .find(|supertype| supertype.target == name)
            .map_or_else(
                || source_name(name),
                |supertype| {
                    format!(
                        "{}<{}>",
                        source_name(name),
                        supertype
                            .arguments
                            .iter()
                            .map(|argument| source_name(argument))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                },
            )
    };
    if let Some(super_class) = &symbols.super_class {
        let skip =
            matches!(super_class.as_str(), "java.lang.Object" | "java.lang.Enum") || is_interface;
        if !skip {
            let _ = write!(header, " extends {}", with_arguments(super_class));
        }
    }
    let interfaces: Vec<String> = symbols
        .interfaces
        .iter()
        .filter(|interface| {
            !(is_annotation && interface.as_str() == "java.lang.annotation.Annotation")
        })
        .map(|interface| with_arguments(interface))
        .collect();
    if !interfaces.is_empty() {
        let keyword = if is_interface {
            "extends"
        } else {
            "implements"
        };
        let _ = write!(header, " {keyword} {}", interfaces.join(", "));
    }
    let _ = writeln!(out, "{indent}{header} {{");

    let body = "    ".repeat(depth + 1);
    // Enum constants first, as the language requires.
    let constants: Vec<&str> = class
        .fields
        .iter()
        .filter(|field| field.access_flags.contains(FieldAccessFlags::ENUM))
        .map(|field| field.name.as_ref())
        .collect();
    if is_enum {
        let _ = writeln!(out, "{body}{};", constants.join(", "));
    }

    for (field, symbol) in class.fields.iter().zip(&symbols.fields) {
        if field.access_flags.contains(FieldAccessFlags::SYNTHETIC)
            || field.access_flags.contains(FieldAccessFlags::ENUM)
            || symbol.is_private
        {
            continue;
        }
        let mut line = String::new();
        push_modifiers(
            &mut line,
            symbol.is_public,
            symbol.is_protected,
            symbol.is_static,
            symbol.is_final,
            is_interface,
        );
        let ty = field_type_text(field, &symbol.field_type.to_string());
        let _ = write!(line, "{ty} {}", field.name);
        if symbol.is_final && symbol.is_static {
            if let Some(value) = constant_value_text(field) {
                let _ = write!(line, " = {value}");
            }
        }
        let _ = writeln!(out, "{body}{line};");
    }

    for (method, symbol) in class.methods.iter().zip(&symbols.methods) {
        if symbol.is_synthetic
            || symbol.is_bridge
            || symbol.is_private
            || method.name.starts_with('<') && method.name.as_ref() != "<init>"
            || (is_enum && is_implicit_enum_method(symbol))
        {
            continue;
        }
        let _ = writeln!(out, "{body}{}", method_text(symbol, &simple, is_interface));
    }

    // Member types, in a stable order.
    for (child_name, child) in members {
        if child
            .inner
            .as_ref()
            .is_some_and(|(_, outer, _)| outer == name)
        {
            render_type(out, members, child_name, depth + 1);
        }
    }
    let _ = writeln!(out, "{indent}}}");
}

fn is_implicit_enum_method(method: &MethodSymbol) -> bool {
    (method.name == "values" && method.parameters.is_empty())
        || (method.name == "valueOf" && method.parameters.len() == 1 && method.is_static)
}

fn push_modifiers(
    line: &mut String,
    public: bool,
    protected: bool,
    is_static: bool,
    is_final: bool,
    in_interface: bool,
) {
    if public && !in_interface {
        line.push_str("public ");
    } else if protected {
        line.push_str("protected ");
    }
    if is_static && !in_interface {
        line.push_str("static ");
    }
    if is_final && !in_interface {
        line.push_str("final ");
    }
}

fn method_text(method: &MethodSymbol, class_simple_name: &str, in_interface: bool) -> String {
    let mut line = String::new();
    push_modifiers(
        &mut line,
        method.is_public,
        method.is_protected,
        method.is_static,
        false,
        in_interface,
    );
    if method.is_final && !in_interface {
        line.push_str("final ");
    }
    if method.is_abstract && !in_interface {
        line.push_str("abstract ");
    }
    if method.is_native {
        line.push_str("native ");
    }
    if in_interface && !method.is_abstract && !method.is_static {
        line.push_str("default ");
    } else if in_interface && method.is_static {
        line.push_str("static ");
    }
    if !method.type_parameters.is_empty() {
        let _ = write!(line, "<{}> ", method.type_parameters.join(", "));
    }
    let constructor = method.name == "<init>";
    if constructor {
        line.push_str(class_simple_name);
    } else {
        let return_type = method
            .generic_return_type
            .clone()
            .unwrap_or_else(|| method.return_type.to_string());
        let _ = write!(line, "{} {}", source_name(&return_type), method.name);
    }
    line.push('(');
    let parameter_count = method.parameters.len();
    // Constructors of inner classes and enums carry synthetic leading
    // parameters in the descriptor that the generic signature does not.
    let use_generic = method.generic_parameter_types.len() == parameter_count;
    for (index, parameter) in method.parameters.iter().enumerate() {
        if index > 0 {
            line.push_str(", ");
        }
        let mut ty = if use_generic {
            method.generic_parameter_types[index].clone()
        } else {
            parameter.to_string()
        };
        if method.is_varargs && index + 1 == parameter_count {
            if let Some(element) = ty.strip_suffix("[]") {
                ty = format!("{element}...");
            }
        }
        let name = method
            .parameter_names
            .get(index)
            .and_then(Clone::clone)
            .filter(|name| !name.is_empty() && !name.starts_with('<'))
            .unwrap_or_else(|| format!("arg{index}"));
        let _ = write!(line, "{} {name}", source_name(&ty));
    }
    line.push(')');
    if !method.exceptions.is_empty() {
        let thrown: Vec<String> = method.exceptions.iter().map(|ty| source_name(ty)).collect();
        let _ = write!(line, " throws {}", thrown.join(", "));
    }
    if method.is_abstract || method.is_native {
        line.push(';');
    } else if constructor || matches!(method.return_type, crate::descriptor::DescriptorType::Void) {
        line.push_str(" { }");
    } else {
        // A valid body for any return type: the stub must not show a
        // `missing return statement` on every method.
        line.push_str(" { throw null; }");
    }
    line
}

/// Binary class names use `$` for nesting; source spells that with dots.
fn source_name(name: &str) -> String {
    name.replace('$', ".")
}

fn field_type_text(field: &cafebabe::FieldInfo<'_>, erased: &str) -> String {
    field
        .attributes
        .iter()
        .find_map(|attribute| {
            if let AttributeData::Signature(signature) = &attribute.data {
                parse_jvm_type_to_string(signature)
            } else {
                None
            }
        })
        .map_or_else(|| source_name(erased), |generic| source_name(&generic))
}

fn constant_value_text(field: &cafebabe::FieldInfo<'_>) -> Option<String> {
    field.attributes.iter().find_map(|attribute| {
        let AttributeData::ConstantValue(value) = &attribute.data else {
            return None;
        };
        Some(match value {
            LiteralConstant::Integer(value) => {
                use cafebabe::descriptors::FieldType;
                match &field.descriptor.field_type {
                    FieldType::Boolean => (*value != 0).to_string(),
                    FieldType::Char => char::from_u32(*value as u32)
                        .map_or_else(|| value.to_string(), |c| format!("{c:?}")),
                    _ => value.to_string(),
                }
            }
            LiteralConstant::Long(value) => format!("{value}L"),
            LiteralConstant::Float(value) => format!("{value:?}f"),
            LiteralConstant::Double(value) => format!("{value:?}"),
            LiteralConstant::String(value) => format!("{:?}", value.as_ref()),
            _ => return None,
        })
    })
}
