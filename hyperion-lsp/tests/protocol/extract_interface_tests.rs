// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Extract interface through the real server.

use super::refactor_harness::{apply_workspace_edit, find_action, Harness};

const PROBE: &str = "package demo;\n\nimport java.util.List;\n\n/** Probes things. */\npublic class Probe implements Runnable {\n    private int hidden;\n\n    public Probe() {}\n\n    public List<String> names() throws java.io.IOException {\n        return List.of();\n    }\n\n    public static Probe of() { return new Probe(); }\n\n    @Override\n    public void run() {}\n\n    void packagePrivate() {}\n}\n";

#[tokio::test(flavor = "multi_thread")]
async fn extract_interface_creates_its_own_file_when_the_client_can_create_files() {
    let h = Harness::start(&[("demo/Probe.java", PROBE)]).await;
    h.open("demo/Probe.java").await;
    let position = h.position_of("demo/Probe.java", "class Probe", 2);
    let actions = h
        .code_actions_until("demo/Probe.java", position, |actions| {
            find_action(actions, "Extract interface 'IProbe'").is_some()
        })
        .await;
    let action = find_action(&actions, "Extract interface 'IProbe'").unwrap();
    apply_workspace_edit(action.edit.as_ref().unwrap());

    let interface = h.read("demo/IProbe.java");
    assert_eq!(
        interface,
        "package demo;\n\nimport java.util.List;\n\npublic interface IProbe {\n    List<String> names() throws java.io.IOException;\n    void run();\n}\n"
    );
    let class = h.read("demo/Probe.java");
    assert!(
        class.contains("public class Probe implements Runnable, IProbe {"),
        "{class}"
    );
    assert!(!class.contains("public public"), "{class}");
}

#[tokio::test(flavor = "multi_thread")]
async fn extract_interface_without_create_support_stays_in_the_file_and_is_valid() {
    let h = Harness::start_with_resource_operations(&[("demo/Probe.java", PROBE)], &[]).await;
    h.open("demo/Probe.java").await;
    let position = h.position_of("demo/Probe.java", "class Probe", 2);
    let actions = h
        .code_actions_until("demo/Probe.java", position, |actions| {
            find_action(actions, "Extract interface 'IProbe'").is_some()
        })
        .await;
    let action = find_action(&actions, "Extract interface 'IProbe'").unwrap();
    apply_workspace_edit(action.edit.as_ref().unwrap());
    let text = h.read("demo/Probe.java");
    assert!(!text.contains("public public"), "{text}");
    assert!(
        text.contains("interface IProbe {\n    List<String> names() throws java.io.IOException;\n    void run();\n}\n\n/** Probes things. */\npublic class Probe implements Runnable, IProbe {"),
        "{text}"
    );
    // Only one public top-level type may remain in the file.
    assert_eq!(
        text.matches("public class").count() + text.matches("public interface").count(),
        1,
        "{text}"
    );
}
