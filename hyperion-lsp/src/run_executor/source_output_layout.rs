// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maps JVM source declarations to their compiled outputs and verifies freshness.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use hyperion_kotlin::{
    parse as parse_kotlin, SyntaxKind as KotlinSyntaxKind, SyntaxNode as KotlinSyntaxNode,
};

use super::collect_language_files;

pub(super) fn ensure_source_files_fresh(
    output_dir: &Path,
    source_root: &Path,
    sources: &[PathBuf],
) -> Result<(), String> {
    for source in sources {
        let source_mtime = fs::metadata(source)
            .and_then(|metadata| metadata.modified())
            .map_err(|error| {
                format!(
                    "cannot verify compiled output freshness: failed to read mtime for {}: {error}",
                    source.display()
                )
            })?;

        let expected = expected_class_files(output_dir, source_root, source)?;
        let class_mtime = |class_file: &Path| -> Result<Option<SystemTime>, String> {
            match fs::metadata(class_file).and_then(|metadata| metadata.modified()) {
                Ok(mtime) => Ok(Some(mtime)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(format!(
                    "compiled output at {} cannot be verified for {}: class metadata for {} is unreadable ({error})",
                    output_dir.display(),
                    source.display(),
                    class_file.display()
                )),
            }
        };
        for class_file in &expected.mandatory {
            let Some(class_mtime) = class_mtime(class_file)? else {
                return Err(format!(
                    "compiled output at {} cannot be verified for {}: no corresponding class was found at {}",
                    output_dir.display(),
                    source.display(),
                    class_file.display()
                ));
            };
            if source_mtime > class_mtime {
                return Err(format!(
                    "compiled output at {} is stale ({} is newer than {})",
                    output_dir.display(),
                    source.display(),
                    class_file.display()
                ));
            }
        }

        let mut found_alternative = false;
        for class_file in &expected.alternatives {
            if let Some(class_mtime) = class_mtime(class_file)? {
                found_alternative = true;
                if source_mtime > class_mtime {
                    return Err(format!(
                        "compiled output at {} is stale ({} is newer than {})",
                        output_dir.display(),
                        source.display(),
                        class_file.display()
                    ));
                }
            }
        }
        if expected.mandatory.is_empty() && !found_alternative {
            return Err(format!(
                "compiled output at {} cannot be verified for {}: no corresponding class was found",
                output_dir.display(),
                source.display()
            ));
        }
    }

    Ok(())
}

pub(super) fn ensure_cached_output_dirs_fresh(output_dirs: &[PathBuf]) -> Result<(), String> {
    for output_dir in output_dirs {
        let Some((source_root, language)) = conventional_source_root(output_dir) else {
            if output_dir.is_dir() {
                return Err(format!(
                    "cached compiled output has an unrecognized layout: {}",
                    output_dir.display()
                ));
            }
            continue;
        };
        if !source_root.is_dir() {
            if output_dir.is_dir() {
                return Err(format!(
                    "cached compiled output {} has no corresponding source root at {}",
                    output_dir.display(),
                    source_root.display()
                ));
            }
            continue;
        }
        let mut sources = Vec::new();
        collect_language_files(&source_root, &language, &mut sources)?;
        if sources.is_empty() {
            if output_dir.is_dir() {
                return Err(format!(
                    "cached compiled output {} cannot be verified because source root {} is empty",
                    output_dir.display(),
                    source_root.display()
                ));
            }
            continue;
        }
        if !output_dir.is_dir() {
            return Err(format!(
                "cached compiled output {} is missing for source root {}",
                output_dir.display(),
                source_root.display()
            ));
        }
        ensure_source_files_fresh(output_dir, &source_root, &sources)?;
    }
    Ok(())
}

fn conventional_source_root(output_dir: &Path) -> Option<(PathBuf, String)> {
    let set_name = output_dir.file_name()?.to_str()?;
    let language_dir = output_dir.parent()?;
    let language = language_dir.file_name()?.to_str()?.to_string();
    if !super::VERIFIABLE_LANGUAGES.contains(&language.as_str()) {
        return None;
    }
    let classes_dir = language_dir.parent()?;
    if classes_dir.file_name()?.to_str()? != "classes" {
        return None;
    }
    let build_dir = classes_dir.parent()?;
    if build_dir.file_name()?.to_str()? != "build" {
        return None;
    }
    let project_root = build_dir.parent()?;
    Some((
        project_root.join("src").join(set_name).join(&language),
        language,
    ))
}

struct ExpectedClassFiles {
    mandatory: Vec<PathBuf>,
    alternatives: Vec<PathBuf>,
}

fn expected_class_files(
    output_dir: &Path,
    source_root: &Path,
    source: &Path,
) -> Result<ExpectedClassFiles, String> {
    let relative_source = source.strip_prefix(source_root).map_err(|error| {
        format!(
            "cannot verify compiled output freshness: {} is outside source root {} ({error})",
            source.display(),
            source_root.display()
        )
    })?;
    let physical_parent = relative_source
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    let stem = relative_source
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("source path has no usable file stem: {}", source.display()))?;

    match source.extension().and_then(|extension| extension.to_str()) {
        Some("java") => {
            let source_text = hyperion_workspace::read_source_file(source).map_err(|error| {
                format!(
                    "cannot verify Java output freshness: failed to read {}: {error}",
                    source.display()
                )
            })?;
            let parent = package_path_from_source(&source_text).unwrap_or_default();
            let names = java_declared_type_names(&source_text);
            let names = if names.is_empty() {
                vec![stem.to_string()]
            } else {
                names
            };
            Ok(ExpectedClassFiles {
                mandatory: names
                    .into_iter()
                    .map(|name| output_dir.join(&parent).join(format!("{name}.class")))
                    .collect(),
                alternatives: Vec::new(),
            })
        }
        Some("kt" | "kts") => {
            let source_text = hyperion_workspace::read_source_file(source).map_err(|error| {
                format!(
                    "cannot verify Kotlin output freshness: failed to read {}: {error}",
                    source.display()
                )
            })?;
            let tree = parse_kotlin(&source_text);
            let root = KotlinSyntaxNode::new_root(tree.green_node);
            let parent = kotlin_package_path(&root)
                .or_else(|| package_path_from_source(&source_text))
                .unwrap_or_default();
            let mut mandatory = Vec::new();
            let mut has_top_level_callable = false;
            for child in root.children() {
                match child.kind() {
                    KotlinSyntaxKind::CLASS_DECL
                    | KotlinSyntaxKind::OBJECT_DECL
                    | KotlinSyntaxKind::INTERFACE_DECL => {
                        if let Some(name) = child
                            .children_with_tokens()
                            .filter_map(|element| element.into_token())
                            .find_map(kotlin_identifier)
                        {
                            mandatory.push(output_dir.join(&parent).join(format!("{name}.class")));
                        }
                    }
                    KotlinSyntaxKind::FUNCTION_DECL | KotlinSyntaxKind::PROPERTY_DECL => {
                        has_top_level_callable = true;
                    }
                    _ => {}
                }
            }
            if has_top_level_callable {
                let facade = kotlin_facade_name(&root, stem);
                mandatory.push(output_dir.join(&parent).join(format!("{facade}.class")));
                if kotlin_has_file_annotation(&root, "JvmMultifileClass") {
                    let part = kotlin_multifile_part_name(stem);
                    mandatory.push(
                        output_dir
                            .join(&parent)
                            .join(format!("{facade}__{part}.class")),
                    );
                }
            }
            if mandatory.is_empty() {
                mandatory.push(
                    output_dir
                        .join(&parent)
                        .join(format!("{}.class", kotlin_facade_name(&root, stem))),
                );
            }
            Ok(ExpectedClassFiles {
                mandatory,
                alternatives: Vec::new(),
            })
        }
        Some("scala") => {
            let source_text = hyperion_workspace::read_source_file(source).map_err(|error| {
                format!(
                    "cannot verify Scala output freshness: failed to read {}: {error}",
                    source.display()
                )
            })?;
            let declarations = scala_declaration_names(&source_text);
            if declarations.is_empty() {
                let parent = scala_package_path(&source_text).unwrap_or_default();
                if scala_has_top_level_package_definitions(&source_text) {
                    return Ok(ExpectedClassFiles {
                        mandatory: vec![
                            output_dir
                                .join(&parent)
                                .join(format!("{stem}$package.class")),
                            output_dir
                                .join(&parent)
                                .join(format!("{stem}$package$.class")),
                        ],
                        alternatives: Vec::new(),
                    });
                }
                return Ok(ExpectedClassFiles {
                    mandatory: Vec::new(),
                    alternatives: vec![
                        output_dir.join(&parent).join(format!("{stem}.class")),
                        output_dir.join(&parent).join(format!("{stem}$.class")),
                        output_dir
                            .join(&parent)
                            .join(format!("{stem}$package.class")),
                        output_dir
                            .join(&parent)
                            .join(format!("{stem}$package$.class")),
                    ],
                });
            }
            let mut mandatory = Vec::new();
            for declaration in declarations {
                if declaration.is_package_part {
                    mandatory.push(
                        output_dir
                            .join(&declaration.parent)
                            .join(format!("{stem}$package.class")),
                    );
                    mandatory.push(
                        output_dir
                            .join(&declaration.parent)
                            .join(format!("{stem}$package$.class")),
                    );
                } else if declaration.is_package_object {
                    let package_dir = output_dir.join(&declaration.parent).join(&declaration.name);
                    mandatory.push(package_dir.join("package.class"));
                    mandatory.push(package_dir.join("package$.class"));
                } else {
                    mandatory.push(
                        output_dir
                            .join(&declaration.parent)
                            .join(format!("{}.class", declaration.name)),
                    );
                    if declaration.is_object {
                        mandatory.push(
                            output_dir
                                .join(&declaration.parent)
                                .join(format!("{}$.class", declaration.name)),
                        );
                    }
                }
            }
            Ok(ExpectedClassFiles {
                mandatory,
                alternatives: Vec::new(),
            })
        }
        _ => Ok(ExpectedClassFiles {
            mandatory: vec![output_dir
                .join(&physical_parent)
                .join(format!("{stem}.class"))],
            alternatives: Vec::new(),
        }),
    }
}

fn kotlin_package_path(root: &KotlinSyntaxNode) -> Option<PathBuf> {
    let package = root
        .children()
        .find(|child| child.kind() == KotlinSyntaxKind::PACKAGE_DECL)?;
    let mut path = PathBuf::new();
    for token in package
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
    {
        if matches!(
            token.kind(),
            KotlinSyntaxKind::IDENT | KotlinSyntaxKind::BACKTICK_IDENT
        ) {
            path.push(kotlin_identifier(token).unwrap_or_default());
        } else if token.kind().is_keyword() && token.kind() != KotlinSyntaxKind::PACKAGE_KW {
            path.push(token.text());
        }
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

fn package_path_from_source(source_text: &str) -> Option<PathBuf> {
    let sanitized = scala_sanitize(source_text);
    for line in sanitized.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("package") else {
            continue;
        };
        if rest
            .chars()
            .next()
            .is_some_and(|character| character.is_alphanumeric() || matches!(character, '_' | '$'))
        {
            continue;
        }
        let mut path = PathBuf::new();
        for (token, _) in scala_tokens(rest) {
            if matches!(token.as_str(), "{" | ":" | ";") {
                break;
            }
            path.push(token);
        }
        if !path.as_os_str().is_empty() {
            return Some(path);
        }
    }
    None
}

fn kotlin_identifier(token: hyperion_kotlin::SyntaxToken) -> Option<String> {
    match token.kind() {
        KotlinSyntaxKind::IDENT => Some(token.text().to_string()),
        KotlinSyntaxKind::BACKTICK_IDENT => Some(token.text().trim_matches('`').to_string()),
        _ => None,
    }
}

fn java_declared_type_names(source_text: &str) -> Vec<String> {
    let tokens = scala_tokens(&scala_sanitize(source_text));
    let mut depth = 0usize;
    let mut names = Vec::new();
    for index in 0..tokens.len() {
        match tokens[index].0.as_str() {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            "class" | "interface" | "enum" | "record" if depth == 0 => {
                if let Some((name, _)) = tokens.get(index + 1) {
                    names.push(name.clone());
                }
            }
            _ => {}
        }
    }
    names.sort();
    names.dedup();
    names
}

fn decode_kotlin_string_literal(value: &str) -> String {
    let value = value
        .strip_prefix("\"\"\"")
        .and_then(|value| value.strip_suffix("\"\"\""))
        .or_else(|| {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap_or(value);
    let mut decoded = String::with_capacity(value.len());
    let mut escaped = false;
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if !escaped {
            if character == '\\' {
                escaped = true;
            } else {
                decoded.push(character);
            }
            continue;
        }
        if character == 'u' {
            let mut digits = String::new();
            for _ in 0..4 {
                if let Some(digit) = characters.next() {
                    digits.push(digit);
                }
            }
            if digits.len() == 4 && digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
                if let Some(decoded_character) = u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    decoded.push(decoded_character);
                    escaped = false;
                    continue;
                }
            }
            decoded.push('u');
            decoded.push_str(&digits);
            escaped = false;
            continue;
        }
        decoded.push(match character {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'b' => '\u{0008}',
            'f' => '\u{000c}',
            '\\' => '\\',
            '"' => '"',
            '\'' => '\'',
            other => other,
        });
        escaped = false;
    }
    if escaped {
        decoded.push('\\');
    }
    decoded
}

fn kotlin_facade_name(root: &KotlinSyntaxNode, stem: &str) -> String {
    // File annotations are direct children of the file node, before the package
    // declaration; annotations under a MODIFIERS node are also accepted.
    let direct = root
        .children()
        .filter(|child| child.kind() == KotlinSyntaxKind::ANNOTATION)
        .map(|annotation| vec![annotation]);
    let wrapped = root
        .children()
        .filter(|child| child.kind() == KotlinSyntaxKind::MODIFIERS)
        .map(|modifiers| {
            modifiers
                .children()
                .filter(|child| child.kind() == KotlinSyntaxKind::ANNOTATION)
                .collect::<Vec<_>>()
        });
    for annotations in direct.chain(wrapped) {
        for annotation in annotations {
            let mut name_tokens = Vec::new();
            for element in annotation.children_with_tokens() {
                match element {
                    rowan::NodeOrToken::Node(node)
                        if node.kind() == KotlinSyntaxKind::ANNOTATION_ARG_LIST =>
                    {
                        break;
                    }
                    rowan::NodeOrToken::Token(token) if token.kind() == KotlinSyntaxKind::IDENT => {
                        name_tokens.push(token);
                    }
                    _ => {}
                }
            }
            if name_tokens
                .first()
                .is_some_and(|token| token.text() == "file")
                && name_tokens
                    .last()
                    .is_some_and(|token| token.text() == "JvmName")
            {
                if let Some(value) = annotation
                    .descendants_with_tokens()
                    .filter_map(|element| element.into_token())
                    .find(|token| {
                        matches!(
                            token.kind(),
                            KotlinSyntaxKind::STRING_LITERAL | KotlinSyntaxKind::RAW_STRING_LITERAL
                        )
                    })
                {
                    return decode_kotlin_string_literal(value.text());
                }
            }
        }
    }
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => format!(
            "{}{}Kt",
            first.to_ascii_uppercase(),
            chars.collect::<String>()
        ),
        None => "Kt".to_string(),
    }
}

fn kotlin_has_file_annotation(root: &KotlinSyntaxNode, target: &str) -> bool {
    // File annotations are direct children of the file node; see kotlin_facade_name.
    root.children()
        .filter(|child| child.kind() == KotlinSyntaxKind::ANNOTATION)
        .chain(
            root.children()
                .filter(|child| child.kind() == KotlinSyntaxKind::MODIFIERS)
                .flat_map(|modifiers| modifiers.children())
                .filter(|annotation| annotation.kind() == KotlinSyntaxKind::ANNOTATION),
        )
        .any(|annotation| {
            let mut name_tokens = annotation
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| token.kind() == KotlinSyntaxKind::IDENT);
            name_tokens
                .next()
                .is_some_and(|token| token.text() == "file")
                && name_tokens.any(|token| token.text() == target)
        })
}

fn kotlin_multifile_part_name(stem: &str) -> String {
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => format!(
            "{}{}Kt",
            first.to_ascii_uppercase(),
            chars.collect::<String>()
        ),
        None => "Kt".to_string(),
    }
}

struct ScalaDeclaration {
    name: String,
    parent: PathBuf,
    is_object: bool,
    is_package_object: bool,
    is_package_part: bool,
}

fn scala_is_declaration_keyword(token: &str) -> bool {
    matches!(
        token,
        "class"
            | "trait"
            | "enum"
            | "object"
            | "def"
            | "val"
            | "var"
            | "type"
            | "given"
            | "extension"
            | "opaque"
            | "import"
            | "private"
            | "protected"
            | "abstract"
            | "final"
            | "sealed"
            | "case"
    )
}

fn scala_package_scope(tokens: &[(String, usize)]) -> (usize, usize) {
    let package_index = tokens.iter().position(|(token, _)| token == "package");
    let mut package_scope_depth = 0usize;
    let mut package_has_colon = false;
    if let Some(package_index) = package_index {
        for (token, _) in tokens.iter().skip(package_index + 1) {
            if scala_is_declaration_keyword(token) {
                break;
            }
            package_scope_depth += usize::from(token == "{");
            package_has_colon |= token == ":";
        }
    }
    let base_indent = if package_has_colon || package_scope_depth > 0 {
        tokens
            .iter()
            .find(|(token, _)| scala_is_declaration_keyword(token))
            .map_or(0, |(_, indent)| *indent)
    } else {
        0
    };
    (package_scope_depth, base_indent)
}

fn scala_declaration_names(source_text: &str) -> Vec<ScalaDeclaration> {
    let tokens = scala_tokens(&scala_sanitize(source_text));
    let mut declarations = Vec::new();
    let (_, base_indent) = scala_package_scope(&tokens);
    let mut current_package = PathBuf::new();
    let mut package_scopes: Vec<(usize, PathBuf)> = Vec::new();
    let mut colon_package_scopes: Vec<(usize, usize, PathBuf)> = Vec::new();
    let mut depth: usize = 0;
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].0 == "package" {
            let mut popped_colon_scope = false;
            while colon_package_scopes
                .last()
                .is_some_and(|(scope_indent, _, _)| *scope_indent >= tokens[index].1)
            {
                colon_package_scopes.pop();
                popped_colon_scope = true;
            }
            if let Some((_, _, parent)) = colon_package_scopes.last() {
                current_package.clone_from(parent);
            } else if popped_colon_scope {
                current_package = PathBuf::new();
            }
        } else if scala_is_declaration_keyword(&tokens[index].0) {
            let mut popped_colon_scope = false;
            while colon_package_scopes
                .last()
                .is_some_and(|(scope_indent, _, _)| *scope_indent >= tokens[index].1)
            {
                colon_package_scopes.pop();
                popped_colon_scope = true;
            }
            if popped_colon_scope {
                current_package = colon_package_scopes
                    .last()
                    .map_or_else(PathBuf::new, |(_, _, parent)| parent.clone());
            }
        }
        match tokens[index].0.as_str() {
            "{" => depth += 1,
            "}" => {
                depth = depth.saturating_sub(1);
                while package_scopes
                    .last()
                    .is_some_and(|(scope_depth, _)| *scope_depth > depth)
                {
                    current_package = package_scopes.pop().expect("checked above").1;
                }
            }
            "package"
                if tokens
                    .get(index + 1)
                    .is_some_and(|(token, _)| token == "object") =>
            {
                if depth
                    == package_scopes
                        .last()
                        .map_or(0, |(scope_depth, _)| *scope_depth)
                    && tokens[index].1 == base_indent
                {
                    if let Some((name, _)) = tokens.get(index + 2) {
                        declarations.push(ScalaDeclaration {
                            name: name.clone(),
                            parent: current_package.clone(),
                            is_object: false,
                            is_package_object: true,
                            is_package_part: false,
                        });
                    }
                }
                index += 2;
            }
            "package" => {
                let package_indent = tokens[index].1;
                let mut end = index + 1;
                let mut package_path = current_package.clone();
                while end < tokens.len() {
                    let token = &tokens[end].0;
                    if matches!(token.as_str(), "{" | ":" | ";" | "package")
                        || scala_is_declaration_keyword(token)
                    {
                        break;
                    }
                    package_path.push(token);
                    end += 1;
                }
                if tokens.get(end).is_some_and(|(token, _)| token == ":") {
                    package_path = colon_package_scopes
                        .last()
                        .map_or_else(PathBuf::new, |(_, _, parent)| parent.clone());
                    for (part, _) in tokens.iter().take(end).skip(index + 1) {
                        package_path.push(part);
                    }
                    let body_indent = tokens
                        .get(end + 1)
                        .map_or(package_indent + 1, |(_, indent)| *indent);
                    colon_package_scopes.push((package_indent, body_indent, package_path.clone()));
                    current_package = package_path;
                    index = end;
                } else if tokens.get(end).is_some_and(|(token, _)| token == "{") {
                    let previous_package = current_package.clone();
                    depth += 1;
                    package_scopes.push((depth, previous_package));
                    current_package = package_path;
                    index = end;
                } else {
                    current_package = package_path;
                    index = end.saturating_sub(1);
                }
            }
            "def" | "val" | "var" | "type" | "given" | "extension" | "opaque"
                if depth
                    == package_scopes
                        .last()
                        .map_or(0, |(scope_depth, _)| *scope_depth)
                    && tokens[index].1
                        == colon_package_scopes
                            .last()
                            .map_or(base_indent, |(_, body_indent, _)| *body_indent) =>
            {
                declarations.push(ScalaDeclaration {
                    name: String::new(),
                    parent: current_package.clone(),
                    is_object: false,
                    is_package_object: false,
                    is_package_part: true,
                });
            }
            "class" | "trait" | "enum" | "object"
                if depth
                    == package_scopes
                        .last()
                        .map_or(0, |(scope_depth, _)| *scope_depth)
                    && tokens[index].1
                        == colon_package_scopes
                            .last()
                            .map_or(base_indent, |(_, body_indent, _)| *body_indent) =>
            {
                if let Some((name, _)) = tokens.get(index + 1) {
                    if name != "{" && name != "}" {
                        declarations.push(ScalaDeclaration {
                            name: name.clone(),
                            parent: current_package.clone(),
                            is_object: tokens[index].0 == "object",
                            is_package_object: false,
                            is_package_part: false,
                        });
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    declarations.sort_by(|left, right| {
        left.parent
            .cmp(&right.parent)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.is_object.cmp(&right.is_object))
            .then_with(|| left.is_package_object.cmp(&right.is_package_object))
            .then_with(|| left.is_package_part.cmp(&right.is_package_part))
    });
    declarations.dedup_by(|left, right| {
        left.parent == right.parent
            && left.name == right.name
            && left.is_object == right.is_object
            && left.is_package_object == right.is_package_object
            && left.is_package_part == right.is_package_part
    });
    declarations
}

fn scala_has_top_level_package_definitions(source_text: &str) -> bool {
    let tokens = scala_tokens(&scala_sanitize(source_text));
    let (package_scope_depth, base_indent) = scala_package_scope(&tokens);
    let mut depth = 0usize;
    tokens.iter().any(|(token, indent)| {
        let matches = depth == package_scope_depth
            && *indent == base_indent
            && matches!(
                token.as_str(),
                "def" | "val" | "var" | "type" | "given" | "extension" | "opaque"
            );
        match token.as_str() {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            _ => {}
        }
        matches
    })
}

fn scala_package_path(source_text: &str) -> Option<PathBuf> {
    let tokens = scala_tokens(&scala_sanitize(source_text));
    let mut path = PathBuf::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].0 != "package" {
            if scala_is_declaration_keyword(&tokens[index].0) {
                break;
            }
            index += 1;
            continue;
        }
        index += 1;
        while index < tokens.len() {
            let token = &tokens[index].0;
            if matches!(token.as_str(), "{" | ":" | ";" | "package")
                || scala_is_declaration_keyword(token)
            {
                break;
            }
            path.push(token);
            index += 1;
        }
        while index < tokens.len() && matches!(tokens[index].0.as_str(), "{" | ":" | ";") {
            index += 1;
        }
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

fn scala_sanitize(source_text: &str) -> String {
    let mut sanitized = String::with_capacity(source_text.len());
    let mut characters = source_text.chars().peekable();
    let mut block_comment_depth = 0usize;
    while let Some(character) = characters.next() {
        if block_comment_depth > 0 {
            if character == '/' && characters.peek() == Some(&'*') {
                characters.next();
                block_comment_depth += 1;
            } else if character == '*' && characters.peek() == Some(&'/') {
                characters.next();
                block_comment_depth = block_comment_depth.saturating_sub(1);
            } else if character == '\n' {
                sanitized.push('\n');
            }
            continue;
        }
        if character == '/' && characters.peek() == Some(&'/') {
            characters.next();
            for character in characters.by_ref() {
                if character == '\n' {
                    sanitized.push('\n');
                    break;
                }
            }
            continue;
        }
        if character == '/' && characters.peek() == Some(&'*') {
            characters.next();
            block_comment_depth = 1;
            sanitized.push(' ');
            continue;
        }
        if character == '"' {
            if characters.peek() == Some(&'"') {
                let mut lookahead = characters.clone();
                lookahead.next();
                if lookahead.peek() == Some(&'"') {
                    characters.next();
                    characters.next();
                    let mut quote_count = 0;
                    for character in characters.by_ref() {
                        if character == '"' {
                            quote_count += 1;
                            if quote_count == 3 {
                                break;
                            }
                        } else {
                            quote_count = 0;
                            if character == '\n' {
                                sanitized.push('\n');
                            }
                        }
                    }
                    sanitized.push(' ');
                    continue;
                }
            }
            let mut escaped = false;
            for character in characters.by_ref() {
                if !escaped && character == '"' {
                    break;
                }
                escaped = !escaped && character == '\\';
                if character == '\n' {
                    sanitized.push('\n');
                }
            }
            sanitized.push(' ');
            continue;
        }
        if character == '\'' {
            let mut escaped = false;
            for character in characters.by_ref() {
                if !escaped && character == '\'' {
                    break;
                }
                escaped = !escaped && character == '\\';
            }
            sanitized.push(' ');
            continue;
        }
        sanitized.push(character);
    }
    sanitized
}

fn scala_tokens(source: &str) -> Vec<(String, usize)> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut line_indent = 0usize;
    let mut at_line_start = true;
    let mut token_indent = 0usize;
    for character in source.chars() {
        if at_line_start && matches!(character, ' ' | '\t') {
            line_indent += 1;
            continue;
        }
        if at_line_start {
            at_line_start = false;
            token_indent = line_indent;
        }
        if character == '\n' {
            if !token.is_empty() {
                tokens.push((std::mem::take(&mut token), token_indent));
            }
            line_indent = 0;
            at_line_start = true;
            continue;
        }
        if character.is_alphanumeric() || matches!(character, '_' | '$') {
            token.push(character);
        } else {
            if !token.is_empty() {
                tokens.push((std::mem::take(&mut token), token_indent));
            }
            if matches!(character, '{' | '}' | ':' | ';') {
                tokens.push((character.to_string(), line_indent));
            }
        }
    }
    if !token.is_empty() {
        tokens.push((token, token_indent));
    }
    tokens
}
