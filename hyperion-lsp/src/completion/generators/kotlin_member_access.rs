// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin member completion (`receiver.|`).
//!
//! What follows a dot in Kotlin is not what follows it in Java:
//!
//! * the receiver is any expression (`"x".`, `shapes.first().`, `foo(1).bar.`),
//! * members are properties and functions, never `getX()` (a Java getter is the
//!   property `x`, and Kotlin's own JVM-view accessors are not members at all),
//! * standard-library and project extension functions are part of the surface,
//! * a type name offers its companion, nested types, enum entries or statics.
//!
//! The receiver's type comes from Kotlin expression inference. Member rendering
//! of workspace types is shared with the Java generator and then re-shaped here.

use super::context::{CompletionGenerator, CompletionGeneratorContext};
use super::member_access::MemberAccessCompletionGenerator;
use crate::completion::metadata::{rank_group, CompletionItemMetadata};
use crate::navigation_util::extract_package_from_source;
use crate::position_utils::position_to_offset;
use hyperion_kotlin::stdlib_members::{self, EntryKind, StdlibEntry};
use hyperion_kotlin::SyntaxKind as Kt;
use hyperion_semantics::db::SemanticDatabase;
use hyperion_semantics::symbol::{ClassType, PrimitiveType};
use hyperion_semantics::{KotlinTypeInferenceEngine, SymbolResolver, Type};
use hyperion_workspace::{KotlinTypeAliasArguments, TypeNameResolution, WorkspaceIndex};
use rowan::{NodeOrToken, TextSize};
use std::collections::HashSet;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, Documentation,
    InsertTextFormat, MarkupContent, MarkupKind, SymbolKind,
};

/// Resolves Kotlin receivers before rendering members, extensions and statics.
pub struct KotlinMemberAccessCompletionGenerator;

/// Identifier the member being typed is replaced by so the buffer parses as a
/// complete member access.
const PLACEHOLDER: &str = "hyperionCompletionPlaceholder";

/// Object members Kotlin does not show on any receiver.
const HIDDEN_OBJECT_METHODS: &[&str] = &[
    "getClass",
    "wait",
    "notify",
    "notifyAll",
    "finalize",
    "clone",
];

/// Where the member being completed starts and ends.
struct MemberSite {
    /// Offset of the first character of the (possibly empty) typed prefix.
    prefix_start: usize,
    /// Offset just past any identifier characters after the cursor.
    replace_end: usize,
}

impl MemberSite {
    fn find(source: &str, offset: usize) -> Option<Self> {
        let before = source.get(..offset)?;
        let prefix_start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(offset, |(index, _)| index);
        if !source[..prefix_start].ends_with('.') {
            return None;
        }
        let replace_end = offset
            + source[offset..]
                .char_indices()
                .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
                .last()
                .map_or(0, |(index, c)| index + c.len_utf8());
        Some(Self {
            prefix_start,
            replace_end,
        })
    }
}

/// The receiver of the member access being completed.
enum Receiver {
    Node(hyperion_kotlin::SyntaxNode),
    Token(hyperion_kotlin::SyntaxToken),
}

impl KotlinMemberAccessCompletionGenerator {
    fn receiver_of(
        source: &str,
        site: &MemberSite,
    ) -> Option<(hyperion_kotlin::SyntaxNode, Receiver)> {
        let patched = format!(
            "{}{PLACEHOLDER}{}",
            &source[..site.prefix_start],
            &source[site.replace_end..]
        );
        let parsed = hyperion_kotlin::parse(&patched);
        let root = hyperion_kotlin::SyntaxNode::new_root(parsed.green_node);
        let token = root
            .token_at_offset(TextSize::from(u32::try_from(site.prefix_start).ok()?))
            .right_biased()
            .filter(|token| token.text() == PLACEHOLDER)?;
        let access = token.parent()?;
        if !matches!(access.kind(), Kt::MEMBER_ACCESS_EXPR | Kt::SAFE_ACCESS_EXPR) {
            return None;
        }
        let receiver = access
            .children_with_tokens()
            .find_map(|element| match element {
                NodeOrToken::Node(node) => Some(Receiver::Node(node)),
                NodeOrToken::Token(token)
                    if !token.kind().is_trivia()
                        && !matches!(token.kind(), Kt::DOT | Kt::SAFE_CALL) =>
                {
                    Some(Receiver::Token(token))
                }
                NodeOrToken::Token(_) => None,
            })?;
        Some((root, receiver))
    }
}

/// Kotlin's name for a type as the index or inference spells it.
fn kotlin_builtin_name(class: &ClassType) -> Option<String> {
    let package = class.package.as_deref();
    let name = class.name.as_str();
    match (package, name) {
        (Some(p), _) if p == "kotlin" || p.starts_with("kotlin.") => {
            Some(name.trim_end_matches('?').to_string())
        }
        (Some("java.lang"), "String") => Some("String".into()),
        // Inference resolves a bare `List`/`Map`/... through the JDK index; in
        // Kotlin source that spelling is the read-only Kotlin collection.
        (Some("java.util"), "List") => Some("List".into()),
        (Some("java.util"), "Set") => Some("Set".into()),
        (Some("java.util"), "Map") => Some("Map".into()),
        (Some("java.util"), "Collection") => Some("Collection".into()),
        (Some("java.util"), "Iterator") => Some("Iterator".into()),
        (Some("java.lang"), "Object") => Some("Any".into()),
        (Some("java.lang"), "CharSequence") => Some("CharSequence".into()),
        (Some("java.lang"), "Number") => Some("Number".into()),
        (Some("java.lang"), "Comparable") => Some("Comparable".into()),
        (Some("java.lang"), "Iterable") => Some("Iterable".into()),
        (Some("java.lang"), "Integer") => Some("Int".into()),
        (Some("java.lang"), "Long") => Some("Long".into()),
        (Some("java.lang"), "Double") => Some("Double".into()),
        (Some("java.lang"), "Float") => Some("Float".into()),
        (Some("java.lang"), "Short") => Some("Short".into()),
        (Some("java.lang"), "Byte") => Some("Byte".into()),
        (Some("java.lang"), "Boolean") => Some("Boolean".into()),
        (Some("java.lang"), "Character") => Some("Char".into()),
        (
            None,
            "String" | "Int" | "Long" | "Double" | "Float" | "Short" | "Byte" | "Boolean" | "Char"
            | "Any" | "List" | "MutableList" | "Set" | "MutableSet" | "Map" | "MutableMap"
            | "Collection" | "MutableCollection" | "Iterable" | "Sequence" | "Array"
            | "CharSequence" | "Number",
        ) => Some(name.to_string()),
        _ => None,
    }
}

/// One typealias argument, from its Kotlin spelling, in the form
/// `instance_members` takes: simple class names, with the argument's own
/// arguments kept, so `kotlin.collections.List<kotlin.Int>?` is
/// `List<Int>`. A top-level variance is dropped. When a nested argument has
/// no such spelling (a projection or a function type), the argument is its
/// class's bare name, `List`; a star projection or a function type has none.
fn alias_argument_spelling(spelling: &str) -> String {
    let spelling = spelling.trim();
    let bare = spelling
        .strip_prefix("out ")
        .or_else(|| spelling.strip_prefix("in "))
        .unwrap_or(spelling);
    simple_name_spelling(bare).unwrap_or_else(|| alias_argument_name(spelling))
}

/// `spelling`, a Kotlin type, with each class named by its simple name and
/// nullability dropped: `kotlin.collections.Map<kotlin.String, kotlin.Int?>`
/// is `Map<String, Int>`. None for a projection, a function type, or a type
/// with one among its arguments.
fn simple_name_spelling(spelling: &str) -> Option<String> {
    let spelling = spelling.trim().trim_end_matches('?').trim_end();
    let (class, arguments) = match spelling.find('<') {
        Some(open) => (
            &spelling[..open],
            Some(spelling[open + 1..].strip_suffix('>')?),
        ),
        None => (spelling, None),
    };
    let class = class.trim();
    if class.is_empty()
        || !class
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        return None;
    }
    let simple = class.rsplit('.').next().unwrap_or(class);
    let Some(arguments) = arguments else {
        return Some(simple.to_string());
    };
    let arguments = stdlib_members::split_top_level(arguments)
        .into_iter()
        .map(simple_name_spelling)
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{simple}<{}>", arguments.join(", ")))
}

/// One written type argument of a receiver type, in the form
/// `instance_members` takes: the class's name with its own arguments, as
/// written (`List<Int>` in `StringMap<List<Int>>`), or its bare
/// name when one of those has no spelling (`List<*>` is `List`). An `out`
/// projection is spelled as its bound, what the receiver gives out. Empty
/// for a type that has none.
fn written_argument_spelling(argument: &Type) -> String {
    match argument {
        Type::Class(inner) => spelled_with_arguments(&inner.name, &inner.type_arguments)
            .unwrap_or_else(|| inner.name.clone()),
        Type::Primitive(primitive) => kotlin_name_of_primitive(*primitive).to_string(),
        Type::Wildcard {
            upper_bound: Some(bound),
            lower_bound: None,
        } => written_argument_spelling(bound),
        _ => String::new(),
    }
}

/// `name` followed by `arguments`, each spelled by its class's simple name
/// and its own arguments, nullability dropped. None when one of them, at any
/// depth, is a projection or a function type.
fn spelled_with_arguments(name: &str, arguments: &[Type]) -> Option<String> {
    if arguments.is_empty() {
        return Some(name.to_string());
    }
    let arguments = arguments
        .iter()
        .map(|argument| match argument {
            Type::Class(inner) => {
                let name = inner.name.trim_end_matches('?');
                (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                    .then(|| spelled_with_arguments(name, &inner.type_arguments))
                    .flatten()
            }
            Type::Primitive(primitive) => Some(kotlin_name_of_primitive(*primitive).to_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{name}<{}>", arguments.join(", ")))
}

/// One typealias argument, from its Kotlin spelling, as the simple name of
/// its class: `kotlin.collections.List<kotlin.Int>?` is `List`. A star
/// projection or a function type has none.
fn alias_argument_name(spelling: &str) -> String {
    let spelling = spelling.trim();
    let spelling = spelling
        .strip_prefix("out ")
        .or_else(|| spelling.strip_prefix("in "))
        .unwrap_or(spelling)
        .trim();
    let class = spelling
        .split('<')
        .next()
        .unwrap_or(spelling)
        .trim_end_matches('?')
        .trim();
    if class.is_empty()
        || !class
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        return String::new();
    }
    class.rsplit('.').next().unwrap_or(class).to_string()
}

/// Kotlin's mapped view of a Java type from the JDK collections/lang packages.
fn mapped_java_name(fqn: &str) -> Option<&'static str> {
    Some(match fqn {
        "java.lang.Object" => "Any",
        "java.lang.String" => "String",
        "java.lang.CharSequence" => "CharSequence",
        "java.lang.Comparable" => "Comparable",
        "java.lang.Number" => "Number",
        "java.lang.Iterable" => "Iterable",
        "java.util.Collection" => "MutableCollection",
        "java.util.List" => "MutableList",
        "java.util.Set" => "MutableSet",
        "java.util.Map" => "MutableMap",
        "java.util.Iterator" => "MutableIterator",
        "java.util.ListIterator" => "MutableListIterator",
        "java.util.Map.Entry" => "MutableEntry",
        _ => return None,
    })
}

const fn kotlin_name_of_primitive(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Int => "Int",
        PrimitiveType::Long => "Long",
        PrimitiveType::Short => "Short",
        PrimitiveType::Byte => "Byte",
        PrimitiveType::Float => "Float",
        PrimitiveType::Double => "Double",
        PrimitiveType::Boolean => "Boolean",
        PrimitiveType::Char => "Char",
    }
}

/// Java spelling -> Kotlin spelling for display (`int` -> `Int`, `void` -> `Unit`).
fn kotlin_display_type(java: &str) -> String {
    let shortened = super::member_access::shorten_type(java);
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        out.push_str(match word.as_str() {
            "int" => "Int",
            "long" => "Long",
            "short" => "Short",
            "byte" => "Byte",
            "float" => "Float",
            "double" => "Double",
            "boolean" => "Boolean",
            "char" => "Char",
            "void" => "Unit",
            "Object" => "Any",
            other => other,
        });
        word.clear();
    };
    for c in shortened.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(if c == '?' { '*' } else { c });
        }
    }
    flush(&mut word, &mut out);
    out
}

/// All supertypes of `fqn` in the workspace hierarchy, as simple names and as
/// the Kotlin names of the JDK types they map to.
fn hierarchy_names(
    index: &WorkspaceIndex,
    fqn: &str,
    simple_names: &mut Vec<String>,
    mapped: &mut Vec<&'static str>,
) {
    let mut queue = vec![fqn.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(next) = queue.pop() {
        if !seen.insert(next.clone()) || seen.len() > 64 {
            continue;
        }
        if let Some(mapped_name) = mapped_java_name(&next) {
            mapped.push(mapped_name);
        }
        simple_names.push(next.rsplit('.').next().unwrap_or(&next).to_string());
        queue.extend(index.direct_supertypes(&next));
    }
}

fn plain_item(label: &str, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        insert_text: Some(label.to_string()),
        insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
        ..CompletionItem::default()
    }
}

/// Completion item for one stdlib member or extension.
fn stdlib_item(entry: &StdlibEntry, receiver_type: &str, snippets: bool) -> CompletionItem {
    let is_function = entry.kind == EntryKind::Function;
    let parameters = entry.parameters;
    let rendered_parameters = if parameters.chars().count() > 48 {
        format!("({}…)", parameters.chars().take(47).collect::<String>())
    } else {
        format!("({parameters})")
    };
    let return_type = entry.return_type;
    let (insert_text, format) = if !is_function {
        (entry.name.to_string(), InsertTextFormat::PLAIN_TEXT)
    } else if parameters.is_empty() {
        (format!("{}()", entry.name), InsertTextFormat::PLAIN_TEXT)
    } else if snippets && entry.takes_trailing_lambda() {
        (
            format!("{} {{ $0 }}", entry.name),
            InsertTextFormat::SNIPPET,
        )
    } else if snippets {
        let first = parameters
            .split(',')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("arg")
            .trim()
            .rsplit(' ')
            .next()
            .unwrap_or("arg")
            .replace(['$', '}', '\\'], "");
        (
            format!("{}(${{1:{first}}})$0", entry.name),
            InsertTextFormat::SNIPPET,
        )
    } else {
        (format!("{}()", entry.name), InsertTextFormat::PLAIN_TEXT)
    };

    let detail = if entry.is_member {
        if is_function {
            format!("fun {}({parameters}): {return_type}", entry.name)
        } else {
            format!("val {}: {return_type}", entry.name)
        }
    } else if is_function {
        format!(
            "fun {} {}.{}({parameters}): {return_type}",
            entry.type_parameters, entry.receiver, entry.name
        )
        .replace("fun  ", "fun ")
    } else {
        format!("val {}.{}: {return_type}", entry.receiver, entry.name)
    };

    let group = if entry.is_member && stdlib_members::base_name(entry.receiver) == receiver_type {
        rank_group::OWN_MEMBER
    } else {
        rank_group::INHERITED_MEMBER
    };
    let semantic_id = format!(
        "kotlin-stdlib:{}.{}({parameters})",
        stdlib_members::base_name(entry.receiver),
        entry.name
    );
    let metadata = CompletionItemMetadata::new(entry.name)
        .with_group(group)
        .with_declared_in(stdlib_members::base_name(entry.receiver).to_string())
        .with_semantic_id(semantic_id)
        .with_item_type(stdlib_members::base_name(return_type).to_string());

    CompletionItem {
        label: entry.name.to_string(),
        label_details: Some(CompletionItemLabelDetails {
            detail: is_function.then_some(rendered_parameters),
            description: (!return_type.is_empty()).then(|| return_type.to_string()),
        }),
        kind: Some(if is_function {
            CompletionItemKind::METHOD
        } else {
            CompletionItemKind::PROPERTY
        }),
        detail: Some(detail),
        sort_text: Some(format!(
            "{}_{}",
            if entry.is_member { 1 } else { 2 },
            entry.name
        )),
        insert_text: Some(insert_text),
        insert_text_format: Some(format),
        data: metadata.to_value(),
        ..CompletionItem::default()
    }
}

/// Re-shape items rendered by the Java member generator for Kotlin: drop what
/// Kotlin does not expose, turn Java getters into properties.
fn kotlinize(items: Vec<CompletionItem>, index: &WorkspaceIndex) -> Vec<CompletionItem> {
    struct Getter {
        property: String,
        item: CompletionItem,
    }

    let mut kept: Vec<CompletionItem> = Vec::new();
    let mut getters: Vec<Getter> = Vec::new();
    let mut setters: Vec<(String, CompletionItem)> = Vec::new();
    let mut property_names: HashSet<String> = HashSet::new();

    for mut item in items {
        let metadata = CompletionItemMetadata::from_item(&item);
        let symbol = metadata
            .as_ref()
            .and_then(|metadata| metadata.fqn.as_deref())
            .and_then(|fqn| index.get_symbol(fqn));
        let Some(symbol) = symbol else {
            kept.push(item);
            continue;
        };

        // The JVM view of a Kotlin declaration is not a Kotlin member.
        if symbol
            .kotlin
            .as_ref()
            .is_some_and(|kotlin| kotlin.jvm_origin.is_some())
        {
            continue;
        }
        let declared_in_object = metadata
            .as_ref()
            .and_then(|metadata| metadata.declared_in.as_deref())
            == Some("java.lang.Object");
        if declared_in_object && HIDDEN_OBJECT_METHODS.contains(&item.label.as_str()) {
            continue;
        }

        let is_java_method = symbol.kind == SymbolKind::METHOD && symbol.kotlin.is_none();
        if is_java_method && !symbol.modifiers.is_static {
            match hyperion_types::jvm_names::property_from_accessor(&symbol.simple_name) {
                Some((property, hyperion_types::jvm_names::AccessorKind::Getter))
                    if symbol.parameters.is_empty()
                        && symbol.return_type.as_deref() != Some("void") =>
                {
                    getters.push(Getter { property, item });
                    continue;
                }
                Some((property, hyperion_types::jvm_names::AccessorKind::BooleanGetter))
                    if symbol.parameters.is_empty() =>
                {
                    getters.push(Getter { property, item });
                    continue;
                }
                Some((property, hyperion_types::jvm_names::AccessorKind::Setter))
                    if symbol.parameters.len() == 1 =>
                {
                    setters.push((property, item));
                    continue;
                }
                _ => {}
            }
        }

        // Kotlin spellings in the row (`int` -> `Int`, `void` -> `Unit`).
        if let Some(details) = item.label_details.as_mut() {
            details.description = details.description.take().map(|d| kotlin_display_type(&d));
            details.detail = details.detail.take().map(|d| kotlin_display_type(&d));
        }
        if matches!(item.kind, Some(CompletionItemKind::FIELD)) && symbol.kotlin.is_none() {
            item.kind = Some(CompletionItemKind::PROPERTY);
        }
        property_names.insert(item.label.clone());
        kept.push(item);
    }

    for Getter { property, item } in getters {
        if property_names.contains(&property) {
            continue;
        }
        let metadata = CompletionItemMetadata::from_item(&item);
        let return_type = item
            .label_details
            .as_ref()
            .and_then(|details| details.description.clone())
            .map(|description| kotlin_display_type(&description));
        let mut property_metadata = CompletionItemMetadata::new(&property);
        if let Some(existing) = metadata {
            if let Some(fqn) = existing.fqn {
                property_metadata = property_metadata.with_fqn(fqn);
            }
            if let Some(owner) = existing.declared_in {
                property_metadata = property_metadata.with_declared_in(owner);
            }
            if let Some(group) = existing.group {
                property_metadata = property_metadata.with_group(group);
            }
        }
        if let Some(return_type) = &return_type {
            property_metadata = property_metadata.with_item_type(return_type.clone());
        }
        property_names.insert(property.clone());
        kept.push(CompletionItem {
            label: property.clone(),
            label_details: Some(CompletionItemLabelDetails {
                detail: None,
                description: return_type.clone(),
            }),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: return_type.map(|ty| format!("{property}: {ty}")),
            documentation: item.documentation.clone(),
            sort_text: Some(format!("1_{property}")),
            insert_text: Some(property),
            insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
            data: property_metadata.to_value(),
            tags: item.tags.clone(),
            deprecated: item.deprecated,
            ..CompletionItem::default()
        });
    }
    // A setter is the property's assignment; without a getter it stays a call.
    for (property, item) in setters {
        if !property_names.contains(&property) {
            kept.push(item);
        }
    }
    kept
}

/// Same name, same parameters, same kind: one row (`equals` from `Any` and from
/// the workspace, `length` from `String` and `CharSequence`).
fn dedupe(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    let mut seen: HashSet<(String, Option<String>, Option<String>)> = HashSet::new();
    items
        .into_iter()
        .filter(|item| {
            seen.insert((
                item.label.clone(),
                item.kind.map(|kind| format!("{kind:?}")),
                item.label_details
                    .as_ref()
                    .and_then(|details| details.detail.clone()),
            ))
        })
        .collect()
}

impl CompletionGenerator for KotlinMemberAccessCompletionGenerator {
    fn generate(
        &self,
        context: &CompletionGeneratorContext<'_>,
    ) -> Result<Vec<CompletionItem>, String> {
        let offset = position_to_offset(context.source_text, context.position)?;
        let site = MemberSite::find(context.source_text, offset)
            .ok_or_else(|| "Cursor is not directly after a member-access dot".to_string())?;
        let (_patched_root, receiver) = Self::receiver_of(context.source_text, &site)
            .ok_or_else(|| "Could not parse the receiver expression".to_string())?;

        let symbol_table = context.snapshot.file_symbol_table(context.file_id);
        let mut resolver = SymbolResolver::with_cached_table(symbol_table.clone());
        let engine =
            KotlinTypeInferenceEngine::new(context.workspace_index.clone(), context.file_id);
        let caller_package = extract_package_from_source(context.source_text);

        // A receiver that names a type (not a value in scope) is a static-like
        // access: companion, nested types, enum entries, statics.
        if let Some(type_fqn) = self.receiver_type_name(context, &receiver, &site, &symbol_table) {
            return self
                .static_members(context, &type_fqn, caller_package.as_deref())
                .map(dedupe);
        }

        let receiver_type = match &receiver {
            Receiver::Node(node) => engine.infer_expression(node, &mut resolver),
            Receiver::Token(token) if token.kind() == Kt::THIS_KW => {
                return Err("`this.` completion is served by the enclosing class".to_string());
            }
            Receiver::Token(token) => engine.infer_token_expression(token, &mut resolver),
        };
        if matches!(receiver_type, Type::Unknown) {
            return Err("Could not infer the Kotlin receiver type".to_string());
        }
        self.instance_members(context, &receiver_type, caller_package.as_deref())
            .map(dedupe)
    }
}

impl KotlinMemberAccessCompletionGenerator {
    /// The FQN of the type a receiver *names*, when it names one.
    fn receiver_type_name(
        &self,
        context: &CompletionGeneratorContext<'_>,
        receiver: &Receiver,
        site: &MemberSite,
        symbol_table: &Arc<hyperion_semantics::symbol::SymbolTable>,
    ) -> Option<String> {
        let text = match receiver {
            Receiver::Token(token) if token.kind() == Kt::IDENT => token.text().to_string(),
            Receiver::Node(node) if matches!(node.kind(), Kt::MEMBER_ACCESS_EXPR) => {
                let compact: String = node.text().to_string().split_whitespace().collect();
                if compact.split('.').all(|part| {
                    !part.is_empty() && part.chars().all(|c| c.is_alphanumeric() || c == '_')
                }) {
                    compact
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        let head = text.split('.').next()?;
        if !head.chars().next().is_some_and(char::is_uppercase) && !text.contains('.') {
            return None;
        }
        // A value in scope wins over a type of the same name.
        let offset = u32::try_from(site.prefix_start).ok()?;
        let scope = symbol_table.find_scope_at_offset(offset)?;
        let is_value = symbol_table
            .lookup_symbol(head, scope)
            .and_then(|id| symbol_table.get_symbol(id))
            .is_some_and(|symbol| {
                matches!(
                    symbol.kind,
                    hyperion_semantics::symbol::SymbolKind::Field { .. }
                        | hyperion_semantics::symbol::SymbolKind::LocalVariable { .. }
                        | hyperion_semantics::symbol::SymbolKind::Parameter { .. }
                )
            });
        if is_value {
            return None;
        }
        match context
            .workspace_index
            .resolve_type_name(context.file_id, &[], &text)
        {
            TypeNameResolution::Exact(fqn) => Some(fqn),
            _ => None,
        }
    }

    /// `Type.|`: companion members, nested types, enum entries, Java statics.
    fn static_members(
        &self,
        context: &CompletionGeneratorContext<'_>,
        fqn: &str,
        caller_package: Option<&str>,
    ) -> Result<Vec<CompletionItem>, String> {
        let index = &context.workspace_index;
        let generator = MemberAccessCompletionGenerator::new();
        let mut items = Vec::new();

        // Kotlin `object` / `companion object`: their members read as statics.
        let companion = format!("{fqn}.Companion");
        for container in [companion.as_str(), fqn] {
            let is_object = index
                .get_symbol(container)
                .is_some_and(|symbol| symbol.kind == SymbolKind::OBJECT);
            if is_object {
                if let Ok(members) =
                    generator.completions_for_fqn(context, container, &[], false, caller_package)
                {
                    items.extend(kotlinize(members, index));
                }
            }
        }

        // Enum entries (Kotlin and Java) and everything static.
        if let Ok(statics) = generator.completions_for_fqn(context, fqn, &[], true, caller_package)
        {
            items.extend(kotlinize(statics, index));
        }

        if items.is_empty() {
            return Err(format!("No static members for '{fqn}'"));
        }
        Ok(items)
    }

    /// Members and extensions of a value of `receiver_type`.
    fn instance_members(
        &self,
        context: &CompletionGeneratorContext<'_>,
        receiver_type: &Type,
        caller_package: Option<&str>,
    ) -> Result<Vec<CompletionItem>, String> {
        let index = &context.workspace_index;

        let (fqn, class, builtin_name, arguments): (
            Option<String>,
            Option<&ClassType>,
            Option<String>,
            Vec<String>,
        ) = match receiver_type {
            Type::Primitive(primitive) => (
                None,
                None,
                Some(kotlin_name_of_primitive(*primitive).to_string()),
                Vec::new(),
            ),
            Type::Class(class) => {
                let written: Vec<String> = class
                    .type_arguments
                    .iter()
                    .map(written_argument_spelling)
                    .collect();
                let arguments = match Self::alias_type_arguments(context, class, &written) {
                    KotlinTypeAliasArguments::NotAnAlias => written,
                    KotlinTypeAliasArguments::Expanded(arguments) => arguments,
                    KotlinTypeAliasArguments::Unmatched => {
                        return Err(format!(
                            "The type arguments of `{}` do not match its typealias",
                            class.name
                        ));
                    }
                };
                let builtin = kotlin_builtin_name(class);
                (
                    Some(self.resolve_class_fqn(context, class)),
                    Some(class),
                    builtin,
                    arguments,
                )
            }
            Type::Array { .. } => (None, None, Some("Array".to_string()), Vec::new()),
            _ => return Err("Receiver type has no members".to_string()),
        };
        let _ = class;

        let mut items: Vec<CompletionItem> = Vec::new();
        let mut extra_names: Vec<&'static str> = Vec::new();
        let mut supertype_simple_names: Vec<String> = Vec::new();

        // Workspace members for anything that is not a Kotlin builtin.
        if builtin_name.is_none() {
            if let Some(fqn) = &fqn {
                hierarchy_names(index, fqn, &mut supertype_simple_names, &mut extra_names);
                let generator = MemberAccessCompletionGenerator::new();
                if let Ok(members) =
                    generator.completions_for_fqn(context, fqn, &arguments, false, caller_package)
                {
                    items.extend(kotlinize(members, index));
                }
            }
        }

        // Standard library: members and extensions of the (mapped) Kotlin type.
        let table_name = builtin_name
            .clone()
            .or_else(|| {
                fqn.as_deref()
                    .and_then(|fqn| mapped_java_name(fqn).map(str::to_string))
            })
            .unwrap_or_else(|| {
                fqn.as_deref()
                    .and_then(|fqn| fqn.rsplit('.').next())
                    .unwrap_or("Any")
                    .to_string()
            });
        let extra: Vec<&str> = extra_names.clone();
        let seen: HashSet<String> = items.iter().map(|item| item.label.clone()).collect();
        for entry in stdlib_members::entries_for(&table_name, &arguments, &extra) {
            // A Java class already offers its own members; do not repeat them
            // as stdlib members (`size`, `isEmpty`) unless the workspace lacks them.
            if entry.is_member && seen.contains(entry.name) && builtin_name.is_none() {
                continue;
            }
            items.push(stdlib_item(entry, &table_name, context.snippets));
        }

        // Project extension functions applicable to the type or a supertype.
        let mut receiver_names: Vec<String> = vec![table_name.clone()];
        receiver_names.extend(supertype_simple_names);
        receiver_names.extend(extra_names.iter().map(|name| (*name).to_string()));
        receiver_names.extend(
            stdlib_members::supertype_closure(&table_name)
                .into_iter()
                .map(str::to_string),
        );
        receiver_names.sort();
        receiver_names.dedup();
        items.extend(self.project_extensions(context, &receiver_names, caller_package));

        if items.is_empty() {
            return Err("No members found".to_string());
        }
        Ok(items)
    }

    fn resolve_class_fqn(
        &self,
        context: &CompletionGeneratorContext<'_>,
        class: &ClassType,
    ) -> String {
        let qualified = class.qualified_name().into_owned();
        if context.workspace_index.has_class(&qualified) {
            return qualified;
        }
        match context
            .workspace_index
            .resolve_type_name(context.file_id, &[], &class.name)
        {
            TypeNameResolution::Exact(fqn) => fqn,
            _ => qualified,
        }
    }

    /// The type arguments a typealias named by `class`, written with the
    /// arguments `written`, gives the class it expands to, in the form
    /// `instance_members` takes them: `val roster: Names` under
    /// `typealias Names = java.util.ArrayList<String>` is an `ArrayList` of
    /// `String`, and `StringMap<Int>` under `typealias StringMap<V> =
    /// java.util.HashMap<String, V>` a `HashMap` of `String` to `Int`.
    ///
    /// `class` is resolved the way [`Self::resolve_class_fqn`] resolves it;
    /// a class is not an alias, and `written` applies to it as it is.
    fn alias_type_arguments(
        context: &CompletionGeneratorContext<'_>,
        class: &ClassType,
        written: &[String],
    ) -> KotlinTypeAliasArguments {
        if context.workspace_index.has_class(&class.qualified_name()) {
            return KotlinTypeAliasArguments::NotAnAlias;
        }
        match context.workspace_index.resolve_kotlin_type_alias_arguments(
            context.file_id,
            &[],
            &class.name,
            written,
        ) {
            KotlinTypeAliasArguments::Expanded(arguments) => KotlinTypeAliasArguments::Expanded(
                arguments
                    .iter()
                    .map(|spelling| alias_argument_spelling(spelling))
                    .collect(),
            ),
            other => other,
        }
    }

    /// Extension functions/properties declared in the project for any of
    /// `receiver_names`, with the import they need.
    fn project_extensions(
        &self,
        context: &CompletionGeneratorContext<'_>,
        receiver_names: &[String],
        caller_package: Option<&str>,
    ) -> Vec<CompletionItem> {
        let index = &context.workspace_index;
        let imports = index.get_imports(context.file_id);
        let import_manager = crate::import_manager::ImportManager::new(Arc::clone(index));
        let mut items = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for receiver in receiver_names {
            for symbol in index.find_extensions_for_type(receiver) {
                if symbol.modifiers.is_private
                    || symbol
                        .kotlin
                        .as_ref()
                        .is_some_and(|kotlin| kotlin.jvm_origin.is_some())
                {
                    continue;
                }
                if !seen.insert(format!("{}#{}", symbol.fqn, symbol.parameters.join(","))) {
                    continue;
                }
                let is_property = symbol.kind == SymbolKind::PROPERTY;
                let parameters: Vec<String> = symbol
                    .parameters
                    .iter()
                    .map(|parameter| {
                        let mut parts = parameter.rsplitn(2, ' ');
                        let name = parts.next().unwrap_or("");
                        match parts.next() {
                            Some(ty) => format!("{name}: {ty}"),
                            None => name.to_string(),
                        }
                    })
                    .collect();
                let rendered = parameters.join(", ");
                let return_type = symbol
                    .return_type
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                let insert = if is_property {
                    symbol.simple_name.to_string()
                } else if parameters.is_empty() {
                    format!("{}()", symbol.simple_name)
                } else {
                    format!("{}($0)", symbol.simple_name)
                };
                let owner_package = symbol
                    .fqn
                    .rsplit_once('.')
                    .map(|(package, _)| package.to_string());
                let visible = owner_package.as_deref() == caller_package
                    || imports.as_ref().is_some_and(|imports| {
                        imports
                            .explicit
                            .values()
                            .any(|fqn| fqn.as_str() == &*symbol.fqn)
                            || owner_package
                                .as_ref()
                                .is_some_and(|package| imports.wildcards.contains(package))
                    });
                let additional_text_edits = (!visible).then(|| {
                    vec![import_manager
                        .generate_kotlin_import_edit(&symbol.fqn, context.source_text)]
                });
                let metadata = CompletionItemMetadata::new(&symbol.simple_name)
                    .with_fqn(symbol.fqn.to_string())
                    .with_group(rank_group::INHERITED_MEMBER)
                    .with_declared_in(receiver.clone())
                    .with_semantic_id(format!(
                        "kotlin-ext:{}#{}",
                        symbol.fqn,
                        symbol.parameters.join(",")
                    ));
                items.push(CompletionItem {
                    label: symbol.simple_name.to_string(),
                    label_details: Some(CompletionItemLabelDetails {
                        detail: (!is_property).then(|| format!("({rendered})")),
                        description: (!return_type.is_empty()).then(|| return_type.clone()),
                    }),
                    kind: Some(if is_property {
                        CompletionItemKind::PROPERTY
                    } else {
                        CompletionItemKind::METHOD
                    }),
                    detail: Some(format!(
                        "{} {}.{}({rendered}): {return_type}",
                        if is_property { "val" } else { "fun" },
                        symbol.receiver_type.as_deref().unwrap_or(receiver),
                        symbol.simple_name
                    )),
                    documentation: symbol.documentation.as_ref().map(|doc| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: doc.to_string(),
                        })
                    }),
                    sort_text: Some(format!("2_{}", symbol.simple_name)),
                    insert_text: Some(insert),
                    insert_text_format: Some(if context.snippets && !is_property {
                        InsertTextFormat::SNIPPET
                    } else {
                        InsertTextFormat::PLAIN_TEXT
                    }),
                    additional_text_edits,
                    data: metadata.to_value(),
                    ..CompletionItem::default()
                });
            }
        }
        let _ = plain_item;
        items
    }
}
