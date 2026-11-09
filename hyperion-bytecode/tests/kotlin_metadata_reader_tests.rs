// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_bytecode::{
    kotlin_metadata::{KotlinClassMetadata, KotlinClassifier, KotlinMetadataKind, KotlinVariance},
    parser::parse_class_file,
};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use zip::ZipArchive;

fn find_jar(file_name_prefix: &str, search_roots: &[PathBuf]) -> Option<PathBuf> {
    for root in search_roots {
        if !root.exists() {
            continue;
        }
        let mut candidates = Vec::new();
        for entry in WalkDir::new(root)
            .max_depth(8)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let name = entry.file_name().to_string_lossy();
            let is_versioned_artifact = name
                .strip_prefix(file_name_prefix)
                .and_then(|suffix| suffix.as_bytes().first())
                .is_some_and(u8::is_ascii_digit);
            if entry.file_type().is_file()
                && is_versioned_artifact
                && name.ends_with(".jar")
                && !name.contains("-sources")
            {
                candidates.push(entry.into_path());
            }
        }
        candidates.sort_unstable();
        if let Some(candidate) = candidates.into_iter().next() {
            return Some(candidate);
        }
    }
    None
}

fn assert_metadata_version_is_well_formed(version: &[i32], jar: &Path) {
    assert!(
        matches!(version, [major, minor, patch, ..] if *major > 0 && *minor >= 0 && *patch >= 0),
        "invalid Kotlin metadata version {version:?} in {}",
        jar.display()
    );
}

fn kotlin_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(
            home.join(".m2")
                .join("repository/org/jetbrains/kotlin/kotlin-stdlib"),
        );
        roots.push(home.join(".gradle/caches"));
        roots.push(home.join(".sdkman/candidates/gradle"));
    }
    roots
}

fn class_bytes(jar_path: &Path, class_path: &str) -> Vec<u8> {
    let file = File::open(jar_path)
        .unwrap_or_else(|error| panic!("failed to open fixture {}: {error}", jar_path.display()));
    let mut jar = ZipArchive::new(file).unwrap();
    let mut entry = jar.by_name(class_path).unwrap_or_else(|error| {
        panic!(
            "fixture {} has no {class_path}: {error}",
            jar_path.display()
        )
    });
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn reads_real_stdlib_class_metadata() {
    let Some(jar) = find_jar("kotlin-stdlib-", &kotlin_search_roots()) else {
        eprintln!("skipping real Kotlin metadata test: no kotlin-stdlib JAR found");
        return;
    };
    let bytes = class_bytes(&jar, "kotlin/Pair.class");
    let class_file = parse_class_file(&bytes).unwrap();
    let info = KotlinClassMetadata::read_strict(&class_file)
        .unwrap()
        .expect("Pair must carry kotlin.Metadata");

    assert_eq!(info.metadata.kind, KotlinMetadataKind::Class);
    assert_eq!(info.name, "kotlin.Pair");
    assert!(info.is_data_class);
    assert_eq!(
        info.type_parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter.variance))
            .collect::<Vec<_>>(),
        vec![("A", KotlinVariance::Out), ("B", KotlinVariance::Out)]
    );

    let first = info
        .properties
        .iter()
        .find(|property| property.name == "first")
        .expect("Pair.first property");
    assert!(first.is_val);
    assert!(first.has_getter);
    assert!(!first.has_setter);
    assert_eq!(first.ty.classifier, KotlinClassifier::TypeParameter(0));

    let copy = info
        .functions
        .iter()
        .find(|function| function.name == "copy")
        .expect("data-class copy function");
    assert_eq!(
        copy.parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter.has_default))
            .collect::<Vec<_>>(),
        vec![("first", true), ("second", true)]
    );

    let equals = info
        .functions
        .iter()
        .find(|function| function.name == "equals")
        .expect("Pair.equals");
    let other = equals.parameters.first().expect("equals other parameter");
    assert_eq!(
        other.ty.classifier,
        KotlinClassifier::Class("kotlin.Any".to_string())
    );
    assert!(other.ty.nullable, "Pair.equals takes Any?");
}

#[test]
fn reads_real_stdlib_extension_receiver_and_jvm_signature() {
    let Some(jar) = find_jar("kotlin-stdlib-", &kotlin_search_roots()) else {
        eprintln!("skipping real Kotlin metadata test: no kotlin-stdlib JAR found");
        return;
    };
    let bytes = class_bytes(&jar, "kotlin/text/StringsKt__StringsKt.class");
    let class_file = parse_class_file(&bytes).unwrap();
    let info = KotlinClassMetadata::read_strict(&class_file)
        .unwrap()
        .expect("StringsKt__StringsKt must carry kotlin.Metadata");

    assert_eq!(info.metadata.kind, KotlinMetadataKind::MultiFileClassPart);
    assert_metadata_version_is_well_formed(&info.metadata.metadata_version, &jar);
    assert_eq!(
        info.metadata.extra_string.as_deref(),
        Some("kotlin/text/StringsKt")
    );
    assert_eq!(info.metadata.extra_int, 49);
    let substring_before = info
        .functions
        .iter()
        .find(|function| {
            function.name == "substringBefore"
                && function.parameters.first().is_some_and(|parameter| {
                    parameter.ty.classifier == KotlinClassifier::Class("kotlin.Char".to_string())
                })
        })
        .expect("String.substringBefore(Char, String)");
    assert_eq!(
        substring_before
            .extension_receiver_type
            .as_ref()
            .and_then(|ty| ty.class_name()),
        Some("kotlin.String")
    );
    assert_eq!(substring_before.parameters[1].name, "missingDelimiterValue");
    assert!(substring_before.parameters[1].has_default);
    let if_blank = info
        .functions
        .iter()
        .find(|function| function.name == "ifBlank")
        .expect("CharSequence.ifBlank");
    let signature = if_blank
        .jvm_signature
        .as_ref()
        .expect("ifBlank has an explicit JVM signature extension");
    assert_eq!(signature.name, "ifBlank");
    assert_eq!(
        signature.descriptor.as_deref(),
        Some("(Ljava/lang/CharSequence;Lkotlin/jvm/functions/Function0;)Ljava/lang/Object;")
    );
}

#[test]
fn reads_real_multifile_facade_part_names() {
    let Some(jar) = find_jar("kotlin-stdlib-", &kotlin_search_roots()) else {
        eprintln!("skipping real Kotlin metadata test: no kotlin-stdlib JAR found");
        return;
    };
    let bytes = class_bytes(&jar, "kotlin/text/StringsKt.class");
    let class_file = parse_class_file(&bytes).unwrap();
    let info = KotlinClassMetadata::read_strict(&class_file)
        .unwrap()
        .expect("StringsKt must carry kotlin.Metadata");

    assert_eq!(info.metadata.kind, KotlinMetadataKind::MultiFileClassFacade);
    assert!(info
        .multi_file_part_names
        .iter()
        .any(|name| name == "kotlin/text/StringsKt__StringsKt"));
    assert!(info.functions.is_empty());
}

#[test]
fn reads_real_coroutines_suspend_function() {
    let roots = kotlin_search_roots();
    let Some(jar) = find_jar("kotlinx-coroutines-core-jvm-", &roots)
        .or_else(|| find_jar("kotlinx-coroutines-core-", &roots))
    else {
        eprintln!("skipping real Kotlin metadata test: no kotlinx-coroutines-core JAR found");
        return;
    };
    let bytes = class_bytes(&jar, "kotlinx/coroutines/DelayKt.class");
    let class_file = parse_class_file(&bytes).unwrap();
    let info = KotlinClassMetadata::read_strict(&class_file)
        .unwrap()
        .expect("DelayKt must carry kotlin.Metadata");

    assert_eq!(info.metadata.kind, KotlinMetadataKind::FileFacade);
    assert_metadata_version_is_well_formed(&info.metadata.metadata_version, &jar);
    assert_eq!(info.metadata.extra_int, 48);
    let delay = info
        .functions
        .iter()
        .find(|function| {
            function.name == "delay"
                && function.parameters.first().is_some_and(|parameter| {
                    parameter.ty.classifier == KotlinClassifier::Class("kotlin.Long".to_string())
                })
        })
        .expect("suspend delay(Long)");
    assert!(delay.is_suspend);
    assert_eq!(delay.parameters[0].name, "timeMillis");
    assert_eq!(
        delay.return_type.classifier,
        KotlinClassifier::Class("kotlin.Unit".to_string())
    );
    assert_eq!(
        delay
            .jvm_signature
            .as_ref()
            .and_then(|signature| signature.descriptor.as_deref()),
        Some("(JLkotlin/coroutines/Continuation;)Ljava/lang/Object;")
    );
}

/// `kotlin.collections` declares its JVM aliases in a file facade. `ArrayList` is
/// `typealias ArrayList<E> = java.util.ArrayList<E>`, which the package metadata must give.
#[test]
fn reads_stdlib_package_typealiases() {
    let Some(jar) = find_jar("kotlin-stdlib-", &kotlin_search_roots()) else {
        eprintln!("skipping stdlib typealias test: no kotlin-stdlib JAR found");
        return;
    };
    let bytes = class_bytes(&jar, "kotlin/collections/TypeAliasesKt.class");
    let class_file = parse_class_file(&bytes).unwrap();
    let info = KotlinClassMetadata::read_strict(&class_file)
        .unwrap()
        .expect("the typealias facade must carry kotlin.Metadata");

    let array_list = info
        .type_aliases
        .iter()
        .find(|alias| alias.name == "ArrayList")
        .expect("ArrayList is a typealias in kotlin.collections");
    assert_eq!(
        array_list.expanded_type.class_name(),
        Some("java.util.ArrayList")
    );
    assert!(array_list.is_public);
    // The alias's own parameter `E`, which the expansion's argument names, by
    // name in the stdlib JARs checked, or by id.
    let [element] = array_list.type_parameters.as_slice() else {
        panic!(
            "ArrayList has one type parameter: {:?}",
            array_list.type_parameters
        );
    };
    assert_eq!(element.name, "E");
    let arguments = array_list
        .expanded_type
        .arguments
        .iter()
        .map(|argument| argument.ty.as_ref().map(|ty| &ty.classifier))
        .collect::<Vec<_>>();
    assert!(
        matches!(
            arguments.as_slice(),
            [Some(KotlinClassifier::TypeParameterName(name))] if *name == element.name
        ) || matches!(
            arguments.as_slice(),
            [Some(KotlinClassifier::TypeParameter(id))] if *id == element.id
        ),
        "{arguments:?}"
    );
    let hash_map = info
        .type_aliases
        .iter()
        .find(|alias| alias.name == "HashMap")
        .expect("HashMap is a typealias in kotlin.collections");
    assert_eq!(
        hash_map
            .type_parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["K", "V"]
    );

    // `private typealias State = Int` in SequenceBuilder.kt, a multifile class part.
    let part_bytes = class_bytes(
        &jar,
        "kotlin/sequences/SequencesKt__SequenceBuilderKt.class",
    );
    let part_file = parse_class_file(&part_bytes).unwrap();
    let part = KotlinClassMetadata::read_strict(&part_file)
        .unwrap()
        .expect("the SequenceBuilder part must carry kotlin.Metadata");
    let state = part
        .type_aliases
        .iter()
        .find(|alias| alias.name == "State")
        .expect("State is a typealias in SequenceBuilder.kt");
    assert!(!state.is_public, "State is declared private");
}

#[test]
fn reads_stdlib_nested_classes_and_enum_entries() {
    let Some(jar) = find_jar("kotlin-stdlib-", &kotlin_search_roots()) else {
        eprintln!("skipping stdlib nested/enum test: no kotlin-stdlib JAR found");
        return;
    };

    // `CoroutineContext` declares the nested interfaces `Element` and `Key`.
    let context_bytes = class_bytes(&jar, "kotlin/coroutines/CoroutineContext.class");
    let context_file = parse_class_file(&context_bytes).unwrap();
    let context = KotlinClassMetadata::read_strict(&context_file)
        .unwrap()
        .expect("kotlin.coroutines.CoroutineContext must carry kotlin.Metadata");
    for nested in ["Element", "Key"] {
        assert!(
            context.nested_classes.iter().any(|name| name == nested),
            "nested classes: {:?}",
            context.nested_classes
        );
    }

    // The enum's entries are recorded in declaration order.
    let regex_option_bytes = class_bytes(&jar, "kotlin/text/RegexOption.class");
    let regex_option_file = parse_class_file(&regex_option_bytes).unwrap();
    let regex_option = KotlinClassMetadata::read_strict(&regex_option_file)
        .unwrap()
        .expect("kotlin.text.RegexOption must carry kotlin.Metadata");
    assert_eq!(
        regex_option.enum_entries.first().map(String::as_str),
        Some("IGNORE_CASE"),
        "enum entries: {:?}",
        regex_option.enum_entries
    );
    assert!(regex_option
        .enum_entries
        .iter()
        .any(|name| name == "MULTILINE"));
}
