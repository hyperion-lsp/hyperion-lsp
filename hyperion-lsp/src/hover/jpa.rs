// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JPA relationship metadata for Java field hovers.

use hyperion_parser::syntax::{Annotation, AstNode, SyntaxKind, SyntaxNode};

use super::extractors::extract_field_type_from_node;

const RELATIONSHIPS: [&str; 4] = ["ManyToOne", "OneToMany", "ManyToMany", "OneToOne"];

/// Render relationship facts attached directly to a field declaration.
pub(super) fn relationship_markdown(field: &SyntaxNode) -> Option<String> {
    let annotation = field
        .children()
        .filter(|child| child.kind() == SyntaxKind::MODIFIER_LIST)
        .chain(
            field
                .prev_sibling()
                .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST),
        )
        .flat_map(|modifiers| modifiers.children())
        .filter_map(Annotation::cast)
        .find_map(|annotation| {
            let name = annotation.name()?.as_string();
            let simple_name = name.rsplit('.').next()?.to_string();
            RELATIONSHIPS
                .contains(&simple_name.as_str())
                .then_some((simple_name, annotation))
        })?;

    let (kind, annotation) = annotation;
    let arguments = annotation
        .args()
        .map(|args| {
            args.syntax()
                .children()
                .filter(|arg| arg.kind() == SyntaxKind::ANNOTATION_ARG)
                .filter_map(|arg| {
                    let text = arg.text().to_string();
                    let (name, value) = text.split_once('=')?;
                    Some((name.trim().to_string(), value.trim().to_string()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let argument = |name: &str| {
        arguments
            .iter()
            .find_map(|(candidate, value)| (candidate == name).then_some(value.as_str()))
    };

    let target = argument("targetEntity")
        .map(normalize_class_literal)
        .filter(|value| !value.is_empty())
        .or_else(|| infer_target_from_field(&kind, field));
    let explicit_fetch = argument("fetch").map(normalize_enum_value);
    let fetch = explicit_fetch
        .clone()
        .unwrap_or_else(|| default_fetch(&kind).to_string());
    let cascade = argument("cascade").map(normalize_cascade);
    let mapped_by = argument("mappedBy")
        .map(|value| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty());

    let mut result = format!("**JPA relationship** (`@{kind}`)");
    if let Some(target) = target {
        result.push_str(&format!("\n\n- Target entity: `{target}`"));
    }
    let default_suffix = if explicit_fetch.is_some() {
        ""
    } else {
        " (default)"
    };
    result.push_str(&format!("\n- Fetch: `{fetch}`{default_suffix}"));
    if let Some(cascade) = cascade.filter(|value| !value.is_empty()) {
        result.push_str(&format!("\n- Cascade: `{cascade}`"));
    }
    if let Some(mapped_by) = mapped_by {
        result.push_str(&format!("\n- Side: inverse (`mappedBy = \"{mapped_by}\"`)"));
    } else {
        result.push_str("\n- Side: owning");
    }
    Some(result)
}

fn infer_target_from_field(kind: &str, field: &SyntaxNode) -> Option<String> {
    let field_type = extract_field_type_from_node(field)?;
    if matches!(kind, "OneToMany" | "ManyToMany") {
        let start = field_type.find('<')? + 1;
        let end = field_type.rfind('>')?;
        let first_argument = field_type[start..end].split(',').next()?.trim();
        Some(
            first_argument
                .trim_start_matches("? extends ")
                .trim_start_matches("? super ")
                .to_string(),
        )
    } else {
        Some(field_type)
    }
}

fn default_fetch(kind: &str) -> &'static str {
    match kind {
        "ManyToOne" | "OneToOne" => "EAGER",
        _ => "LAZY",
    }
}

fn normalize_class_literal(value: &str) -> String {
    value.trim().trim_end_matches(".class").trim().to_string()
}

fn normalize_enum_value(value: &str) -> String {
    value
        .trim()
        .rsplit('.')
        .next()
        .unwrap_or(value)
        .trim()
        .to_string()
}

fn normalize_cascade(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split(',')
        .map(normalize_enum_value)
        .collect::<Vec<_>>()
        .join(", ")
}
