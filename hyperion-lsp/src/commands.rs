// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP command identifiers and parsing helpers.

use serde_json::Value;

pub const RUN_COMMAND: &str = "hyperion.run";
pub const RUN_TEST_COMMAND: &str = "hyperion.runTest";
pub const SHOW_IMPLEMENTATIONS_COMMAND: &str = "hyperion.showImplementations";
pub const RUN_CONFIGS_COMMAND: &str = "hyperion.runConfigurations";
pub const SAFE_DELETE_COMMAND: &str = "hyperion.safeDelete";
/// Re-read the Gradle/Maven model (and, when the user allowed it, re-run the
/// build tool to resolve classpaths).
pub const RELOAD_PROJECT_COMMAND: &str = "hyperion.reloadProject";
/// Resolve how to build and launch (or test) the entry point at a position.
pub const RESOLVE_LAUNCH_COMMAND: &str = "hyperion.resolveLaunch";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    /// "▶ Run" lens on a `public static void main(String[] args)` method.
    /// `uri` is the originating document, injected into the lens's command
    /// arguments by `handle_code_lens` (the `CodeLens`/`Command` payload
    /// itself only carries the class name).
    Run {
        class_name: Option<String>,
        uri: Option<String>,
    },
    /// Legacy `hyperion.runTest` invocation from clients that cached an old
    /// lens. Hyperion keeps parsing this command so it can report the feature
    /// as unsupported, but no longer advertises or emits it.
    RunTest {
        method_name: Option<String>,
    },
    ShowImplementations {
        class_name: Option<String>,
    },
    RunConfigurations,
    ReloadProject,
    /// `uri`, optional LSP `position` and `target` (`auto`/`main`/`test`).
    ResolveLaunch {
        uri: Option<String>,
        position: Option<(u32, u32)>,
        target: Option<String>,
    },
    SafeDelete {
        uri: Option<String>,
        line: Option<u32>,
        character: Option<u32>,
        version: Option<i32>,
    },
}

pub fn supported_commands() -> Vec<String> {
    vec![
        RUN_COMMAND.to_string(),
        RUN_CONFIGS_COMMAND.to_string(),
        SAFE_DELETE_COMMAND.to_string(),
        RELOAD_PROJECT_COMMAND.to_string(),
        RESOLVE_LAUNCH_COMMAND.to_string(),
    ]
}

pub fn parse_command(command: &str, args: Option<&[Value]>) -> Option<CommandAction> {
    match command {
        RUN_COMMAND => Some(CommandAction::Run {
            class_name: extract_arg_string(args, "className"),
            uri: extract_arg_string(args, "uri"),
        }),
        RUN_TEST_COMMAND => Some(CommandAction::RunTest {
            method_name: extract_arg_string(args, "methodName"),
        }),
        SHOW_IMPLEMENTATIONS_COMMAND => Some(CommandAction::ShowImplementations {
            class_name: extract_arg_string(args, "className"),
        }),
        RUN_CONFIGS_COMMAND => Some(CommandAction::RunConfigurations),
        RELOAD_PROJECT_COMMAND => Some(CommandAction::ReloadProject),
        RESOLVE_LAUNCH_COMMAND => Some(CommandAction::ResolveLaunch {
            uri: extract_arg_string(args, "uri"),
            position: extract_position(args),
            target: extract_arg_string(args, "target"),
        }),
        SAFE_DELETE_COMMAND => Some(CommandAction::SafeDelete {
            uri: extract_arg_string(args, "uri"),
            line: extract_arg_u32(args, "line"),
            character: extract_arg_u32(args, "character"),
            version: extract_arg_i32(args, "version"),
        }),
        _ => None,
    }
}

fn extract_position(args: Option<&[Value]>) -> Option<(u32, u32)> {
    let position = args?.first()?.as_object()?.get("position")?.as_object()?;
    let line = u32::try_from(position.get("line")?.as_u64()?).ok()?;
    let character = u32::try_from(position.get("character")?.as_u64()?).ok()?;
    Some((line, character))
}

fn extract_arg_u32(args: Option<&[Value]>, key: &str) -> Option<u32> {
    u32::try_from(args?.first()?.as_object()?.get(key)?.as_u64()?).ok()
}

fn extract_arg_i32(args: Option<&[Value]>, key: &str) -> Option<i32> {
    i32::try_from(args?.first()?.as_object()?.get(key)?.as_i64()?).ok()
}

fn extract_arg_string(args: Option<&[Value]>, key: &str) -> Option<String> {
    let obj = args?.first()?.as_object()?;
    obj.get(key)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_commands() {
        let commands = supported_commands();
        assert!(commands.contains(&RUN_COMMAND.to_string()));
        assert!(!commands.contains(&RUN_TEST_COMMAND.to_string()));
        assert!(!commands.contains(&SHOW_IMPLEMENTATIONS_COMMAND.to_string()));
        assert!(commands.contains(&RUN_CONFIGS_COMMAND.to_string()));
        assert!(commands.contains(&SAFE_DELETE_COMMAND.to_string()));
        assert!(commands.contains(&RELOAD_PROJECT_COMMAND.to_string()));
        assert!(commands.contains(&RESOLVE_LAUNCH_COMMAND.to_string()));
        assert_eq!(
            parse_command(RELOAD_PROJECT_COMMAND, None),
            Some(CommandAction::ReloadProject)
        );
    }

    #[test]
    fn test_parse_legacy_run_test_command_ignores_obsolete_execution_fields() {
        let args = vec![serde_json::json!({
            "methodName": "testFoo",
            "className": "MyTest",
            "hasMain": false,
            "uri": "file:///workspace/MyTest.java",
        })];
        let parsed = parse_command(RUN_TEST_COMMAND, Some(&args));
        assert_eq!(
            parsed,
            Some(CommandAction::RunTest {
                method_name: Some("testFoo".to_string()),
            })
        );
    }

    #[test]
    fn test_parse_legacy_run_test_command_without_optional_fields() {
        let args = vec![serde_json::json!({ "methodName": "testFoo" })];
        let parsed = parse_command(RUN_TEST_COMMAND, Some(&args));
        assert_eq!(
            parsed,
            Some(CommandAction::RunTest {
                method_name: Some("testFoo".to_string()),
            })
        );
    }

    #[test]
    fn test_parse_run_command() {
        let args = vec![serde_json::json!({
            "className": "Main",
            "uri": "file:///workspace/Main.java",
        })];
        let parsed = parse_command(RUN_COMMAND, Some(&args));
        assert_eq!(
            parsed,
            Some(CommandAction::Run {
                class_name: Some("Main".to_string()),
                uri: Some("file:///workspace/Main.java".to_string()),
            })
        );
    }

    #[test]
    fn test_parse_safe_delete_command() {
        let args = vec![serde_json::json!({
            "uri": "file:///workspace/Entity.java",
            "line": 4,
            "character": 12,
            "version": 7,
        })];
        assert_eq!(
            parse_command(SAFE_DELETE_COMMAND, Some(&args)),
            Some(CommandAction::SafeDelete {
                uri: Some("file:///workspace/Entity.java".to_string()),
                line: Some(4),
                character: Some(12),
                version: Some(7),
            })
        );
    }

    #[test]
    fn test_parse_show_implementations_command() {
        let args = vec![serde_json::json!({ "className": "MyInterface" })];
        let parsed = parse_command(SHOW_IMPLEMENTATIONS_COMMAND, Some(&args));
        assert_eq!(
            parsed,
            Some(CommandAction::ShowImplementations {
                class_name: Some("MyInterface".to_string())
            })
        );
    }
}
