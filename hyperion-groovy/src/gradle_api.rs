// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Gradle API method definitions
//!
//! This module contains a curated catalog of common Gradle DSL methods.
//!
//! These methods are organized by DSL block (dependencies, repositories, tasks, etc.)

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Metadata about a Gradle DSL method
#[derive(Clone, Debug)]
pub struct GradleMethod {
    /// Method name (e.g., "implementation", "testImplementation")
    pub name: String,
    /// Fully qualified method signature
    pub signature: GradleMethodSignature,
    /// DSL block this method belongs to (e.g., "dependencies", "repositories")
    pub block: String,
    /// Documentation/description
    pub description: String,
}

/// Full method signature for Gradle API
#[derive(Clone, Debug)]
pub struct GradleMethodSignature {
    /// Method name
    pub name: String,
    /// Human-readable parameter types used for completion and hover
    pub params: Vec<String>,
    /// Return type (if any)
    pub return_type: Option<String>,
}

/// Build the Gradle API method registry
fn build_gradle_api() -> HashMap<String, GradleMethod> {
    let mut methods = HashMap::new();

    // ===== DEPENDENCIES BLOCK (10 methods) =====

    methods.insert(
        "implementation".to_string(),
        GradleMethod {
            name: "implementation".to_string(),
            signature: GradleMethodSignature {
                name: "implementation".to_string(),
                params: vec!["dependency: String|Dependency".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Add implementation dependency (visible at runtime and compile time)"
                .to_string(),
        },
    );

    methods.insert(
        "testImplementation".to_string(),
        GradleMethod {
            name: "testImplementation".to_string(),
            signature: GradleMethodSignature {
                name: "testImplementation".to_string(),
                params: vec!["dependency: String|Dependency".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Add test implementation dependency (visible only for tests)".to_string(),
        },
    );

    methods.insert(
        "runtimeOnly".to_string(),
        GradleMethod {
            name: "runtimeOnly".to_string(),
            signature: GradleMethodSignature {
                name: "runtimeOnly".to_string(),
                params: vec!["dependency: String|Dependency".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Add runtime-only dependency (not visible at compile time)".to_string(),
        },
    );

    methods.insert(
        "compileOnly".to_string(),
        GradleMethod {
            name: "compileOnly".to_string(),
            signature: GradleMethodSignature {
                name: "compileOnly".to_string(),
                params: vec!["dependency: String|Dependency".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Add compile-only dependency (not in runtime classpath)".to_string(),
        },
    );

    methods.insert(
        "annotationProcessor".to_string(),
        GradleMethod {
            name: "annotationProcessor".to_string(),
            signature: GradleMethodSignature {
                name: "annotationProcessor".to_string(),
                params: vec!["dependency: String|Dependency".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Add annotation processor dependency".to_string(),
        },
    );

    methods.insert(
        "kapt".to_string(),
        GradleMethod {
            name: "kapt".to_string(),
            signature: GradleMethodSignature {
                name: "kapt".to_string(),
                params: vec!["dependency: String|Dependency".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Add Kotlin annotation processor (Kotlin Annotation Processing Tool)"
                .to_string(),
        },
    );

    methods.insert(
        "api".to_string(),
        GradleMethod {
            name: "api".to_string(),
            signature: GradleMethodSignature {
                name: "api".to_string(),
                params: vec!["dependency: String|Dependency".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Add API dependency (visible to consumers of this library)".to_string(),
        },
    );

    methods.insert(
        "constraints".to_string(),
        GradleMethod {
            name: "constraints".to_string(),
            signature: GradleMethodSignature {
                name: "constraints".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "dependencies".to_string(),
            description: "Define version constraints for transitive dependencies".to_string(),
        },
    );

    for name in [
        "androidTestImplementation",
        "debugImplementation",
        "testRuntimeOnly",
        "ksp",
        "classpath",
    ] {
        methods.insert(
            name.to_string(),
            GradleMethod {
                name: name.to_string(),
                signature: GradleMethodSignature {
                    name: name.to_string(),
                    params: vec!["dependency: String|Dependency".to_string()],
                    return_type: None,
                },
                block: "dependencies".to_string(),
                description: format!("Add a dependency to the {name} configuration"),
            },
        );
    }

    // ===== REPOSITORIES BLOCK (3 methods) =====

    methods.insert(
        "mavenCentral".to_string(),
        GradleMethod {
            name: "mavenCentral".to_string(),
            signature: GradleMethodSignature {
                name: "mavenCentral".to_string(),
                params: vec![],
                return_type: None,
            },
            block: "repositories".to_string(),
            description: "Add Maven Central repository".to_string(),
        },
    );

    methods.insert(
        "mavenLocal".to_string(),
        GradleMethod {
            name: "mavenLocal".to_string(),
            signature: GradleMethodSignature {
                name: "mavenLocal".to_string(),
                params: vec![],
                return_type: None,
            },
            block: "repositories".to_string(),
            description: "Add local Maven repository (~/.m2/repository)".to_string(),
        },
    );

    methods.insert(
        "maven".to_string(),
        GradleMethod {
            name: "maven".to_string(),
            signature: GradleMethodSignature {
                name: "maven".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "repositories".to_string(),
            description: "Add custom Maven repository".to_string(),
        },
    );

    methods.insert(
        "google".to_string(),
        GradleMethod {
            name: "google".to_string(),
            signature: GradleMethodSignature {
                name: "google".to_string(),
                params: vec![],
                return_type: None,
            },
            block: "repositories".to_string(),
            description: "Add Google Maven repository (Android/Google Play Services)".to_string(),
        },
    );

    // ===== TASKS BLOCK & TASK REGISTRATION (4 methods) =====

    methods.insert(
        "register".to_string(),
        GradleMethod {
            name: "register".to_string(),
            signature: GradleMethodSignature {
                name: "register".to_string(),
                params: vec![
                    "taskName: String".to_string(),
                    "taskType: Class".to_string(),
                ],
                return_type: Some("TaskProvider".to_string()),
            },
            block: "tasks".to_string(),
            description: "Register a new task with the given name and type".to_string(),
        },
    );

    methods.insert(
        "create".to_string(),
        GradleMethod {
            name: "create".to_string(),
            signature: GradleMethodSignature {
                name: "create".to_string(),
                params: vec![
                    "taskName: String".to_string(),
                    "taskType: Class".to_string(),
                ],
                return_type: Some("Task".to_string()),
            },
            block: "tasks".to_string(),
            description: "Create a new task (eager evaluation)".to_string(),
        },
    );

    methods.insert(
        "named".to_string(),
        GradleMethod {
            name: "named".to_string(),
            signature: GradleMethodSignature {
                name: "named".to_string(),
                params: vec!["taskName: String".to_string()],
                return_type: Some("TaskProvider".to_string()),
            },
            block: "tasks".to_string(),
            description: "Get provider for existing task by name".to_string(),
        },
    );

    // ===== PLUGINS BLOCK (2 methods) =====

    methods.insert(
        "id".to_string(),
        GradleMethod {
            name: "id".to_string(),
            signature: GradleMethodSignature {
                name: "id".to_string(),
                params: vec!["pluginId: String".to_string()],
                return_type: Some("PluginDependencySpec".to_string()),
            },
            block: "plugins".to_string(),
            description: "Reference a plugin by ID".to_string(),
        },
    );

    methods.insert(
        "kotlin".to_string(),
        GradleMethod {
            name: "kotlin".to_string(),
            signature: GradleMethodSignature {
                name: "kotlin".to_string(),
                params: vec!["version: String".to_string()],
                return_type: Some("PluginDependencySpec".to_string()),
            },
            block: "plugins".to_string(),
            description: "Apply Kotlin plugin".to_string(),
        },
    );

    // ===== TOP-LEVEL BLOCK METHODS (7 methods) =====

    methods.insert(
        "dependencies".to_string(),
        GradleMethod {
            name: "dependencies".to_string(),
            signature: GradleMethodSignature {
                name: "dependencies".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure project dependencies".to_string(),
        },
    );

    methods.insert(
        "repositories".to_string(),
        GradleMethod {
            name: "repositories".to_string(),
            signature: GradleMethodSignature {
                name: "repositories".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure artifact repositories".to_string(),
        },
    );

    methods.insert(
        "tasks".to_string(),
        GradleMethod {
            name: "tasks".to_string(),
            signature: GradleMethodSignature {
                name: "tasks".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure project tasks".to_string(),
        },
    );

    methods.insert(
        "plugins".to_string(),
        GradleMethod {
            name: "plugins".to_string(),
            signature: GradleMethodSignature {
                name: "plugins".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure build plugins".to_string(),
        },
    );

    methods.insert(
        "allprojects".to_string(),
        GradleMethod {
            name: "allprojects".to_string(),
            signature: GradleMethodSignature {
                name: "allprojects".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure all projects in build".to_string(),
        },
    );

    methods.insert(
        "subprojects".to_string(),
        GradleMethod {
            name: "subprojects".to_string(),
            signature: GradleMethodSignature {
                name: "subprojects".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure subprojects only".to_string(),
        },
    );

    methods.insert(
        "apply".to_string(),
        GradleMethod {
            name: "apply".to_string(),
            signature: GradleMethodSignature {
                name: "apply".to_string(),
                params: vec!["map: Map".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Apply a plugin or script".to_string(),
        },
    );

    // ===== COMMON TASK CONFIGURATION (3 methods) =====

    methods.insert(
        "doLast".to_string(),
        GradleMethod {
            name: "doLast".to_string(),
            signature: GradleMethodSignature {
                name: "doLast".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "task".to_string(),
            description: "Add action to run at end of task execution".to_string(),
        },
    );

    methods.insert(
        "doFirst".to_string(),
        GradleMethod {
            name: "doFirst".to_string(),
            signature: GradleMethodSignature {
                name: "doFirst".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "task".to_string(),
            description: "Add action to run at start of task execution".to_string(),
        },
    );

    methods.insert(
        "dependsOn".to_string(),
        GradleMethod {
            name: "dependsOn".to_string(),
            signature: GradleMethodSignature {
                name: "dependsOn".to_string(),
                params: vec!["task: String|Task".to_string()],
                return_type: None,
            },
            block: "task".to_string(),
            description: "Add task dependency".to_string(),
        },
    );

    // ===== ANDROID PLUGIN (COMMON) (2 methods) =====

    methods.insert(
        "android".to_string(),
        GradleMethod {
            name: "android".to_string(),
            signature: GradleMethodSignature {
                name: "android".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure Android build plugin".to_string(),
        },
    );

    methods.insert(
        "compileSdk".to_string(),
        GradleMethod {
            name: "compileSdk".to_string(),
            signature: GradleMethodSignature {
                name: "compileSdk".to_string(),
                params: vec!["version: Int".to_string()],
                return_type: None,
            },
            block: "android".to_string(),
            description: "Set target Android SDK version".to_string(),
        },
    );

    for (name, parameter, block, description) in [
        (
            "compileSdkVersion",
            "version: Int",
            "android",
            "Set the Android compile SDK version (legacy Groovy DSL form)",
        ),
        (
            "defaultConfig",
            "closure: Closure",
            "android",
            "Configure default Android application settings",
        ),
        (
            "applicationId",
            "id: String",
            "android",
            "Set the Android application identifier",
        ),
        (
            "minSdkVersion",
            "version: Int",
            "android",
            "Set the minimum supported Android SDK version",
        ),
        (
            "targetSdkVersion",
            "version: Int",
            "android",
            "Set the target Android SDK version",
        ),
        (
            "minifyEnabled",
            "enabled: Boolean",
            "android",
            "Enable or disable code shrinking for an Android build type",
        ),
        (
            "minSdk",
            "version: Int",
            "android",
            "Set the minimum supported Android SDK version",
        ),
        (
            "targetSdk",
            "version: Int",
            "android",
            "Set the target Android SDK version",
        ),
        (
            "versionCode",
            "code: Int",
            "android",
            "Set the Android application version code",
        ),
        (
            "versionName",
            "name: String",
            "android",
            "Set the Android application version name",
        ),
        (
            "proguardFiles",
            "files: Object...",
            "buildTypes",
            "Configure code-shrinker rule files",
        ),
        (
            "getDefaultProguardFile",
            "fileName: String",
            "buildTypes",
            "Resolve a default Android code-shrinker configuration",
        ),
        (
            "buildFeatures",
            "closure: Closure",
            "android",
            "Configure generated Android build features",
        ),
        (
            "viewBinding",
            "enabled: Boolean",
            "buildFeatures",
            "Enable or disable Android view binding",
        ),
    ] {
        methods.insert(
            name.to_string(),
            GradleMethod {
                name: name.to_string(),
                signature: GradleMethodSignature {
                    name: name.to_string(),
                    params: vec![parameter.to_string()],
                    return_type: None,
                },
                block: block.to_string(),
                description: description.to_string(),
            },
        );
    }

    // ===== BUILD & OUTPUT (2 methods) =====

    methods.insert(
        "buildTypes".to_string(),
        GradleMethod {
            name: "buildTypes".to_string(),
            signature: GradleMethodSignature {
                name: "buildTypes".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "android".to_string(),
            description: "Configure build types (debug, release, etc.)".to_string(),
        },
    );

    for name in ["debug", "release"] {
        methods.insert(
            name.to_string(),
            GradleMethod {
                name: name.to_string(),
                signature: GradleMethodSignature {
                    name: name.to_string(),
                    params: vec!["closure: Closure".to_string()],
                    return_type: None,
                },
                block: "buildTypes".to_string(),
                description: format!("Configure the Android {name} build type"),
            },
        );
    }

    methods.insert(
        "productFlavors".to_string(),
        GradleMethod {
            name: "productFlavors".to_string(),
            signature: GradleMethodSignature {
                name: "productFlavors".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "android".to_string(),
            description: "Configure product flavors".to_string(),
        },
    );

    // ===== SOURCE & BUILD CONFIGURATION (2 methods) =====

    methods.insert(
        "sourceSets".to_string(),
        GradleMethod {
            name: "sourceSets".to_string(),
            signature: GradleMethodSignature {
                name: "sourceSets".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure source sets".to_string(),
        },
    );

    methods.insert(
        "version".to_string(),
        GradleMethod {
            name: "version".to_string(),
            signature: GradleMethodSignature {
                name: "version".to_string(),
                params: vec!["version: String".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Set project version".to_string(),
        },
    );

    // ===== CONFIGURATION & BUILD CACHE (1 method) =====

    methods.insert(
        "configurations".to_string(),
        GradleMethod {
            name: "configurations".to_string(),
            signature: GradleMethodSignature {
                name: "configurations".to_string(),
                params: vec!["closure: Closure".to_string()],
                return_type: None,
            },
            block: "root".to_string(),
            description: "Configure dependency configurations".to_string(),
        },
    );

    methods
}

/// Lazy-initialized Gradle API method registry
pub static GRADLE_API: Lazy<HashMap<String, GradleMethod>> = Lazy::new(build_gradle_api);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradle_api_has_common_methods() {
        let api = &*GRADLE_API;
        assert!(api.contains_key("implementation"));
        assert!(api.contains_key("testImplementation"));
        assert!(api.contains_key("dependencies"));
        assert!(api.contains_key("mavenCentral"));
        assert!(api.contains_key("doLast"));
    }

    #[test]
    fn gradle_methods_have_signatures() {
        let api = &*GRADLE_API;
        let impl_method = api
            .get("implementation")
            .expect("implementation should exist");
        assert_eq!(impl_method.name.as_str(), "implementation");
        assert_eq!(impl_method.block, "dependencies");
        assert!(!impl_method.signature.params.is_empty());
    }
}
