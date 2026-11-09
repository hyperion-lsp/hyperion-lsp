// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Import quick fixes, "add all missing imports" and organize imports.

use super::refactor_harness::{apply_workspace_edit, title_of, Harness};
use tower_lsp::lsp_types::{CodeActionOrCommand, Diagnostic};

const APP: &str = "package demo;\n\npublic class App {\n    List<String> names = new ArrayList<>();\n    Map<String, String> index;\n    Optional<String> first;\n}\n";

async fn wait_for_unresolved(h: &Harness, file: &str, name: &str) -> Diagnostic {
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    loop {
        if let Some(found) = h.latest_diagnostic_items(file).and_then(|items| {
            items
                .into_iter()
                .find(|d| d.message.contains("cannot find symbol") && d.message.contains(name))
        }) {
            return found;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no unresolved-symbol diagnostic for {name}: {:?}",
            h.latest_diagnostics(file)
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Quick fixes for `name`, retried until the JDK index is loaded (it is built
/// in the background, and until then nothing can be offered).
async fn fixes_when_jdk_is_indexed(h: &Harness, name: &str) -> Vec<CodeActionOrCommand> {
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    loop {
        let diagnostic = wait_for_unresolved(h, "demo/App.java", name).await;
        let actions = h.quick_fixes("demo/App.java", &diagnostic).await;
        if !import_titles(&actions).is_empty() {
            return actions;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no import fix for {name}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

fn import_titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    actions
        .iter()
        .map(title_of)
        .filter(|title| title.starts_with("Import '"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn import_fix_offers_the_jdk_type_first_and_no_internals_or_members() {
    let h = Harness::start(&[("demo/App.java", APP)]).await;
    h.open("demo/App.java").await;
    for (name, expected) in [
        ("List", "Import 'java.util.List'"),
        ("Map", "Import 'java.util.Map'"),
        ("Optional", "Import 'java.util.Optional'"),
        ("ArrayList", "Import 'java.util.ArrayList'"),
    ] {
        let actions = fixes_when_jdk_is_indexed(&h, name).await;
        let titles = import_titles(&actions);
        assert_eq!(
            titles.first().map(String::as_str),
            Some(expected),
            "{name}: {titles:?}"
        );
        for title in &titles {
            assert!(
                !title.contains("com.sun.")
                    && !title.contains("'sun.")
                    && !title.contains("jdk.internal"),
                "{name}: non-exported JDK internals offered: {titles:?}"
            );
            assert!(
                !title.contains('('),
                "{name}: constructor offered as import: {titles:?}"
            );
        }
        let mut deduped = titles.clone();
        deduped.dedup();
        assert_eq!(titles, deduped, "{name}: duplicate imports offered");
        // The first import is the preferred one.
        let first = actions
            .iter()
            .find_map(|a| match a {
                CodeActionOrCommand::CodeAction(action) if action.title == expected => Some(action),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            first.is_preferred,
            Some(true),
            "{name}: java.util type must be preferred"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn add_all_missing_imports_and_organize_imports_add_unambiguous_imports() {
    let h = Harness::start(&[("demo/App.java", APP)]).await;
    h.open("demo/App.java").await;
    wait_for_unresolved(&h, "demo/App.java", "Optional").await;
    let actions = fixes_when_jdk_is_indexed(&h, "List").await;
    let all = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(action)
                if action.title == "Add all missing imports" =>
            {
                Some(action.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "no 'Add all missing imports' in {:?}",
                actions.iter().map(title_of).collect::<Vec<_>>()
            )
        });
    apply_workspace_edit(all.edit.as_ref().expect("edit"));
    let fixed = h.read("demo/App.java");
    for import in [
        "import java.util.ArrayList;",
        "import java.util.List;",
        "import java.util.Map;",
        "import java.util.Optional;",
    ] {
        assert!(fixed.contains(import), "missing {import}:\n{fixed}");
    }
    assert!(
        fixed.starts_with("package demo;\n\nimport java.util.ArrayList;\n"),
        "{fixed}"
    );
}

const MESSY: &str = "package demo;\n\nimport java.util.Set;\nimport java.io.File;\n\npublic class Messy {\n    Set<String> set;\n    File file;\n    List<String> names;\n    Optional<String> first;\n}\n";

#[tokio::test(flavor = "multi_thread")]
async fn organize_imports_sorts_and_adds_unambiguous_missing_imports() {
    let h = Harness::start(&[("demo/Messy.java", MESSY)]).await;
    h.open("demo/Messy.java").await;
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    let position = h.position_of("demo/Messy.java", "class Messy", 0);
    let organize = loop {
        let actions = h
            .code_actions_until("demo/Messy.java", position, |_| true)
            .await;
        let organize = actions.iter().find_map(|a| match a {
            CodeActionOrCommand::CodeAction(action)
                if action.title == "Organize imports"
                    && format!("{:?}", action.edit).contains("java.util.List") =>
            {
                Some(action.clone())
            }
            _ => None,
        });
        if let Some(organize) = organize {
            break organize;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "organize imports never added List"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };
    apply_workspace_edit(organize.edit.as_ref().unwrap());
    let organized = h.read("demo/Messy.java");
    assert!(
        organized.starts_with(
            "package demo;\n\nimport java.io.File;\nimport java.util.List;\nimport java.util.Optional;\nimport java.util.Set;\n\npublic class Messy"
        ),
        "{organized}"
    );
}
