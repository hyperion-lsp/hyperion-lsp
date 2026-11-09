// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Canonical type-string -> `Type` parser for Java types stored as strings in the
//! workspace index (e.g. `Symbol::return_type`, parameter strings, field types).
//!
//! Returns `Some(Type)` when parsing succeeds, `None` for malformed input — callers
//! decide the fallback. Pure: no `self`, no workspace state, no semantic
//! interpretation. Special behaviors callers may need are explicit:
//!
//! - **Stripping a trailing parameter name** (e.g. `"String name"` -> `"String"`):
//!   call [`strip_parameter_name`] first.
//! - **Mapping `void` to a sentinel like `Type::Unknown`** for `var` inference:
//!   match on `Type::Void` at the call site.
//! - **Treating bare names as type parameters** (e.g. `T`, `K`, `V`):
//!   pre-resolve before calling, or post-process the returned `ClassType`.

use crate::symbol::{ClassType, PrimitiveType, Type};
use tracing::{debug, warn};

/// Parse a type string into a `Type`.
///
/// Handles primitives, `void`, arrays (`String[]`, `int[][]`), generics
/// (`List<String>`, `Map<K, V>`), the diamond operator (`List<>`), simple
/// class names, and fully qualified names (with or without generics).
///
/// Returns `None` for empty input, unmatched brackets, malformed FQNs, or any
/// other structurally invalid input.
#[must_use]
pub fn parse_type_string(type_str: &str) -> Option<Type> {
    let type_str = type_str.trim();

    if type_str.is_empty() {
        debug!("parse_type_string: empty input");
        return None;
    }

    // Wildcards (JLS §4.5.1): `?`, `? extends X`, `? super X`. These appear
    // as type arguments (`Class<?>`, `List<? extends Number>`) — previously
    // they fell through to `split_fqn` and produced a corrupt ClassType named
    // "? extends Number", bypassing wildcard assignability and bound
    // substitution.
    if let Some(rest) = type_str.strip_prefix('?') {
        let rest = rest.trim_start();
        if rest.is_empty() {
            return Some(Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            });
        }
        if let Some(bound_str) = rest.strip_prefix("extends") {
            if bound_str.starts_with(char::is_whitespace) {
                let bound = parse_type_string(bound_str)?;
                return Some(Type::Wildcard {
                    upper_bound: Some(Box::new(bound)),
                    lower_bound: None,
                });
            }
        }
        if let Some(bound_str) = rest.strip_prefix("super") {
            if bound_str.starts_with(char::is_whitespace) {
                let bound = parse_type_string(bound_str)?;
                return Some(Type::Wildcard {
                    upper_bound: None,
                    lower_bound: Some(Box::new(bound)),
                });
            }
        }
        warn!("parse_type_string: malformed wildcard ({type_str})");
        return None;
    }

    // Arrays peel first so `List<String>[]` recurses on `List<String>`.
    if let Some(stripped) = type_str.strip_suffix("[]") {
        let mut dimensions: u8 = 1;
        let mut remaining = stripped;
        while let Some(next) = remaining.strip_suffix("[]") {
            dimensions = dimensions.saturating_add(1);
            remaining = next;
        }
        if remaining.is_empty() {
            warn!("parse_type_string: array with empty element type ({type_str})");
            return None;
        }
        let element_type = parse_type_string(remaining)?;
        return Some(Type::Array {
            element_type: Box::new(element_type),
            dimensions,
        });
    }

    // Generics: `Base<...>`. Base may itself be a fully qualified name.
    if let Some(generic_start) = type_str.find('<') {
        if !type_str.ends_with('>') {
            warn!("parse_type_string: unmatched generic brackets ({type_str})");
            return None;
        }
        let base_name = &type_str[..generic_start];
        let args_str = &type_str[generic_start + 1..type_str.len() - 1];
        if base_name.is_empty() {
            warn!("parse_type_string: generic with empty base name ({type_str})");
            return None;
        }
        // Defensive: a primitive with type arguments is malformed but we have
        // historically accepted it by ignoring the arguments. Preserve that.
        if let Some(prim) = try_parse_primitive(base_name) {
            return Some(prim);
        }
        let type_arguments = parse_type_arguments(args_str);
        let is_diamond = args_str.trim().is_empty();
        let (package, name) = split_fqn(base_name)?;
        return Some(Type::Class(ClassType {
            name,
            package,
            type_arguments,
            is_diamond,
        }));
    }

    if let Some(prim) = try_parse_primitive(type_str) {
        return Some(prim);
    }
    if type_str == "void" {
        return Some(Type::Void);
    }

    let (package, name) = split_fqn(type_str)?;
    Some(Type::Class(ClassType {
        name,
        package,
        type_arguments: vec![],
        is_diamond: false,
    }))
}

/// Strip a trailing identifier (e.g. parameter name or varargs ellipsis) from a
/// type-with-name string like `"String name"` or `"String... args"`.
///
/// Splits on the *last* whitespace outside generic brackets, so
/// `"Map<K, V> map"` returns `"Map<K, V>"`. Trailing `...` is removed.
#[must_use]
pub fn strip_parameter_name(type_str: &str) -> &str {
    let trimmed = type_str.trim();
    let mut depth: i32 = 0;
    let mut last_space = None;
    for (idx, ch) in trimmed.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    debug!("strip_parameter_name: malformed generics in '{trimmed}'");
                    return trimmed;
                }
            }
            ' ' if depth == 0 => last_space = Some(idx),
            _ => {}
        }
    }
    let type_part = last_space.map_or(trimmed, |i| &trimmed[..i]);
    type_part.strip_suffix("...").unwrap_or(type_part)
}

fn try_parse_primitive(s: &str) -> Option<Type> {
    let p = match s {
        "boolean" => PrimitiveType::Boolean,
        "byte" => PrimitiveType::Byte,
        "short" => PrimitiveType::Short,
        "int" => PrimitiveType::Int,
        "long" => PrimitiveType::Long,
        "char" => PrimitiveType::Char,
        "float" => PrimitiveType::Float,
        "double" => PrimitiveType::Double,
        _ => return None,
    };
    Some(Type::Primitive(p))
}

/// Split `"java.util.List"` into `(Some("java.util"), "List")` and `"List"` into
/// `(None, "List")`. Rejects leading/trailing dots and empty package segments.
fn split_fqn(s: &str) -> Option<(Option<String>, String)> {
    if s.starts_with('.') || s.ends_with('.') || s.contains("..") {
        warn!("parse_type_string: malformed FQN ({s})");
        return None;
    }
    Some(s.rsplit_once('.').map_or_else(
        || (None, s.to_string()),
        |(pkg, name)| (Some(pkg.to_string()), name.to_string()),
    ))
}

/// Parse comma-separated type arguments, respecting nested generics.
fn parse_type_arguments(args_str: &str) -> Vec<Type> {
    let mut args = Vec::new();
    let mut start = 0;
    let mut depth: u32 = 0;
    for (i, ch) in args_str.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                push_arg(&mut args, &args_str[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    push_arg(&mut args, &args_str[start..]);
    args
}

fn push_arg(args: &mut Vec<Type>, raw: &str) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(parsed) = parse_type_string(trimmed) {
        args.push(parsed);
    } else {
        debug!("parse_type_string: failed to parse type argument '{trimmed}'");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(name: &str) -> Type {
        Type::Class(ClassType {
            name: name.into(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        })
    }
    fn class_pkg(pkg: &str, name: &str) -> Type {
        Type::Class(ClassType {
            name: name.into(),
            package: Some(pkg.into()),
            type_arguments: vec![],
            is_diamond: false,
        })
    }

    #[test]
    fn primitives() {
        assert_eq!(
            parse_type_string("int"),
            Some(Type::Primitive(PrimitiveType::Int))
        );
        assert_eq!(
            parse_type_string("byte"),
            Some(Type::Primitive(PrimitiveType::Byte))
        );
        assert_eq!(
            parse_type_string("boolean"),
            Some(Type::Primitive(PrimitiveType::Boolean))
        );
    }

    #[test]
    fn void_returns_void() {
        assert_eq!(parse_type_string("void"), Some(Type::Void));
    }

    #[test]
    fn simple_class_and_fqn() {
        assert_eq!(parse_type_string("String"), Some(class("String")));
        assert_eq!(
            parse_type_string("java.lang.String"),
            Some(class_pkg("java.lang", "String"))
        );
    }

    #[test]
    fn array_dimensions() {
        let bytes_1d = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Byte)),
            dimensions: 1,
        };
        let bytes_2d = Type::Array {
            element_type: Box::new(Type::Primitive(PrimitiveType::Byte)),
            dimensions: 2,
        };
        assert_eq!(parse_type_string("byte[]"), Some(bytes_1d));
        assert_eq!(parse_type_string("byte[][]"), Some(bytes_2d));
    }

    #[test]
    fn generic_with_fqn_base_keeps_package() {
        // Regression: an earlier parser dropped the package on FQN+generics,
        // producing `ClassType { name: "java.util.List", package: None, ... }`.
        let parsed = parse_type_string("java.util.List<String>").unwrap();
        let Type::Class(ct) = parsed else {
            panic!("expected Class")
        };
        assert_eq!(ct.name, "List");
        assert_eq!(ct.package.as_deref(), Some("java.util"));
        assert_eq!(ct.type_arguments.len(), 1);
    }

    #[test]
    fn diamond_operator() {
        let parsed = parse_type_string("ArrayList<>").unwrap();
        let Type::Class(ct) = parsed else {
            panic!("expected Class")
        };
        assert!(ct.is_diamond);
        assert!(ct.type_arguments.is_empty());
    }

    #[test]
    fn nested_generics() {
        let parsed = parse_type_string("Map<String, List<Integer>>").unwrap();
        let Type::Class(ct) = parsed else {
            panic!("expected Class")
        };
        assert_eq!(ct.type_arguments.len(), 2);
    }

    #[test]
    fn type_argument_splitting() {
        // Cases ported from the old `split_type_arguments` tests — nested
        // commas inside `<...>` must not split the surrounding argument list.
        fn arg_count(s: &str) -> usize {
            let Type::Class(ct) = parse_type_string(s).unwrap() else {
                panic!("expected Class for {s}")
            };
            ct.type_arguments.len()
        }
        assert_eq!(arg_count("Pair<String, Integer>"), 2);
        assert_eq!(arg_count("Pair<List<A, B>, Set<C>>"), 2);
        assert_eq!(arg_count("Pair<Map<String, List<Integer>>, Set<User>>"), 2);
        assert_eq!(
            arg_count("Triple<Map<String, Map<Integer, List<User>>>, A, B>"),
            3
        );
        assert_eq!(arg_count("Pair<  String  ,  Integer  >"), 2);
        assert_eq!(arg_count("Box<String>"), 1);
    }

    #[test]
    fn malformed_type_argument_is_omitted_instead_of_becoming_unknown() {
        let parsed = parse_type_string("Map<String, invalid..Type>")
            .expect("the valid outer type should remain usable");
        let Type::Class(map) = parsed else {
            panic!("expected Map class type");
        };

        assert!(matches!(
            map.type_arguments.as_slice(),
            [Type::Class(argument)] if argument.name == "String"
        ));
        assert!(!map
            .type_arguments
            .iter()
            .any(|argument| matches!(argument, Type::Unknown)));
    }

    #[test]
    fn array_of_generic() {
        let parsed = parse_type_string("List<String>[]").unwrap();
        let Type::Array {
            element_type,
            dimensions,
        } = parsed
        else {
            panic!("expected Array")
        };
        assert_eq!(dimensions, 1);
        let Type::Class(ct) = element_type.as_ref() else {
            panic!("element should be Class")
        };
        assert_eq!(ct.name, "List");
        assert_eq!(ct.type_arguments.len(), 1);
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(parse_type_string(""), None);
        assert_eq!(parse_type_string("List<String"), None);
        assert_eq!(parse_type_string(".java.lang.String"), None);
        assert_eq!(parse_type_string("java.lang.String."), None);
        assert_eq!(parse_type_string("foo..bar"), None);
        assert_eq!(parse_type_string("[]"), None);
    }

    #[test]
    fn strip_parameter_name_basic() {
        assert_eq!(strip_parameter_name("String name"), "String");
        assert_eq!(strip_parameter_name("String"), "String");
        assert_eq!(strip_parameter_name("Map<K, V> map"), "Map<K, V>");
        assert_eq!(strip_parameter_name("String... args"), "String");
        assert_eq!(
            strip_parameter_name("List<String>... items"),
            "List<String>"
        );
    }

    // Wildcard type arguments must parse to Type::Wildcard, not to
    // a corrupt ClassType named "? extends Number".
    #[test]
    fn parses_unbounded_wildcard() {
        assert_eq!(
            parse_type_string("?"),
            Some(Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            })
        );
    }

    #[test]
    fn parses_upper_bounded_wildcard() {
        match parse_type_string("? extends java.lang.Number") {
            Some(Type::Wildcard {
                upper_bound: Some(bound),
                lower_bound: None,
            }) => assert_eq!(*bound, class_pkg("java.lang", "Number")),
            other => panic!("Expected upper-bounded wildcard, got {other:?}"),
        }
    }

    #[test]
    fn parses_lower_bounded_wildcard() {
        match parse_type_string("? super Integer") {
            Some(Type::Wildcard {
                upper_bound: None,
                lower_bound: Some(bound),
            }) => assert_eq!(*bound, class("Integer")),
            other => panic!("Expected lower-bounded wildcard, got {other:?}"),
        }
    }

    #[test]
    fn parses_wildcard_inside_generic_argument() {
        match parse_type_string("List<? extends Number>") {
            Some(Type::Class(ct)) => {
                assert_eq!(ct.name, "List");
                assert_eq!(ct.type_arguments.len(), 1);
                assert!(
                    matches!(
                        &ct.type_arguments[0],
                        Type::Wildcard {
                            upper_bound: Some(_),
                            lower_bound: None,
                        }
                    ),
                    "argument must be a wildcard, got {:?}",
                    ct.type_arguments[0]
                );
            }
            other => panic!("Expected List with wildcard argument, got {other:?}"),
        }
    }

    #[test]
    fn parses_class_wildcard() {
        match parse_type_string("Class<?>") {
            Some(Type::Class(ct)) => {
                assert_eq!(ct.name, "Class");
                assert_eq!(
                    ct.type_arguments,
                    vec![Type::Wildcard {
                        upper_bound: None,
                        lower_bound: None,
                    }]
                );
                assert!(!ct.is_diamond, "Class<?> is not a diamond");
            }
            other => panic!("Expected Class<?>, got {other:?}"),
        }
    }

    #[test]
    fn accepts_wildcard_without_space_after_question_mark() {
        // `List<?extends Number>` is valid Java: `?` is its own token, so the
        // space between `?` and `extends` is optional.
        match parse_type_string("?extends Number") {
            Some(Type::Wildcard {
                upper_bound: Some(bound),
                lower_bound: None,
            }) => assert_eq!(*bound, class("Number")),
            other => panic!("Expected upper-bounded wildcard, got {other:?}"),
        }
    }

    #[test]
    fn rejects_malformed_wildcards() {
        assert_eq!(parse_type_string("? extendsNumber"), None);
        assert_eq!(parse_type_string("? implements Number"), None);
    }
}
