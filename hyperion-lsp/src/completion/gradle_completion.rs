// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle-specific code completion
//!
//! This module provides completion items for Gradle build files:
//! - Dependency methods (implementation, api, testImplementation, etc.)
//! - Repository methods (mavenCentral, google, etc.)
//! - Plugin methods
//! - Task methods
//! - Android block methods
//! - General top-level blocks
//!
//! Methods are sourced from the central GRADLE_API registry in hyperion-groovy.

use hyperion_groovy::GRADLE_API;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat, MarkupContent, MarkupKind,
};
use tracing::debug;

/// The context in which Gradle completion is being requested
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradleCompletionContext {
    /// In dependencies block
    Dependencies,
    /// In repositories block
    Repositories,
    /// In plugins block
    Plugins,
    /// In tasks block
    Tasks,
    /// In android block
    Android,
    /// Top-level or other context
    General,
}

/// Analyze the context at the cursor position in a Gradle file
pub fn analyze_context(text_before_cursor: &str) -> GradleCompletionContext {
    let mut blocks = Vec::new();
    let mut identifier = String::new();
    let mut last_identifier = None;
    let mut quote = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut chars = text_before_cursor.chars().peekable();

    while let Some(character) = chars.next() {
        if line_comment {
            if character == '\n' {
                line_comment = false;
                last_identifier = None;
            }
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            continue;
        }
        if character == '/' && chars.peek() == Some(&'/') {
            line_comment = true;
            continue;
        }

        if character.is_alphanumeric() || character == '_' {
            identifier.push(character);
            continue;
        }
        if !identifier.is_empty() {
            last_identifier = Some(std::mem::take(&mut identifier));
        }

        match character {
            '{' => {
                blocks.push(last_identifier.as_deref().and_then(context_for_block));
                last_identifier = None;
            }
            '}' => {
                blocks.pop();
                last_identifier = None;
            }
            '\n' | ';' => last_identifier = None,
            _ => {}
        }
    }

    blocks
        .into_iter()
        .rev()
        .flatten()
        .next()
        .unwrap_or(GradleCompletionContext::General)
}

fn context_for_block(name: &str) -> Option<GradleCompletionContext> {
    match name {
        "dependencies" => Some(GradleCompletionContext::Dependencies),
        "repositories" => Some(GradleCompletionContext::Repositories),
        "plugins" => Some(GradleCompletionContext::Plugins),
        "tasks" => Some(GradleCompletionContext::Tasks),
        "android" => Some(GradleCompletionContext::Android),
        _ => None,
    }
}

/// Generate Gradle completion items based on context
pub fn completions_for_context(context: GradleCompletionContext) -> Vec<CompletionItem> {
    debug!("COMPLETION: Gradle context = {:?}", context);

    let mut completions = match context {
        GradleCompletionContext::Dependencies => dependency_method_completions(),
        GradleCompletionContext::Repositories => repository_method_completions(),
        GradleCompletionContext::Plugins => plugin_completions(),
        GradleCompletionContext::Tasks => task_method_completions(),
        GradleCompletionContext::Android => android_completions(),
        GradleCompletionContext::General => general_completions(),
    };
    completions.sort_by(|left, right| left.label.cmp(&right.label));
    completions.dedup_by(|left, right| left.label == right.label);
    completions
}

/// Create a completion item from a Gradle API method
fn create_gradle_completion(
    name: &str,
    description: &str,
    block: &str,
    params: &[String],
    return_type: Option<&String>,
) -> CompletionItem {
    // Determine insert text based on method signature
    let insert_text = if params.is_empty() {
        // No params - add () for repository methods like mavenCentral()
        format!("{}()", name)
    } else if params.iter().any(|p| p.contains("Closure")) {
        // Block method - add { } for configuration blocks
        format!("{} {{\n\t$0\n}}", name)
    } else if params.iter().any(|p| p.contains("String")) {
        // String argument - dependency-style completion
        format!("{} '$0'", name)
    } else {
        format!("{}($0)", name)
    };

    // Build documentation markdown
    let mut doc = format!("**{}**\n\n{}", name, description);
    if !params.is_empty() {
        doc.push_str(&format!("\n\n**Parameters:** `{}`", params.join(", ")));
    }
    if let Some(ret) = return_type {
        doc.push_str(&format!("\n\n**Returns:** `{}`", ret));
    }
    doc.push_str(&format!("\n\n**Block:** `{}`", block));

    CompletionItem {
        label: name.to_string(),
        label_details: None,
        kind: Some(CompletionItemKind::METHOD),
        detail: Some(description.to_string()),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc,
        })),
        preselect: None,
        sort_text: Some(format!("1_{name}")),
        filter_text: None,
        insert_text: Some(insert_text),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        insert_text_mode: None,
        text_edit: None,
        additional_text_edits: None,
        command: None,
        commit_characters: None,
        data: None,
        tags: None,
        deprecated: None,
    }
}

/// Get completions from GRADLE_API for a specific block
fn completions_from_api(target_block: &str) -> Vec<CompletionItem> {
    GRADLE_API
        .iter()
        .filter(|(_, method)| method.block == target_block)
        .map(|(name, method)| {
            create_gradle_completion(
                name,
                &method.description,
                &method.block,
                &method.signature.params,
                method.signature.return_type.as_ref(),
            )
        })
        .collect()
}

/// Get completions for dependency methods (implementation, api, testImplementation, etc.)
pub fn dependency_method_completions() -> Vec<CompletionItem> {
    // Get completions from GRADLE_API for the dependencies block
    let mut completions = completions_from_api("dependencies");

    // Add additional common dependency configurations not in the core API
    let extra_configs = [
        ("testCompileOnly", "Test compile-only dependency"),
        ("testRuntimeOnly", "Test runtime-only dependency"),
        (
            "androidTestImplementation",
            "Android test implementation dependency",
        ),
        (
            "androidTestCompileOnly",
            "Android test compile-only dependency",
        ),
        (
            "androidTestRuntimeOnly",
            "Android test runtime-only dependency",
        ),
        ("debugImplementation", "Debug implementation dependency"),
        ("releaseImplementation", "Release implementation dependency"),
    ];

    for (method, detail) in extra_configs {
        completions.push(create_gradle_completion(
            method,
            detail,
            "dependencies",
            &["dependency: String|Dependency".to_string()],
            None,
        ));
    }

    completions
}

/// Get completions for repository methods (mavenCentral, google, etc.)
pub fn repository_method_completions() -> Vec<CompletionItem> {
    // Get completions from GRADLE_API for the repositories block
    let mut completions = completions_from_api("repositories");

    // Add additional common repository methods not in the core API
    let extra_repos = [
        ("google", "Google Maven Repository"),
        ("jcenter", "JCenter Repository (deprecated)"),
        ("ivy", "Ivy Repository"),
        ("flatDir", "Flat Directory Repository"),
        ("gradlePluginPortal", "Gradle Plugin Portal"),
    ];

    for (method, detail) in extra_repos {
        completions.push(create_gradle_completion(
            method,
            detail,
            "repositories",
            &[],
            None,
        ));
    }

    completions
}

/// Get completions for plugin methods
pub fn plugin_completions() -> Vec<CompletionItem> {
    // Get completions from GRADLE_API for the plugins block
    let mut completions = completions_from_api("plugins");

    // Add additional common plugin DSL methods not in the core API
    let extra_plugins = [
        ("java", "Apply Java plugin"),
        ("application", "Apply Application plugin"),
        (
            "java-library",
            "Apply Java Library plugin (adds 'api' configuration)",
        ),
        ("groovy", "Apply Groovy plugin"),
        ("scala", "Apply Scala plugin"),
    ];

    for (method, detail) in extra_plugins {
        completions.push(create_gradle_completion(
            method,
            detail,
            "plugins",
            &["pluginId: String".to_string()],
            Some(&"PluginDependencySpec".to_string()),
        ));
    }

    completions
}

/// Get completions for task methods (register, create, etc.)
pub fn task_method_completions() -> Vec<CompletionItem> {
    // Get completions from GRADLE_API for the tasks block
    let mut completions = completions_from_api("tasks");

    // Also include task configuration methods
    completions.extend(completions_from_api("task"));

    // Add additional common task methods not in the core API
    let extra_task_methods = [
        ("existing", "Reference an existing task"),
        ("withType", "Configure all tasks of a type"),
        ("all", "Configure all tasks"),
        ("getByName", "Get a task by name"),
    ];

    for (method, detail) in extra_task_methods {
        completions.push(create_gradle_completion(
            method,
            detail,
            "tasks",
            &["taskName: String".to_string()],
            Some(&"TaskProvider".to_string()),
        ));
    }

    completions
}

/// Get completions for Android block methods
pub fn android_completions() -> Vec<CompletionItem> {
    // Get completions from GRADLE_API for the android block
    let mut completions = completions_from_api("android");

    // Add additional common Android DSL methods not in the core API
    let extra_android_methods = [
        ("compileSdkVersion", "Set the target API level", false),
        ("minSdkVersion", "Set the minimum API level", false),
        ("targetSdkVersion", "Set the target API level", false),
        ("defaultConfig", "Default build configuration", true),
        ("buildFeatures", "Enable/disable build features", true),
        ("flavorDimensions", "Define flavor dimensions", false),
        ("signingConfigs", "Define signing configurations", true),
        ("compileOptions", "Compiler options", true),
        ("kotlinOptions", "Kotlin compiler options", true),
        ("packagingOptions", "Packaging options", true),
        ("splits", "APK splits configuration", true),
        ("testOptions", "Test configuration options", true),
    ];

    for (method, detail, is_block) in extra_android_methods {
        let params = if is_block {
            vec!["closure: Closure".to_string()]
        } else {
            vec!["version: Int".to_string()]
        };
        completions.push(create_gradle_completion(
            method, detail, "android", &params, None,
        ));
    }

    completions
}

/// Get general Gradle completions (top-level blocks)
pub fn general_completions() -> Vec<CompletionItem> {
    // Get completions from GRADLE_API for the root block
    let mut completions = completions_from_api("root");

    // Add additional common top-level blocks not in the core API
    let extra_blocks = [
        ("buildscript", "Buildscript configuration (deprecated)"),
        ("artifacts", "Define artifacts"),
        ("ext", "Define project extensions"),
        ("group", "Set project group"),
        ("description", "Set project description"),
    ];

    for (keyword, detail) in extra_blocks {
        completions.push(create_gradle_completion(
            keyword,
            detail,
            "root",
            &["closure: Closure".to_string()],
            None,
        ));
    }

    // Set kind to KEYWORD for top-level blocks (they're more like DSL keywords)
    for item in &mut completions {
        item.kind = Some(CompletionItemKind::KEYWORD);
        // Boost priority for top-level blocks
        if let Some(ref label) = item.sort_text.clone() {
            item.sort_text = Some(label.replace("1_", "0_"));
        }
    }

    completions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balances_blocks_and_ignores_braces_in_strings_and_comments() {
        assert_eq!(
            analyze_context(
                r#"dependencies {
    implementation "example:{artifact}:1"
}
// repositories {
"#,
            ),
            GradleCompletionContext::General
        );
        assert_eq!(
            analyze_context(
                r#"android {
    buildTypes {
        release {
"#,
            ),
            GradleCompletionContext::Android
        );
    }

    #[test]
    fn completion_labels_are_unique_and_deterministic() {
        for context in [
            GradleCompletionContext::Dependencies,
            GradleCompletionContext::Repositories,
            GradleCompletionContext::Plugins,
            GradleCompletionContext::Tasks,
            GradleCompletionContext::Android,
            GradleCompletionContext::General,
        ] {
            let completions = completions_for_context(context);
            let labels = completions
                .iter()
                .map(|completion| completion.label.as_str())
                .collect::<Vec<_>>();
            assert!(
                labels.windows(2).all(|pair| pair[0] < pair[1]),
                "{context:?} completion labels must be unique and sorted: {labels:?}"
            );
        }
    }
}
