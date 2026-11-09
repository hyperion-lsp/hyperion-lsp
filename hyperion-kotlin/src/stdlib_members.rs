// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Members and extensions of the Kotlin standard library, for completion.
//!
//! `stdlib_members.tsv` is generated from the `kotlin-stdlib` 2.2 sources
//! (declaration headers only): `T` rows describe a type and its supertypes,
//! `M` rows a member function/property of a type, `E` rows an extension
//! function/property with its receiver type. Nothing here is resolved beyond
//! names and Kotlin-spelled signatures: the point is offering the right names
//! with the right shape after `"x".`, `list.` or `map.`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Whether an entry is a function or a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Function,
    Property,
}

/// One stdlib member or extension.
#[derive(Debug, Clone)]
pub struct StdlibEntry {
    /// `true` for a member declared in the type, `false` for an extension.
    pub is_member: bool,
    /// Type (member) or receiver type as written (extension), e.g. `Iterable<T>`.
    pub receiver: &'static str,
    pub name: &'static str,
    pub kind: EntryKind,
    /// Own type parameters, e.g. `<T, R>`.
    pub type_parameters: &'static str,
    /// Parameter list as written, without parentheses.
    pub parameters: &'static str,
    pub return_type: &'static str,
    pub is_infix: bool,
    pub is_operator: bool,
}

impl StdlibEntry {
    /// The last parameter is a function type and the only required one, so
    /// the call is normally written `name { .. }`.
    #[must_use]
    pub fn takes_trailing_lambda(&self) -> bool {
        let last = split_top_level(self.parameters).pop().unwrap_or_default();
        last.contains("->")
            && split_top_level(self.parameters)
                .iter()
                .filter(|parameter| !parameter.contains('='))
                .count()
                == 1
    }
}

struct Table {
    members: HashMap<&'static str, Vec<StdlibEntry>>,
    extensions: Vec<StdlibEntry>,
    supertypes: HashMap<&'static str, Vec<&'static str>>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut members: HashMap<&'static str, Vec<StdlibEntry>> = HashMap::new();
        let mut extensions = Vec::new();
        let mut supertypes: HashMap<&'static str, Vec<&'static str>> = HashMap::new();
        for line in include_str!("stdlib_members.tsv").lines() {
            let mut fields = line.split('\t');
            match fields.next() {
                Some("T") => {
                    let (Some(name), Some(_type_parameters), Some(supers)) =
                        (fields.next(), fields.next(), fields.next())
                    else {
                        continue;
                    };
                    supertypes.insert(
                        name,
                        split_top_level(supers)
                            .into_iter()
                            .map(base_name)
                            .filter(|name| !name.is_empty())
                            .collect(),
                    );
                }
                Some(tag @ ("M" | "E")) => {
                    let parts: Vec<&'static str> = fields.collect();
                    if parts.len() < 7 {
                        continue;
                    }
                    let flags = parts[6];
                    let entry = StdlibEntry {
                        is_member: tag == "M",
                        receiver: parts[0],
                        name: parts[1],
                        kind: if parts[2] == "fun" {
                            EntryKind::Function
                        } else {
                            EntryKind::Property
                        },
                        type_parameters: parts[3],
                        parameters: parts[4],
                        return_type: parts[5],
                        is_infix: flags.contains("infix"),
                        is_operator: flags.contains("operator"),
                    };
                    if entry.is_member {
                        members.entry(entry.receiver).or_default().push(entry);
                    } else {
                        extensions.push(entry);
                    }
                }
                _ => {}
            }
        }
        Table {
            members,
            extensions,
            supertypes,
        }
    })
}

/// `Iterable<T>` -> `Iterable`, `Array<out T>?` -> `Array`.
#[must_use]
pub fn base_name(type_text: &str) -> &str {
    let trimmed = type_text.trim().trim_end_matches('?');
    trimmed.split('<').next().unwrap_or(trimmed).trim()
}

/// Split `A, Map<B, C>, (D) -> E` at top-level commas.
#[must_use]
pub fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = text.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' if index > 0 && bytes[index - 1] == b'-' => {}
            b'>' | b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

/// Names of `type_name` and everything it is a subtype of, nearest first, `Any`
/// last. Unknown types still reach `Any`.
#[must_use]
pub fn supertype_closure(type_name: &str) -> Vec<&'static str> {
    let table = table();
    let mut closure: Vec<&'static str> = Vec::new();
    let mut queue: Vec<&str> = vec![type_name];
    while let Some(next) = queue.pop() {
        let next = base_name(next);
        let canonical = table
            .supertypes
            .get_key_value(next)
            .map(|(name, _)| *name)
            .or_else(|| table.members.get_key_value(next).map(|(name, _)| *name));
        let Some(name) = canonical else {
            continue;
        };
        if closure.contains(&name) {
            continue;
        }
        closure.push(name);
        if let Some(supers) = table.supertypes.get(name) {
            queue.extend(supers.iter().rev());
        }
    }
    if !closure.contains(&"Any") {
        closure.push("Any");
    }
    closure
}

/// A bound `T : Bound` of the extension's own type parameter `receiver_param`.
fn receiver_bound(type_parameters: &str, receiver_param: &str) -> Option<String> {
    let inner = type_parameters
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    for parameter in split_top_level(inner) {
        let mut parts = parameter.splitn(2, ':');
        let name = parts.next()?.trim();
        if name == receiver_param {
            return parts.next().map(|bound| base_name(bound).to_string());
        }
    }
    None
}

/// Whether `receiver`'s type arguments could accept `actual_arguments`
/// (simple names; empty means "unknown, accept").
fn arguments_compatible(entry: &StdlibEntry, actual_arguments: &[String]) -> bool {
    if actual_arguments.is_empty() {
        return true;
    }
    let Some(open) = entry.receiver.find('<') else {
        return true;
    };
    let inner = entry.receiver[open + 1..]
        .trim_end_matches('?')
        .trim_end_matches('>');
    let own: Vec<&str> = split_top_level(inner);
    let declared_parameters: Vec<&str> = {
        let params = entry
            .type_parameters
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        split_top_level(params)
            .into_iter()
            .map(|parameter| parameter.split(':').next().unwrap_or("").trim())
            .collect()
    };
    for (index, argument) in own.iter().enumerate() {
        let argument = argument
            .trim_start_matches("out ")
            .trim_start_matches("in ")
            .trim();
        if argument == "*" || declared_parameters.contains(&argument) {
            continue;
        }
        // A concrete argument such as `Int` in `Iterable<Int>.sum()`.
        let concrete = base_name(argument);
        if let Some(actual) = actual_arguments.get(index) {
            let actual = base_name(actual);
            if !actual.is_empty()
                && actual != "?"
                && actual != concrete
                && actual.chars().count() > 1
                && concrete != "Any"
                && concrete != "Comparable"
                && concrete != "Number"
            {
                return false;
            }
        }
    }
    true
}

/// Members and extensions applicable to a value of Kotlin type `type_name`
/// (simple name, e.g. `String`, `List`, `Int`). `type_arguments` are the
/// simple names of its type arguments when known.
///
/// `extra_supertypes` lets a caller extend the closure with names the table
/// cannot know (project types that extend `Iterable`, mapped Java types).
#[must_use]
pub fn entries_for(
    type_name: &str,
    type_arguments: &[String],
    extra_supertypes: &[&str],
) -> Vec<&'static StdlibEntry> {
    let table = table();
    let mut closure = supertype_closure(type_name);
    for extra in extra_supertypes {
        for name in supertype_closure(extra) {
            if !closure.contains(&name) {
                closure.push(name);
            }
        }
    }

    let mut result: Vec<&'static StdlibEntry> = Vec::new();
    for owner in &closure {
        if let Some(entries) = table.members.get(owner) {
            result.extend(entries.iter());
        }
    }
    for entry in &table.extensions {
        let receiver_base = base_name(entry.receiver);
        let applicable = if receiver_base == "T" || receiver_base == "R" {
            match receiver_bound(entry.type_parameters, receiver_base) {
                None => true,
                Some(bound) => closure.contains(&bound.as_str()) || bound == "Any",
            }
        } else {
            closure.contains(&receiver_base) && arguments_compatible(entry, type_arguments)
        };
        if applicable {
            result.push(entry);
        }
    }
    result
}

#[cfg(test)]
fn names_contains_sum_for_int() -> bool {
    entries_for("List", &["Int".to_string()], &[])
        .iter()
        .any(|entry| entry.name == "sum")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(type_name: &str, arguments: &[&str]) -> Vec<&'static str> {
        let arguments: Vec<String> = arguments.iter().map(|a| (*a).to_string()).collect();
        entries_for(type_name, &arguments, &[])
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    #[test]
    fn string_has_members_and_text_extensions() {
        let names = names("String", &[]);
        for expected in [
            "length",
            "uppercase",
            "trim",
            "substring",
            "toInt",
            "lines",
            "let",
        ] {
            assert!(names.contains(&expected), "String lacks {expected}");
        }
        assert!(!names.contains(&"keys"), "String is not a Map");
        assert!(!names.contains(&"size"), "String has length, not size");
    }

    #[test]
    fn list_has_collection_members_and_iterable_extensions() {
        let names = names("List", &["String"]);
        for expected in [
            "size",
            "get",
            "first",
            "map",
            "filter",
            "forEach",
            "joinToString",
        ] {
            assert!(names.contains(&expected), "List lacks {expected}");
        }
        assert!(!names.contains(&"sum"), "List<String> has no sum()");
        assert!(super::names_contains_sum_for_int());
    }

    #[test]
    fn map_has_keys_values_and_entries() {
        let names = names("Map", &["String", "Int"]);
        for expected in [
            "keys",
            "values",
            "entries",
            "containsKey",
            "getOrDefault",
            "mapValues",
        ] {
            assert!(names.contains(&expected), "Map lacks {expected}");
        }
    }

    #[test]
    fn any_receiver_extensions_apply_everywhere() {
        let names = names("SomethingUnknown", &[]);
        for expected in ["let", "also", "apply", "run", "takeIf", "toString"] {
            assert!(names.contains(&expected), "Any lacks {expected}");
        }
    }
}
