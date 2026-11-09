// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! IntelliJ run configuration parser.
//!
//! Loads and parses `.idea/runConfigurations/*.xml` and `.run/*.xml` files,
//! supporting `Remote`, `Application`, and `GradleRunConfiguration` types.
//! Returns launch.json-shaped `serde_json::Value` entries.

use std::path::Path;

use serde_json::{json, Value};

const MAX_RUN_CONFIG_BYTES: usize = 4 * 1024 * 1024;

/// Discover and parse all IntelliJ run configurations under `project_root`.
///
/// Searches both `.idea/runConfigurations/*.xml` and `.run/*.xml`.
/// Returns launch.json-shaped JSON values suitable for DAP clients.
pub fn discover_run_configs(project_root: &Path) -> Vec<Value> {
    let dirs = [
        project_root.join(".idea/runConfigurations"),
        project_root.join(".run"),
    ];
    let mut configs = Vec::new();
    for dir in &dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "xml") {
                    if let Ok(content) =
                        hyperion_workspace::read_utf8_file_bounded(&path, MAX_RUN_CONFIG_BYTES)
                    {
                        if let Some(config) = parse_run_config_str(&content, project_root) {
                            configs.push(config);
                        }
                    }
                }
            }
        }
    }
    configs
}

/// Parse an IntelliJ run configuration from an XML string, returning a
/// launch.json-shaped JSON value.
fn parse_run_config_str(content: &str, project_root: &Path) -> Option<Value> {
    let doc = roxmltree::Document::parse(content).ok()?;
    let config_elem = doc
        .descendants()
        .find(|n| n.has_tag_name("configuration"))?;
    let name = config_elem.attribute("name")?;
    match config_elem.attribute("type")? {
        "Remote" => parse_remote_config(config_elem, name),
        "Application" => parse_application_config(config_elem, name, project_root),
        "GradleRunConfiguration" => parse_gradle_config(config_elem, name, project_root),
        _ => None,
    }
}

/// Extract `<option name="key" value="val" />` from direct children of `parent`.
fn option_value<'a>(parent: roxmltree::Node<'a, '_>, key: &str) -> Option<&'a str> {
    parent
        .children()
        .filter(|n| n.has_tag_name("option"))
        .find(|n| n.attribute("name") == Some(key))
        .and_then(|n| n.attribute("value"))
}

fn parse_remote_config(config: roxmltree::Node, name: &str) -> Option<Value> {
    let host = option_value(config, "HOST").unwrap_or("127.0.0.1");
    let port: u16 = option_value(config, "PORT")
        .unwrap_or("5005")
        .parse()
        .ok()?;
    Some(json!({
        "name": name,
        "type": "java",
        "request": "attach",
        "host": host,
        "port": port,
    }))
}

fn parse_application_config(
    config: roxmltree::Node,
    name: &str,
    project_root: &Path,
) -> Option<Value> {
    let main_class = option_value(config, "MAIN_CLASS_NAME")?;
    let jvm_args: Vec<String> = option_value(config, "VM_PARAMETERS")
        .map(shell_split)
        .unwrap_or_default();
    let args: Vec<String> = option_value(config, "PROGRAM_PARAMETERS")
        .map(shell_split)
        .unwrap_or_default();
    let cwd = option_value(config, "WORKING_DIRECTORY").map_or_else(
        || project_root.display().to_string(),
        |w| resolve_project_dir(w, project_root),
    );
    Some(json!({
        "name": name,
        "type": "java",
        "request": "launch",
        "mainClass": main_class,
        "jvmArgs": jvm_args,
        "args": args,
        "cwd": cwd,
    }))
}

fn parse_gradle_config(config: roxmltree::Node, name: &str, project_root: &Path) -> Option<Value> {
    let settings = config
        .descendants()
        .find(|n| n.has_tag_name("ExternalSystemSettings"));
    let tasks = settings.map(extract_task_names).unwrap_or_default();
    let task = extract_first_task_name(&tasks);
    let args: Vec<String> = settings
        .and_then(|s| option_value(s, "scriptParameters"))
        .map(shell_split)
        .unwrap_or_default();
    let project_path = settings
        .and_then(|s| option_value(s, "externalProjectPath"))
        .map_or_else(
            || project_root.display().to_string(),
            |p| resolve_project_dir(p, project_root),
        );
    Some(json!({
        "name": name,
        "type": "java",
        "request": "gradle",
        "task": task,
        "args": args,
        "projectPath": project_path,
    }))
}

/// Extract task names from the nested Gradle XML structure:
/// ```xml
/// <ExternalSystemSettings>
///   <option name="taskNames">
///     <list>
///       <option value=":test" />
///     </list>
///   </option>
/// </ExternalSystemSettings>
/// ```
fn extract_task_names(settings: roxmltree::Node) -> Vec<String> {
    let task_names_option = settings
        .children()
        .filter(|n| n.has_tag_name("option"))
        .find(|n| n.attribute("name") == Some("taskNames"));
    let Some(task_names_option) = task_names_option else {
        return Vec::new();
    };
    let Some(list) = task_names_option
        .children()
        .find(|n| n.has_tag_name("list"))
    else {
        return Vec::new();
    };
    list.children()
        .filter(|n| n.has_tag_name("option"))
        .filter_map(|n| n.attribute("value").map(String::from))
        .collect()
}

/// Combine task names into a single task string (e.g. `"clean build"` or `":test"`).
fn extract_first_task_name(tasks: &[String]) -> String {
    tasks.join(" ")
}

/// Replace `$PROJECT_DIR$` with the actual project root path.
fn resolve_project_dir(value: &str, project_root: &Path) -> String {
    value.replace("$PROJECT_DIR$", &project_root.display().to_string())
}

/// Split a shell-like argument string respecting quotes and backslash escapes.
///
/// IntelliJ stores arguments as a single string (e.g. `--tests "SomeTest" -Dfoo=bar`).
/// `roxmltree` handles XML entity unescaping (`&quot;` → `"`), so we handle the
/// post-XML-unescaped string here.
///
/// Supports:
/// - Double and single quoted regions
/// - `\"` → literal `"`, `\\` → literal `\`, `\ ` → literal space (outside quotes)
/// - Inside double quotes, `\"` and `\\` are recognized escapes
/// - Inside single quotes, everything is literal (no escape processing)
fn shell_split(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while let Some(c) = chars.next() {
        match c {
            '\\' if !in_single_quote => {
                // Backslash escape: consume the next character literally.
                if let Some(next) = chars.next() {
                    current.push(next);
                } else {
                    current.push('\\');
                }
            }
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            c if c.is_whitespace() && !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from("/test/project")
    }

    #[test]
    fn remote_config_basic() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="Debug Backend" type="Remote" factoryName="Remote">
            <option name="HOST" value="localhost" />
            <option name="PORT" value="5005" />
          </configuration>
        </component>
        "#;
        let config = parse_run_config_str(xml, &root()).unwrap();
        assert_eq!(config["name"], "Debug Backend");
        assert_eq!(config["type"], "java");
        assert_eq!(config["request"], "attach");
        assert_eq!(config["host"], "localhost");
        assert_eq!(config["port"], 5005);
    }

    #[test]
    fn remote_config_defaults() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="Attach" type="Remote" factoryName="Remote">
          </configuration>
        </component>
        "#;
        let config = parse_run_config_str(xml, &root()).unwrap();
        assert_eq!(config["host"], "127.0.0.1");
        assert_eq!(config["port"], 5005);
    }

    #[test]
    fn remote_config_custom_port() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="ES Debug" type="Remote" factoryName="Remote">
            <option name="HOST" value="192.168.1.100" />
            <option name="PORT" value="8787" />
          </configuration>
        </component>
        "#;
        let config = parse_run_config_str(xml, &root()).unwrap();
        assert_eq!(config["host"], "192.168.1.100");
        assert_eq!(config["port"], 8787);
    }

    #[test]
    fn application_config_full() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="Run Main" type="Application" factoryName="Application">
            <option name="MAIN_CLASS_NAME" value="com.example.Main" />
            <option name="VM_PARAMETERS" value="-Xmx512m -Dfoo=bar" />
            <option name="PROGRAM_PARAMETERS" value="--port 8080" />
            <option name="WORKING_DIRECTORY" value="$PROJECT_DIR$" />
          </configuration>
        </component>
        "#;
        let config = parse_run_config_str(xml, &root()).unwrap();
        assert_eq!(config["name"], "Run Main");
        assert_eq!(config["request"], "launch");
        assert_eq!(config["mainClass"], "com.example.Main");
        assert_eq!(config["jvmArgs"], json!(["-Xmx512m", "-Dfoo=bar"]));
        assert_eq!(config["args"], json!(["--port", "8080"]));
        assert_eq!(config["cwd"], "/test/project");
    }

    #[test]
    fn application_config_missing_main_class() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="No Main" type="Application" factoryName="Application">
            <option name="VM_PARAMETERS" value="-Xmx512m" />
          </configuration>
        </component>
        "#;
        assert!(parse_run_config_str(xml, &root()).is_none());
    }

    #[test]
    fn gradle_config_nested_tasks() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="Run Tests" type="GradleRunConfiguration" factoryName="Gradle">
            <ExternalSystemSettings>
              <option name="externalProjectPath" value="/home/user/project" />
              <option name="scriptParameters" value="--tests &quot;com.example.MyTest&quot; --info" />
              <option name="taskNames">
                <list>
                  <option value=":test" />
                </list>
              </option>
            </ExternalSystemSettings>
          </configuration>
        </component>
        "#;
        let config = parse_run_config_str(xml, &root()).unwrap();
        assert_eq!(config["name"], "Run Tests");
        assert_eq!(config["request"], "gradle");
        assert_eq!(config["task"], ":test");
        assert_eq!(
            config["args"],
            json!(["--tests", "com.example.MyTest", "--info"])
        );
        assert_eq!(config["projectPath"], "/home/user/project");
    }

    #[test]
    fn gradle_config_multiple_tasks() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="Build All" type="GradleRunConfiguration" factoryName="Gradle">
            <ExternalSystemSettings>
              <option name="taskNames">
                <list>
                  <option value="clean" />
                  <option value="build" />
                </list>
              </option>
            </ExternalSystemSettings>
          </configuration>
        </component>
        "#;
        let config = parse_run_config_str(xml, &root()).unwrap();
        assert_eq!(config["task"], "clean build");
        assert_eq!(config["args"], json!([]));
        assert_eq!(config["projectPath"], "/test/project");
    }

    #[test]
    fn unknown_type_returns_none() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="Docker" type="docker-deploy" factoryName="Docker">
            <option name="something" value="else" />
          </configuration>
        </component>
        "#;
        assert!(parse_run_config_str(xml, &root()).is_none());
    }

    #[test]
    fn missing_type_returns_none() {
        let xml = r#"
        <component name="ProjectRunConfigurationManager">
          <configuration name="NoType">
          </configuration>
        </component>
        "#;
        assert!(parse_run_config_str(xml, &root()).is_none());
    }

    #[test]
    fn shell_split_basic() {
        assert_eq!(shell_split("a b c"), vec!["a", "b", "c"]);
    }

    #[test]
    fn shell_split_double_quotes() {
        assert_eq!(
            shell_split(r#"--tests "com.example.MyTest" --info"#),
            vec!["--tests", "com.example.MyTest", "--info"]
        );
    }

    #[test]
    fn shell_split_single_quotes() {
        assert_eq!(
            shell_split("--name 'hello world' --flag"),
            vec!["--name", "hello world", "--flag"]
        );
    }

    #[test]
    fn shell_split_empty() {
        assert!(shell_split("").is_empty());
        assert!(shell_split("   ").is_empty());
    }

    #[test]
    fn shell_split_preserves_inner_quotes() {
        assert_eq!(
            shell_split(r#"-Dprop="value with spaces""#),
            vec!["-Dprop=value with spaces"]
        );
    }

    #[test]
    fn shell_split_escaped_double_quote() {
        assert_eq!(shell_split(r#"--name=\"foo\""#), vec![r#"--name="foo""#]);
    }

    #[test]
    fn shell_split_escaped_quote_inside_quotes() {
        assert_eq!(
            shell_split(r#""hello \"world\"""#),
            vec![r#"hello "world""#]
        );
    }

    #[test]
    fn shell_split_escaped_backslash() {
        assert_eq!(shell_split(r"path\\to\\dir"), vec![r"path\to\dir"]);
    }

    #[test]
    fn shell_split_escaped_space() {
        assert_eq!(shell_split(r"hello\ world foo"), vec!["hello world", "foo"]);
    }
}
