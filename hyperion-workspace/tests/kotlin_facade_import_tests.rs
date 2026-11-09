// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: Kotlin facade classes are Java-importable
//! entry points (`import kotlin.collections.CollectionsKt;` is how Java calls
//! Kotlin top-level functions), but the jar indexer dropped them:
//! kind-4 multifile facades returned no symbols at all, and kind-2 file
//! facades suppressed their class symbol. Their packages ARE populated by
//! sibling classes, so `check_import_decl` emitted a false
//! "undefined symbol" for exactly these imports while others worked.

use hyperion_workspace::{extract_jar_symbols, FileId, WorkspaceIndex};
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

#[test]
fn kotlin_facade_classes_are_indexed_as_importable_classes() {
    let Some(jar) = find_kotlin_stdlib() else {
        eprintln!("skipping facade test: no kotlin-stdlib JAR in Maven/Gradle caches");
        return;
    };

    let (symbols, _) = extract_jar_symbols(&jar, FileId::new(11)).expect("stdlib should index");
    let temp = TempDir::new().unwrap();
    let index = WorkspaceIndex::new(temp.path().to_path_buf());
    index.add_external_symbols_batch(symbols);

    // Kind-4 multifile facades and kind-2 file facades — all Java-importable.
    for facade in [
        "kotlin.collections.CollectionsKt",
        "kotlin.text.StringsKt",
        "kotlin.io.FilesKt",
        "kotlin.LazyKt",
        "kotlin.TuplesKt",
    ] {
        assert!(
            index.has_class(facade),
            "{facade} must be indexed as an importable class"
        );
        let symbol = index.get_symbol(facade).expect("facade symbol");
        assert_eq!(
            symbol.kind,
            SymbolKind::CLASS,
            "{facade} must be a CLASS symbol"
        );
    }

    // Kind-5 part classes are compiler artifacts and must stay suppressed —
    // their declarations route through the kind-4 facade.
    let part_indexed = index
        .get_package_class_names("kotlin.collections")
        .iter()
        .any(|name| name.contains("__"));
    assert!(
        !part_indexed,
        "kind-5 part classes (Foo__BarKt) must not be indexed as classes"
    );
}
