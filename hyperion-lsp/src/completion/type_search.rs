// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type-name candidate search for completion.
//!
//! One implementation of "which types can the user mean by this prefix",
//! shared by plain type completion and `new` completion. It searches the whole
//! index (no cap before ranking), drops declarations the caller could not name
//! (JDK internals, non-public classes of other packages), and orders what is
//! left so `java.lang` and `java.util` beat obscure packages.

use crate::completion::metadata::rank_group;
use hyperion_workspace::name_match::{match_name, NameMatch};
use hyperion_workspace::{ImportMap, Symbol, WorkspaceIndex};
use std::sync::Arc;
use tower_lsp::lsp_types::CompletionItemKind;

/// Package prefixes of JDK/library implementation classes that are never part
/// of an API a user should be offered.
const INTERNAL_PACKAGE_PREFIXES: &[&str] = &[
    "sun.",
    "jdk.internal.",
    "com.sun.org.apache.",
    "com.sun.java.",
    "com.sun.crypto.",
    "com.sun.jmx.",
    "com.sun.imageio.",
    "com.sun.beans.",
    "com.sun.naming.",
    "com.sun.jndi.",
    "com.sun.media.",
    "com.sun.rowset.",
    "com.sun.tools.",
    "com.sun.xml.internal.",
    "com.sun.security.sasl.",
    "com.sun.security.ntlm.",
    "com.sun.security.jgss.",
];

/// Packages whose classes are used all the time.
const COMMON_PACKAGES: &[&str] = &[
    "java.util",
    "java.util.function",
    "java.util.stream",
    "java.util.concurrent",
    "java.util.regex",
    "java.io",
    "java.nio.file",
    "java.time",
    "java.math",
    "java.net",
    "java.text",
];

/// The JDK classes nearly every program uses. They outrank the rest of their
/// package the way IntelliJ's usage statistics do, so `Arr` offers `ArrayList`
/// before `ArrayIndexOutOfBoundsException`.
const POPULAR_TYPES: &[&str] = &[
    "java.lang.Object",
    "java.lang.String",
    "java.lang.StringBuilder",
    "java.lang.Integer",
    "java.lang.Long",
    "java.lang.Double",
    "java.lang.Boolean",
    "java.lang.Math",
    "java.lang.System",
    "java.lang.Exception",
    "java.lang.RuntimeException",
    "java.lang.IllegalArgumentException",
    "java.lang.IllegalStateException",
    "java.lang.Override",
    "java.lang.Deprecated",
    "java.lang.FunctionalInterface",
    "java.lang.Runnable",
    "java.lang.Iterable",
    "java.lang.Comparable",
    "java.lang.Thread",
    "java.lang.Character",
    "java.lang.Number",
    "java.lang.Class",
    "java.lang.Enum",
    "java.lang.Record",
    "java.lang.CharSequence",
    "java.lang.Throwable",
    "java.lang.Void",
    "java.lang.SuppressWarnings",
    "java.lang.SafeVarargs",
    "java.lang.AutoCloseable",
    "java.util.List",
    "java.util.ArrayList",
    "java.util.LinkedList",
    "java.util.Map",
    "java.util.HashMap",
    "java.util.LinkedHashMap",
    "java.util.TreeMap",
    "java.util.Set",
    "java.util.HashSet",
    "java.util.LinkedHashSet",
    "java.util.TreeSet",
    "java.util.Collection",
    "java.util.Collections",
    "java.util.Arrays",
    "java.util.Optional",
    "java.util.Objects",
    "java.util.Iterator",
    "java.util.Comparator",
    "java.util.Queue",
    "java.util.Deque",
    "java.util.ArrayDeque",
    "java.util.UUID",
    "java.util.Date",
    "java.util.Scanner",
    "java.util.Random",
    "java.util.StringJoiner",
    "java.util.function.Function",
    "java.util.function.Supplier",
    "java.util.function.Consumer",
    "java.util.function.BiFunction",
    "java.util.function.Predicate",
    "java.util.stream.Stream",
    "java.util.stream.Collectors",
    "java.util.stream.IntStream",
    "java.util.concurrent.ConcurrentHashMap",
    "java.util.concurrent.CompletableFuture",
    "java.util.concurrent.ExecutorService",
    "java.util.concurrent.Executors",
    "java.util.concurrent.TimeUnit",
    "java.util.concurrent.atomic.AtomicInteger",
    "java.util.concurrent.atomic.AtomicLong",
    "java.util.regex.Pattern",
    "java.util.regex.Matcher",
    "java.io.File",
    "java.io.IOException",
    "java.io.InputStream",
    "java.io.OutputStream",
    "java.io.Serializable",
    "java.io.BufferedReader",
    "java.io.UncheckedIOException",
    "java.nio.file.Path",
    "java.nio.file.Paths",
    "java.nio.file.Files",
    "java.time.LocalDate",
    "java.time.LocalDateTime",
    "java.time.Instant",
    "java.time.Duration",
    "java.time.ZonedDateTime",
    "java.time.LocalTime",
    "java.math.BigDecimal",
    "java.math.BigInteger",
    "java.net.URI",
    "java.net.URL",
    "java.text.SimpleDateFormat",
    "java.text.DecimalFormat",
];

/// Whether a whole package is implementation detail rather than API.
pub fn is_internal_package(package: &str) -> bool {
    let dotted = format!("{package}.");
    INTERNAL_PACKAGE_PREFIXES
        .iter()
        .any(|prefix| dotted.starts_with(prefix))
        || dotted.contains(".internal.")
}

/// One type that matches the typed pattern.
#[derive(Debug, Clone)]
pub struct TypeCandidate {
    pub symbol: Arc<Symbol>,
    pub name_match: NameMatch,
    /// [`rank_group`] value describing how close the type is to the caller.
    pub group: u8,
    /// Enclosing type when this is a nested type.
    pub outer: Option<Arc<str>>,
    /// Tie-break among types of the same group: position in the curated
    /// popularity list, otherwise `100 + name length` (shorter is more basic).
    pub hint: u8,
}

impl TypeCandidate {
    /// Package of the outermost enclosing type (or of the type itself).
    pub fn package(&self) -> &str {
        package_of(&self.symbol.fqn)
    }

    /// Whether this declaration is an annotation type.
    pub fn is_annotation(&self) -> bool {
        is_annotation(&self.symbol)
    }
}

/// Package of a type FQN: the leading segments before the first one that
/// starts with an uppercase letter (so `java.util.Map.Entry` is in `java.util`).
pub fn package_of(fqn: &str) -> &str {
    let mut end: usize = 0;
    for segment in fqn.split('.') {
        if segment.chars().next().is_some_and(char::is_uppercase) {
            return end.checked_sub(1).map_or("", |cut| &fqn[..cut]);
        }
        end += segment.len() + 1;
    }
    fqn.rsplit_once('.').map_or("", |(package, _)| package)
}

/// Whether a symbol is an annotation declaration.
pub fn is_annotation(symbol: &Symbol) -> bool {
    symbol.parent_class.as_deref() == Some("java.lang.annotation.Annotation")
        || symbol
            .interfaces
            .iter()
            .any(|interface| interface.as_ref() == "java.lang.annotation.Annotation")
}

/// Search every indexed type matching `pattern`, best first, at most `limit`.
pub fn search_types(
    index: &WorkspaceIndex,
    imports: Option<&ImportMap>,
    pattern: &str,
    limit: usize,
) -> Vec<TypeCandidate> {
    let caller_package = imports.and_then(|imports| imports.current_package.as_deref());
    let mut candidates: Vec<TypeCandidate> = index
        .type_symbols_matching(pattern)
        .into_iter()
        .filter_map(|(symbol, external)| {
            let name_match = match_name(pattern, &symbol.simple_name)?;
            candidate(index, imports, caller_package, symbol, external, name_match)
        })
        .collect();
    candidates.sort_by(|a, b| {
        (a.name_match, a.group, a.hint, &a.symbol.fqn).cmp(&(
            b.name_match,
            b.group,
            b.hint,
            &b.symbol.fqn,
        ))
    });
    candidates.truncate(limit);
    // Scope election is the expensive check, so run it on the shortlist only.
    candidates.retain(|candidate| index.has_class(&candidate.symbol.fqn));
    candidates
}

fn candidate(
    index: &WorkspaceIndex,
    imports: Option<&ImportMap>,
    caller_package: Option<&str>,
    symbol: Arc<Symbol>,
    external: bool,
    name_match: NameMatch,
) -> Option<TypeCandidate> {
    let name = symbol.simple_name.as_ref();
    if name.is_empty()
        || name.contains('$')
        || name == "package-info"
        || name == "module-info"
        || name.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let fqn = symbol.fqn.as_ref();
    let internal = INTERNAL_PACKAGE_PREFIXES
        .iter()
        .any(|prefix| fqn.starts_with(prefix))
        || fqn.contains(".internal.");
    if internal {
        return None;
    }
    let package = package_of(fqn).to_string();
    // A nested type's FQN continues past `package.Name`; the enclosing type is
    // everything before its last segment. (Bytecode-indexed nested types carry
    // no reliable `container`, so the name structure is the source of truth.)
    let top_level_fqn = if package.is_empty() {
        name.to_string()
    } else {
        format!("{package}.{name}")
    };
    let outer: Option<Arc<str>> = (fqn != top_level_fqn)
        .then(|| fqn.rsplit_once('.').map(|(outer, _)| Arc::from(outer)))
        .flatten();
    let same_package = caller_package.unwrap_or("") == package;
    let is_public = symbol.modifiers.is_public || symbol.modifiers.is_protected && outer.is_some();
    let accessible = is_public || (same_package && !symbol.modifiers.is_private);
    if !accessible {
        return None;
    }
    if let Some(outer_fqn) = &outer {
        // A nested type is reachable by simple name only when it is static
        // (or implicitly static) and its outer type is itself exposed.
        let implicitly_static = symbol.kind != lsp_types::SymbolKind::CLASS
            || symbol.modifiers.is_static
            || index
                .get_symbol(outer_fqn)
                .is_some_and(|o| o.kind == lsp_types::SymbolKind::INTERFACE);
        if !implicitly_static {
            return None;
        }
    }

    let popularity = POPULAR_TYPES.iter().position(|popular| *popular == fqn);
    let group = if outer.is_some() {
        rank_group::NESTED_TYPE
    } else if is_near(imports, fqn, same_package) {
        // The same package and explicit imports.
        rank_group::NEAR_TYPE
    } else if popularity.is_some() {
        rank_group::POPULAR_TYPE
    } else if package == "java.lang" {
        rank_group::JAVA_LANG_TYPE
    } else if COMMON_PACKAGES.contains(&package.as_str()) || !external {
        // Common JDK packages, and the project's own types in other packages.
        rank_group::COMMON_TYPE
    } else {
        rank_group::OTHER_TYPE
    };
    let hint = popularity.map_or_else(
        || u8::try_from(100 + name.chars().count()).unwrap_or(u8::MAX),
        |index| u8::try_from(index).unwrap_or(99),
    );
    Some(TypeCandidate {
        symbol,
        name_match,
        group,
        outer,
        hint,
    })
}

fn is_near(imports: Option<&ImportMap>, fqn: &str, same_package: bool) -> bool {
    same_package
        || imports.is_some_and(|imports| {
            imports
                .explicit
                .get(fqn.rsplit('.').next().unwrap_or(fqn))
                .is_some_and(|imported| imported == fqn)
        })
}

/// LSP kind for a type declaration.
pub const fn completion_kind(symbol: &Symbol) -> CompletionItemKind {
    use lsp_types::SymbolKind;
    match symbol.kind {
        SymbolKind::INTERFACE => CompletionItemKind::INTERFACE,
        SymbolKind::ENUM => CompletionItemKind::ENUM,
        SymbolKind::STRUCT => CompletionItemKind::STRUCT,
        _ => CompletionItemKind::CLASS,
    }
}
