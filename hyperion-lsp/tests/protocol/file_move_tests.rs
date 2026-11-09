// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Moving types between packages through the real `LanguageServer` boundary.

use super::refactor_harness::{apply_workspace_edit, find_action, title_of, Harness};

const SHAPE: &str = "package demo.shapes;\n\npublic interface Shape {\n    double area();\n}\n";
const CIRCLE: &str = "package demo.shapes;\n\n/** A circle, see {@link Shape}. */\npublic class Circle implements Shape {\n    private final double r;\n\n    public Circle(double r) {\n        this.r = r;\n    }\n\n    @Override\n    public double area() {\n        return Math.PI * r * r;\n    }\n}\n";
const UTIL: &str = "package demo.shapes;\n\npublic class Util {\n    static Circle unit() {\n        return new Circle(1);\n    }\n}\n";
const MAIN: &str = "package demo.app;\n\nimport demo.shapes.Circle;\nimport demo.shapes.Shape;\n\npublic class Main {\n    public static void main(String[] args) {\n        Shape s = new Circle(2);\n        System.out.println(s.area());\n    }\n}\n";
const MARKER: &str = "package demo.geo;\n\npublic class Marker {}\n";

#[tokio::test(flavor = "multi_thread")]
async fn move_class_to_package_relocates_file_and_fixes_both_sides() {
    let h = Harness::start(&[
        ("demo/shapes/Shape.java", SHAPE),
        ("demo/shapes/Circle.java", CIRCLE),
        ("demo/shapes/Util.java", UTIL),
        ("demo/app/Main.java", MAIN),
        ("demo/geo/Marker.java", MARKER),
    ])
    .await;
    h.open("demo/shapes/Circle.java").await;
    let position = h.position_of("demo/shapes/Circle.java", "Circle implements", 2);
    let actions = h
        .code_actions_until("demo/shapes/Circle.java", position, |actions| {
            actions
                .iter()
                .any(|a| title_of(a) == "Move 'Circle' to package 'demo.geo'")
        })
        .await;
    let action = find_action(&actions, "Move 'Circle' to package 'demo.geo'").unwrap();
    let edit = action.edit.clone().expect("move action carries an edit");
    apply_workspace_edit(&edit);

    assert!(
        !h.exists("demo/shapes/Circle.java"),
        "original file must be gone, not blanked"
    );
    let moved = h.read("demo/geo/Circle.java");
    assert!(
        moved.starts_with("package demo.geo;\n\nimport demo.shapes.Shape;\n\n/** A circle"),
        "{moved}"
    );
    assert!(
        moved.contains("import demo.shapes.Shape;"),
        "moved class must import the same-package type it used:\n{moved}"
    );
    assert!(
        moved.contains("public class Circle implements Shape"),
        "{moved}"
    );
    let main = h.read("demo/app/Main.java");
    assert!(main.contains("import demo.geo.Circle;"), "{main}");
    assert!(!main.contains("demo.shapes.Circle"), "{main}");
    assert!(main.contains("import demo.shapes.Shape;"), "{main}");
    let util = h.read("demo/shapes/Util.java");
    assert!(
        util.contains("import demo.geo.Circle;"),
        "same-package user of the moved class needs an import:\n{util}"
    );
    assert!(
        util.starts_with("package demo.shapes;\n\nimport demo.geo.Circle;\n\npublic class Util"),
        "{util}"
    );
}

fn rename_params(h: &Harness, old: &str, new: &str) -> tower_lsp::lsp_types::RenameFilesParams {
    tower_lsp::lsp_types::RenameFilesParams {
        files: vec![tower_lsp::lsp_types::FileRename {
            old_uri: h.uri(old).to_string(),
            new_uri: h.uri(new).to_string(),
        }],
    }
}

/// Ask the server what a file-explorer rename needs, retrying while the
/// workspace is still indexing, then act as the client would: apply the edit,
/// then move the path.
async fn explorer_rename(h: &Harness, old: &str, new: &str) {
    use tower_lsp::LanguageServer;
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    let edit = loop {
        let edit = h
            .backend
            .will_rename_files(rename_params(h, old, new))
            .await
            .expect("willRenameFiles");
        if let Some(edit) = edit {
            break edit;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "willRenameFiles never produced an edit"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    apply_workspace_edit(&edit);
    std::fs::create_dir_all(h.path(new).parent().unwrap()).unwrap();
    std::fs::rename(h.path(old), h.path(new)).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn server_advertises_file_operations_and_watchers() {
    let h = Harness::start(&[("demo/A.java", "package demo;\npublic class A {}\n")]).await;
    let operations = h
        .capabilities
        .workspace
        .as_ref()
        .and_then(|w| w.file_operations.as_ref())
        .expect("fileOperations capability");
    assert!(operations.will_rename.is_some());
    assert!(operations.did_rename.is_some() && operations.did_create.is_some());
    assert!(operations.did_delete.is_some());
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    loop {
        let registered = h
            .server_messages
            .lock()
            .unwrap()
            .iter()
            .any(|(method, params)| {
                method == "client/registerCapability"
                    && params
                        .to_string()
                        .contains("workspace/didChangeWatchedFiles")
                    && params.to_string().contains("**/*.java")
            });
        if registered {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no didChangeWatchedFiles registration"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn explorer_move_of_a_file_updates_package_and_references() {
    let h = Harness::start(&[
        ("demo/shapes/Shape.java", SHAPE),
        ("demo/shapes/Circle.java", CIRCLE),
        ("demo/shapes/Util.java", UTIL),
        ("demo/app/Main.java", MAIN),
        ("demo/geo/Marker.java", MARKER),
    ])
    .await;
    explorer_rename(&h, "demo/shapes/Circle.java", "demo/geo/Circle.java").await;

    let moved = h.read("demo/geo/Circle.java");
    assert!(
        moved.starts_with("package demo.geo;\n\nimport demo.shapes.Shape;\n"),
        "{moved}"
    );
    assert!(h
        .read("demo/app/Main.java")
        .contains("import demo.geo.Circle;"));
    assert!(h
        .read("demo/shapes/Util.java")
        .contains("import demo.geo.Circle;"));
}

#[tokio::test(flavor = "multi_thread")]
async fn explorer_move_of_a_directory_renames_the_package() {
    let h = Harness::start(&[
        ("demo/shapes/Shape.java", SHAPE),
        ("demo/shapes/Circle.java", CIRCLE),
        ("demo/shapes/Util.java", UTIL),
        ("demo/app/Main.java", MAIN),
    ])
    .await;
    explorer_rename(&h, "demo/shapes", "demo/figures").await;

    for file in ["Shape", "Circle", "Util"] {
        let text = h.read(&format!("demo/figures/{file}.java"));
        assert!(
            text.starts_with("package demo.figures;\n"),
            "{file}: {text}"
        );
    }
    // Files that moved together keep seeing each other without imports.
    let util = h.read("demo/figures/Util.java");
    assert!(!util.contains("import"), "{util}");
    let main = h.read("demo/app/Main.java");
    assert!(main.contains("import demo.figures.Circle;"), "{main}");
    assert!(main.contains("import demo.figures.Shape;"), "{main}");
    assert!(!main.contains("demo.shapes"), "{main}");
}

#[tokio::test(flavor = "multi_thread")]
async fn explorer_rename_of_a_public_class_file_renames_the_class() {
    let h = Harness::start(&[
        ("demo/shapes/Shape.java", SHAPE),
        ("demo/shapes/Circle.java", CIRCLE),
        ("demo/app/Main.java", MAIN),
    ])
    .await;
    explorer_rename(&h, "demo/shapes/Circle.java", "demo/shapes/Round.java").await;
    let round = h.read("demo/shapes/Round.java");
    assert!(
        round.contains("public class Round implements Shape"),
        "{round}"
    );
    let main = h.read("demo/app/Main.java");
    assert!(main.contains("import demo.shapes.Round;"), "{main}");
    assert!(main.contains("new Round(2)"), "{main}");
}

#[tokio::test(flavor = "multi_thread")]
async fn explorer_rename_uses_the_declaration_token_after_comments_and_prefix_names() {
    let circle = "package demo;\r\n// class Circle is the public API.\r\nclass CircleHelper {}\r\npublic class\r\n/* declaration */ Circle { public Circle() {} }\r\n";
    let main = "package demo;\nclass Use { Circle value = new Circle(); }\n";
    let h = Harness::start(&[("demo/Circle.java", circle), ("demo/Use.java", main)]).await;
    explorer_rename(&h, "demo/Circle.java", "demo/Round.java").await;

    let renamed = h.read("demo/Round.java");
    assert!(
        renamed.contains("/* declaration */ Round { public Round() {} }"),
        "{renamed}"
    );
    assert!(
        renamed.contains("// class Circle is the public API."),
        "{renamed}"
    );
    assert!(renamed.contains("class CircleHelper {}"), "{renamed}");
    let usage = h.read("demo/Use.java");
    assert!(usage.contains("Round value = new Round();"), "{usage}");
}

#[tokio::test(flavor = "multi_thread")]
async fn explorer_move_updates_kotlin_references_and_moves_kotlin_sources() {
    let h = Harness::start(&[
        ("demo/shapes/Shape.java", SHAPE),
        ("demo/shapes/Circle.java", CIRCLE),
        (
            "demo/app/Use.kt",
            "package demo.app\n\nimport demo.shapes.Circle\n\nval c: Circle = Circle(1.0)\n",
        ),
        (
            "demo/shapes/Local.kt",
            "package demo.shapes\n\nfun local(): Circle = Circle(2.0)\n",
        ),
        (
            "demo/shapes/Ring.kt",
            "package demo.shapes\n\nclass Ring(val inner: Shape)\n",
        ),
        ("demo/geo/Marker.java", MARKER),
    ])
    .await;
    explorer_rename(&h, "demo/shapes/Circle.java", "demo/geo/Circle.java").await;
    let use_kt = h.read("demo/app/Use.kt");
    assert!(use_kt.contains("import demo.geo.Circle\n"), "{use_kt}");
    let local = h.read("demo/shapes/Local.kt");
    assert!(
        local.starts_with("package demo.shapes\n\nimport demo.geo.Circle\n"),
        "Kotlin same-package user needs the import: {local}"
    );

    // Moving a Kotlin class rewrites its package declaration too and adds the
    // imports for the neighbours it used implicitly.
    explorer_rename(&h, "demo/shapes/Ring.kt", "demo/geo/Ring.kt").await;
    let ring = h.read("demo/geo/Ring.kt");
    assert!(
        ring.starts_with("package demo.geo\n\nimport demo.shapes.Shape\n"),
        "{ring}"
    );
}

async fn rename_at(
    h: &Harness,
    file: &str,
    position: tower_lsp::lsp_types::Position,
    new_name: &str,
) -> tower_lsp::lsp_types::WorkspaceEdit {
    use tower_lsp::lsp_types::{
        RenameParams, TextDocumentIdentifier, TextDocumentPositionParams, WorkDoneProgressParams,
    };
    use tower_lsp::LanguageServer;
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    loop {
        let result = h
            .backend
            .rename(RenameParams {
                text_document_position: TextDocumentPositionParams::new(
                    TextDocumentIdentifier::new(h.uri(file)),
                    position,
                ),
                new_name: new_name.to_string(),
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await;
        match result {
            Ok(Some(edit)) => return edit,
            other => assert!(
                tokio::time::Instant::now() < deadline,
                "rename never succeeded: {other:?}"
            ),
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rename_package_from_its_declaration_or_an_import() {
    let h = Harness::start(&[
        ("demo/shapes/Shape.java", SHAPE),
        ("demo/shapes/Circle.java", CIRCLE),
        ("demo/shapes/round/Disc.java", "package demo.shapes.round;\n\nimport demo.shapes.Shape;\n\npublic class Disc implements Shape {\n    public double area() { return 3; }\n}\n"),
        ("demo/app/Main.java", MAIN),
    ])
    .await;
    h.open("demo/app/Main.java").await;
    // Cursor on `shapes` in `import demo.shapes.Circle;`.
    let position = h.position_of("demo/app/Main.java", "shapes.Circle", 1);
    use tower_lsp::LanguageServer;
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    loop {
        let prepared = h
            .backend
            .prepare_rename(tower_lsp::lsp_types::TextDocumentPositionParams::new(
                tower_lsp::lsp_types::TextDocumentIdentifier::new(h.uri("demo/app/Main.java")),
                position,
            ))
            .await
            .expect("prepareRename");
        if prepared.is_some() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the package segment must be renameable"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let edit = rename_at(&h, "demo/app/Main.java", position, "figures").await;
    apply_workspace_edit(&edit);

    assert!(
        !h.exists("demo/shapes"),
        "old directory must be renamed away"
    );
    assert!(h
        .read("demo/figures/Circle.java")
        .starts_with("package demo.figures;"));
    // Subpackages move with the directory and keep their own suffix.
    let disc = h.read("demo/figures/round/Disc.java");
    assert!(disc.starts_with("package demo.figures.round;"), "{disc}");
    assert!(disc.contains("import demo.figures.Shape;"), "{disc}");
    let main = h.read("demo/app/Main.java");
    assert!(main.contains("import demo.figures.Circle;"), "{main}");
    assert!(main.contains("import demo.figures.Shape;"), "{main}");
}
