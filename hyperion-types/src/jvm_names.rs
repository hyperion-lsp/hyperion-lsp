// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Naming rules that connect Kotlin properties and Java accessors.
//!
//! Kotlin -> JVM: a property `note` compiles to `getNote()` / `setNote(v)`; a
//! property whose name starts with `is` keeps its name as getter and swaps the
//! prefix for `set`. Java -> Kotlin: `getRadius()` is seen as the synthetic
//! property `radius`, `isActive()` as `isActive`, `setRadius(x)` as its setter.

/// `Foo` from `foo`; the first character is upper-cased, nothing else changes.
#[must_use]
pub fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Whether a property name follows the `isFoo` convention (`is` + upper case).
#[must_use]
pub fn is_boolean_style_name(property: &str) -> bool {
    property.len() > 2
        && property.starts_with("is")
        && property[2..].chars().next().is_some_and(char::is_uppercase)
}

/// The JVM getter and setter names of a Kotlin property.
#[must_use]
pub fn accessor_names(property: &str) -> (String, String) {
    if is_boolean_style_name(property) {
        (property.to_string(), format!("set{}", &property[2..]))
    } else {
        let capitalized = capitalize(property);
        (format!("get{capitalized}"), format!("set{capitalized}"))
    }
}

/// Java getter names that make `property` visible to Kotlin, most specific first.
#[must_use]
pub fn java_getter_names(property: &str) -> Vec<String> {
    let mut names = vec![format!("get{}", capitalize(property))];
    if is_boolean_style_name(property) {
        names.push(property.to_string());
    }
    names
}

/// The setter name a Kotlin assignment `obj.property = v` compiles to.
#[must_use]
pub fn java_setter_name(property: &str) -> String {
    accessor_names(property).1
}

/// What a JVM method name means for a property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessorKind {
    Getter,
    BooleanGetter,
    Setter,
}

/// Decompose `getRadius` / `isActive` / `setRadius` into the Kotlin property
/// name and accessor kind.
///
/// Kotlin lower-cases the leading upper-case run of the remainder
/// (`getURL` -> `url`, `getXCoord` -> `xCoord`).
#[must_use]
pub fn property_from_accessor(method: &str) -> Option<(String, AccessorKind)> {
    let (rest, kind) = if let Some(rest) = method.strip_prefix("get") {
        (rest, AccessorKind::Getter)
    } else if let Some(rest) = method.strip_prefix("set") {
        (rest, AccessorKind::Setter)
    } else if method.starts_with("is") && is_boolean_style_name(method) {
        return Some((method.to_string(), AccessorKind::BooleanGetter));
    } else {
        return None;
    };
    if !rest.chars().next().is_some_and(char::is_uppercase) {
        return None;
    }
    Some((decapitalize_leading_run(rest), kind))
}

fn decapitalize_leading_run(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let upper_run = chars.iter().take_while(|c| c.is_uppercase()).count();
    if upper_run <= 1 || upper_run == chars.len() {
        // `Radius` -> `radius`, `URL` -> `url`
        let lowered: String = chars
            .iter()
            .take(upper_run.max(1))
            .flat_map(|c| c.to_lowercase())
            .collect();
        let rest: String = chars.iter().skip(upper_run.max(1)).collect();
        return lowered + &rest;
    }
    // `URLString` -> `urlString`: lower all but the last upper-case letter of the run
    let lowered: String = chars
        .iter()
        .take(upper_run - 1)
        .flat_map(|c| c.to_lowercase())
        .collect();
    let rest: String = chars.iter().skip(upper_run - 1).collect();
    lowered + &rest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessor_naming_follows_kotlin() {
        assert_eq!(accessor_names("note"), ("getNote".into(), "setNote".into()));
        assert_eq!(
            accessor_names("isActive"),
            ("isActive".into(), "setActive".into())
        );
        assert_eq!(
            accessor_names("issue"),
            ("getIssue".into(), "setIssue".into())
        );
        assert_eq!(accessor_names("url"), ("getUrl".into(), "setUrl".into()));
    }

    #[test]
    fn synthetic_property_names_follow_kotlin() {
        assert_eq!(
            property_from_accessor("getRadius"),
            Some(("radius".into(), AccessorKind::Getter))
        );
        assert_eq!(
            property_from_accessor("getURL"),
            Some(("url".into(), AccessorKind::Getter))
        );
        assert_eq!(
            property_from_accessor("getURLString"),
            Some(("urlString".into(), AccessorKind::Getter))
        );
        assert_eq!(
            property_from_accessor("isActive"),
            Some(("isActive".into(), AccessorKind::BooleanGetter))
        );
        assert_eq!(
            property_from_accessor("setName"),
            Some(("name".into(), AccessorKind::Setter))
        );
        assert_eq!(property_from_accessor("getaway"), None);
        assert_eq!(property_from_accessor("island"), None);
    }
}
