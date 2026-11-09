// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Canonical extraction of unresolved symbols from compiler diagnostics.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UnresolvedMethod {
    pub name: String,
    pub parameter_types: Vec<String>,
}

pub(super) fn unresolved_symbol(message: &str) -> Option<String> {
    diagnostic_subject(message).and_then(subject_name)
}

pub(super) fn unresolved_method(message: &str) -> Option<UnresolvedMethod> {
    let signature = diagnostic_subject(message)?;
    let signature = strip_subject_kind(signature, "method")?;
    let signature = signature.trim_start_matches(['\'', '"']);
    let name_end = signature
        .find(|character: char| character == '(' || character.is_whitespace())
        .unwrap_or(signature.len());
    let name = signature[..name_end].trim_matches(['\'', '"']);
    if name.is_empty() {
        return None;
    }

    let parameter_types = signature
        .get(name_end..)
        .and_then(|rest| rest.trim_start().strip_prefix('('))
        .and_then(|parameters| parameters.rsplit_once(')'))
        .map_or_else(Vec::new, |(parameters, _)| {
            split_parameter_types(parameters)
        });

    Some(UnresolvedMethod {
        name: name.to_owned(),
        parameter_types,
    })
}

fn diagnostic_subject(message: &str) -> Option<&str> {
    message
        .lines()
        .find_map(|line| line.trim().strip_prefix("symbol:").map(str::trim))
        .or_else(|| {
            message
                .split_once("cannot find symbol:")
                .map(|(_, subject)| subject.trim())
        })
        .or_else(|| {
            message
                .split_once("cannot resolve symbol")
                .map(|(_, subject)| subject.trim())
        })
        .or_else(|| {
            message
                .split_once("unresolved reference:")
                .map(|(_, subject)| subject.trim())
        })
}

fn subject_name(subject: &str) -> Option<String> {
    let subject = [
        "class",
        "interface",
        "enum",
        "record",
        "variable",
        "field",
        "method",
    ]
    .into_iter()
    .find_map(|kind| strip_subject_kind(subject, kind))
    .unwrap_or(subject)
    .trim_start_matches(['\'', '"']);
    let name_end = subject
        .find(|character: char| {
            character.is_whitespace()
                || matches!(character, '\'' | '"' | '(' | ')' | '[' | ']' | '<' | '>')
        })
        .unwrap_or(subject.len());
    let name = subject[..name_end].trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn strip_subject_kind<'a>(subject: &'a str, kind: &str) -> Option<&'a str> {
    let rest = subject.strip_prefix(kind)?;
    rest.chars()
        .next()
        .is_none_or(|character| character.is_whitespace())
        .then(|| rest.trim_start())
}

fn split_parameter_types(parameters: &str) -> Vec<String> {
    let mut types = Vec::new();
    let mut start = 0;
    let mut angle_depth = 0_u32;
    let mut array_depth = 0_u32;

    for (offset, character) in parameters.char_indices() {
        match character {
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '[' => array_depth += 1,
            ']' => array_depth = array_depth.saturating_sub(1),
            ',' if angle_depth == 0 && array_depth == 0 => {
                push_parameter_type(&mut types, &parameters[start..offset]);
                start = offset + character.len_utf8();
            }
            _ => {}
        }
    }
    push_parameter_type(&mut types, &parameters[start..]);
    types
}

fn push_parameter_type(types: &mut Vec<String>, parameter: &str) {
    let parameter = parameter.trim();
    if !parameter.is_empty() {
        types.push(parameter.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::{unresolved_method, unresolved_symbol, UnresolvedMethod};

    #[test]
    fn extracts_compact_diagnostic_subjects() {
        assert_eq!(
            unresolved_symbol("cannot find symbol: List"),
            Some("List".to_owned())
        );
        assert_eq!(
            unresolved_symbol("cannot resolve symbol 'ArrayList'"),
            Some("ArrayList".to_owned())
        );
        assert_eq!(
            unresolved_symbol("unresolved reference: HashMap"),
            Some("HashMap".to_owned())
        );
    }

    #[test]
    fn extracts_javac_multiline_symbol_instead_of_the_kind() {
        let message = "cannot find symbol\n  symbol:   class ArrayList\n  location: class Test";
        assert_eq!(unresolved_symbol(message), Some("ArrayList".to_owned()));
    }

    #[test]
    fn extracts_method_name_and_top_level_parameter_types() {
        let message = "cannot find symbol\n  symbol: method transform(java.util.Map<String, Integer>, String[])\n  location: class Test";
        assert_eq!(
            unresolved_method(message),
            Some(UnresolvedMethod {
                name: "transform".to_owned(),
                parameter_types: vec![
                    "java.util.Map<String, Integer>".to_owned(),
                    "String[]".to_owned(),
                ],
            })
        );
    }

    #[test]
    fn rejects_diagnostics_without_an_unresolved_subject() {
        assert_eq!(unresolved_symbol("type mismatch"), None);
        assert_eq!(unresolved_method("cannot find symbol: field value"), None);
    }
}
