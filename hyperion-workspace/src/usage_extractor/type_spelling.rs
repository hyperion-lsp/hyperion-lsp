// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Structural helpers for Java type spellings used during usage extraction.
//! Parameterized receiver information must survive lexical type lookup so a
//! later chain segment can project a generic member return type exactly.

use crate::import_resolver::ImportResolver;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParameterizedType {
    pub(super) base: String,
    pub(super) arguments: Vec<String>,
}

pub(super) fn parse_parameterized_type(written: &str) -> Option<ParameterizedType> {
    let written = written.trim();
    if written.is_empty() {
        return None;
    }
    let Some(open) = written.find('<') else {
        return Some(ParameterizedType {
            base: written.to_string(),
            arguments: Vec::new(),
        });
    };
    let base = written[..open].trim();
    if base.is_empty() {
        return None;
    }

    let mut depth = 0_u32;
    let mut close = None;
    for (relative, character) in written[open..].char_indices() {
        match character {
            '<' => depth = depth.checked_add(1)?,
            '>' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    close = Some(open + relative);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    if !written[close + 1..].trim().is_empty() {
        return None;
    }

    let mut arguments = Vec::new();
    let inner = &written[open + 1..close];
    let mut depth = 0_u32;
    let mut start = 0;
    for (offset, character) in inner.char_indices() {
        match character {
            '<' => depth = depth.checked_add(1)?,
            '>' => depth = depth.checked_sub(1)?,
            ',' if depth == 0 => {
                let argument = inner[start..offset].trim();
                if argument.is_empty() {
                    return None;
                }
                arguments.push(argument.to_string());
                start = offset + character.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    let argument = inner[start..].trim();
    if argument.is_empty() {
        return None;
    }
    arguments.push(argument.to_string());

    Some(ParameterizedType {
        base: base.to_string(),
        arguments,
    })
}

pub(super) fn resolve_parameterized_type_in_owner(
    written: &str,
    resolver: &ImportResolver<'_>,
    owner_fqn: Option<&str>,
) -> Option<String> {
    let parsed = parse_parameterized_type(written)?;
    let crate::TypeNameResolution::Exact(base) =
        resolver.resolve_type_identity_in_owner(&parsed.base, owner_fqn)
    else {
        return None;
    };
    if parsed.arguments.is_empty() {
        return Some(base);
    }

    let arguments = parsed
        .arguments
        .iter()
        .map(|argument| resolve_parameterized_type_in_owner(argument, resolver, owner_fqn))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{base}<{}>", arguments.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_parameterized_type_without_erasure() {
        assert_eq!(
            parse_parameterized_type("p.Factory<java.util.Map<String, p.Service>>"),
            Some(ParameterizedType {
                base: "p.Factory".to_string(),
                arguments: vec!["java.util.Map<String, p.Service>".to_string()],
            })
        );
        assert!(parse_parameterized_type("Factory<Service").is_none());
        assert!(parse_parameterized_type("Factory<Service> trailing").is_none());
    }
}
