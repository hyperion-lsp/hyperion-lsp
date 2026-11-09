// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_workspace::{
    extract_jar_symbols, FileId, KotlinTypeAlias, KotlinTypeAliasArguments, Modifiers, Symbol,
    TextRange, TypeNameResolution, WorkspaceIndex,
};
use lsp_types::SymbolKind;
use std::path::PathBuf;
use tempfile::TempDir;
use walkdir::WalkDir;

fn find_kotlin_stdlib() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let roots = [
        home.join(".m2/repository/org/jetbrains/kotlin/kotlin-stdlib"),
        home.join(".gradle/caches"),
        home.join(".sdkman/candidates/gradle"),
    ];
    roots.into_iter().find_map(|root| {
        if !root.exists() {
            return None;
        }
        WalkDir::new(root)
            .max_depth(8)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .find(|entry| {
                let name = entry.file_name().to_string_lossy();
                entry.file_type().is_file()
                    && name.starts_with("kotlin-stdlib-")
                    && name.ends_with(".jar")
                    && !name.contains("-sources")
                    && !name.contains("-common")
                    && !name.contains("-jdk7")
                    && !name.contains("-jdk8")
            })
            .map(|entry| entry.into_path())
    })
}

/// The major version in the jar manifest's `Kotlin-Version` attribute, such as 1 for "1.8".
fn kotlin_major_version(jar: &std::path::Path) -> Option<u32> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(jar).ok()?).ok()?;
    let mut manifest = String::new();
    archive
        .by_name("META-INF/MANIFEST.MF")
        .ok()?
        .read_to_string(&mut manifest)
        .ok()?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("Kotlin-Version:"))?
        .trim()
        .split('.')
        .next()?
        .parse()
        .ok()
}

#[test]
fn indexes_real_kotlin_declarations_from_metadata() {
    let Some(jar) = find_kotlin_stdlib() else {
        eprintln!("skipping Kotlin JAR indexing test: no kotlin-stdlib JAR in Maven/Gradle caches");
        return;
    };
    eprintln!("Kotlin JAR indexing fixture: {}", jar.display());

    let (symbols, _) =
        extract_jar_symbols(&jar, FileId::new(7)).expect("index the kotlin-stdlib fixture");

    let pair = symbols
        .iter()
        .find(|symbol| symbol.fqn.as_ref() == "kotlin.Pair")
        .expect("kotlin.Pair class");
    assert!(
        pair.modifiers.is_data,
        "Pair must retain Kotlin data identity"
    );
    let companion = symbols
        .iter()
        .find(|symbol| symbol.fqn.as_ref() == "kotlin.KotlinVersion.Companion")
        .expect("KotlinVersion.Companion nested object");
    assert_eq!(companion.simple_name.as_ref(), "Companion");
    assert!(
        symbols
            .iter()
            .all(|symbol| symbol.fqn.as_ref() != "kotlin.KotlinVersion$Companion"),
        "Kotlin metadata must keep nested declarations in source form"
    );

    let copy = symbols
        .iter()
        .find(|symbol| {
            symbol.container.as_deref() == Some("kotlin.Pair")
                && symbol.simple_name.as_ref() == "copy"
        })
        .expect("Pair.copy");
    let copy_metadata = copy.kotlin.as_ref().expect("Kotlin metadata on Pair.copy");
    assert_eq!(copy.parameters.len(), 2);
    assert!(
        copy_metadata
            .parameters
            .iter()
            .all(|parameter| parameter.has_default),
        "both Pair.copy parameters have defaults"
    );
    let equals = symbols
        .iter()
        .find(|symbol| {
            symbol.container.as_deref() == Some("kotlin.Pair")
                && symbol.simple_name.as_ref() == "equals"
        })
        .expect("Pair.equals");
    assert!(
        equals
            .kotlin
            .as_ref()
            .and_then(|metadata| metadata.parameters.first())
            .is_some_and(|parameter| parameter.nullable),
        "Pair.equals must retain its nullable Any? parameter"
    );
    let first = symbols
        .iter()
        .find(|symbol| {
            symbol.container.as_deref() == Some("kotlin.Pair")
                && symbol.simple_name.as_ref() == "first"
                && symbol.kind == SymbolKind::PROPERTY
        })
        .expect("Pair.first Kotlin property");
    let first_property = first
        .kotlin
        .as_ref()
        .and_then(|metadata| metadata.property.as_ref())
        .expect("Pair.first property metadata");
    assert!(first_property.is_val);
    assert!(first_property.has_getter);
    assert!(!first_property.has_setter);

    let yield_one = symbols
        .iter()
        .find(|symbol| {
            symbol.container.as_deref() == Some("kotlin.sequences.SequenceScope")
                && symbol.simple_name.as_ref() == "yield"
        })
        .expect("SequenceScope.yield");
    assert!(
        yield_one
            .kotlin
            .as_ref()
            .is_some_and(|metadata| metadata.is_suspend),
        "SequenceScope.yield must be marked suspend"
    );
    assert_eq!(
        yield_one.parameters.len(),
        1,
        "the JVM Continuation parameter must be hidden"
    );
    assert!(yield_one
        .parameters
        .iter()
        .all(|parameter| !parameter.contains("Continuation")));

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(symbols);
    let substring_before = index.find_extension_by_name("kotlin.String", "substringBefore");
    let char_overload = substring_before
        .iter()
        .find(|symbol| {
            symbol.kind == SymbolKind::FUNCTION
                && symbol
                    .parameters
                    .first()
                    .is_some_and(|parameter| parameter.starts_with("kotlin.Char "))
        })
        .expect("String.substringBefore(Char, String)");
    assert_eq!(
        char_overload.receiver_type.as_deref(),
        Some("kotlin.String")
    );
    assert_eq!(
        char_overload.container.as_deref(),
        Some("kotlin.text.StringsKt"),
        "kind-5 declarations must route through the kind-4 facade"
    );
    assert!(
        char_overload
            .kotlin
            .as_ref()
            .and_then(|metadata| metadata.parameters.get(1))
            .is_some_and(|parameter| parameter.has_default),
        "missingDelimiterValue must be marked defaulted"
    );
    assert_eq!(
        substring_before
            .iter()
            .filter(|symbol| {
                symbol
                    .parameters
                    .first()
                    .is_some_and(|parameter| parameter.starts_with("kotlin.Char "))
            })
            .count(),
        1,
        "kind-4 facade routing must not duplicate kind-5 declarations"
    );
}

/// A JDK declaration, standing in for the JDK, which these tests do not index.
fn jdk_type(fqn: &str, kind: SymbolKind) -> Symbol {
    Symbol {
        fqn: fqn.into(),
        simple_name: fqn.rsplit('.').next().unwrap_or(fqn).into(),
        range: TextRange::new(0, 0),
        kind,
        modifiers: Modifiers {
            is_public: true,
            ..Modifiers::default()
        },
        container: None,
        return_type: None,
        parameters: vec![],
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        type_parameters: vec![],
        is_deprecated: false,
        receiver_type: None,
        kotlin: None,
        throws: None,
    }
}

/// The stdlib's typealiases are recorded on the facade classes that
/// declare them, and Kotlin type-name resolution expands them through the index.
#[test]
fn stdlib_typealiases_resolve_to_their_expanded_classes() {
    let Some(jar) = find_kotlin_stdlib() else {
        eprintln!("skipping stdlib typealias test: no kotlin-stdlib JAR in Maven/Gradle caches");
        return;
    };
    let (symbols, _) =
        extract_jar_symbols(&jar, FileId::new(7)).expect("index the kotlin-stdlib fixture");
    let aliases_of = |facade: &str| {
        symbols
            .iter()
            .find(|symbol| symbol.fqn.as_ref() == facade)
            .and_then(|symbol| symbol.kotlin.as_ref())
            .map(|kotlin| kotlin.type_aliases.clone())
            .unwrap_or_default()
    };
    // These aliases are generic (`ArrayList<E> = java.util.ArrayList<E>`): their
    // own type parameters are recorded, and the expansion's arguments name them.
    let alias = |fqn: &str, expanded_class: &str, parameters: &[&str]| KotlinTypeAlias {
        fqn: fqn.into(),
        expanded_class: expanded_class.into(),
        type_parameters: parameters.iter().map(|name| (*name).to_owned()).collect(),
        expanded_type_arguments: parameters.iter().map(|name| (*name).to_owned()).collect(),
    };
    let collections = aliases_of("kotlin.collections.TypeAliasesKt");
    assert!(
        collections.contains(&alias(
            "kotlin.collections.ArrayList",
            "java.util.ArrayList",
            &["E"]
        )),
        "{collections:?}"
    );
    assert!(collections.contains(&alias(
        "kotlin.collections.HashMap",
        "java.util.HashMap",
        &["K", "V"]
    )));
    assert!(aliases_of("kotlin.TypeAliasesKt").contains(&alias(
        "kotlin.Comparator",
        "java.util.Comparator",
        &["T"]
    )));
    assert!(
        aliases_of("kotlin.io.SerializableKt").is_empty(),
        "`internal actual typealias Serializable` cannot be named outside the stdlib"
    );
    // `typealias AutoCloseable` exists from Kotlin 2.0. In 1.x the facade holds only `use`.
    if kotlin_major_version(&jar) >= Some(2) {
        assert!(
            aliases_of("kotlin.jdk7.AutoCloseableKt")
                .iter()
                .any(|alias| alias.fqn == "kotlin.AutoCloseable"),
            "@JvmPackageName moves the facade, not the alias's package"
        );
    }

    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(symbols);
    index.add_external_symbols_batch(vec![
        jdk_type("java.util.ArrayList", SymbolKind::CLASS),
        jdk_type("java.util.Comparator", SymbolKind::INTERFACE),
    ]);
    let source_path = temp.path().join("app/Names.kt");
    index
        .index_file_with_source(
            &source_path,
            "package app\n\nfun sorted(names: ArrayList<String>, order: Comparator<String>) =\n    \
             names.sortedWith(order)\n",
        )
        .unwrap();
    let file_id = index.get_file_id(&source_path).unwrap();
    for (written, expanded) in [
        ("ArrayList", "java.util.ArrayList"),
        ("kotlin.collections.ArrayList", "java.util.ArrayList"),
        ("Comparator", "java.util.Comparator"),
        ("kotlin.Comparator", "java.util.Comparator"),
    ] {
        assert_eq!(
            index.resolve_type_name(file_id, &[], written),
            TypeNameResolution::Exact(expanded.into()),
            "{written} must resolve through the stdlib typealias"
        );
    }
    // `kotlin.Comparator` is not in the built-in table, so its use binds the
    // alias's `T`; a use without its argument is not guessed.
    let string = ["kotlin.String".to_owned()];
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(file_id, &[], "Comparator", &string),
        KotlinTypeAliasArguments::Expanded(string.to_vec())
    );
    assert_eq!(
        index.resolve_kotlin_type_alias_arguments(file_id, &[], "Comparator", &[]),
        KotlinTypeAliasArguments::Unmatched
    );
}
