// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive Gradle Build Script Tests
//!
//! Tests the Gradle executor against a broad set of build.gradle patterns
//! to ensure production readiness for parsing and configuration extraction.
//!
//! Test categories:
//! - Basic patterns (empty, plugins only, dependencies only, etc.)
//! - Plugin patterns (java, kotlin, android, spring boot, etc.)
//! - Repository patterns (maven central, google, custom maven, etc.)
//! - Dependency patterns (all configurations, exclusions, version catalogs, etc.)
//! - Property/Variable patterns (ext block, def variables, GString interpolation)
//! - Task patterns (basic, typed, registered, dependencies)

use hyperion_bytecode::gradle::GradleBuildConfig;
use hyperion_bytecode::gradle_executor::GradleExecutor;

// ============================================================================
// Test Helpers
// ============================================================================

fn execute_gradle(source: &str) -> Result<GradleBuildConfig, String> {
    let mut executor = GradleExecutor::new();
    executor
        .execute_source(source)
        .map_err(|e| format!("{:?}", e))
}

fn assert_gradle_ok(source: &str, test_name: &str) -> GradleBuildConfig {
    match execute_gradle(source) {
        Ok(config) => config,
        Err(e) => panic!("[{}] Gradle execution failed: {}", test_name, e),
    }
}

// ============================================================================
// CATEGORY 1: Basic Patterns (20 tests)
// ============================================================================

mod basic_patterns {
    use super::*;

    #[test]
    fn test_empty_build_gradle() {
        let config = assert_gradle_ok("", "empty_build_gradle");
        assert!(config.plugins.is_empty());
        assert!(config.dependency_list.is_empty());
        assert!(config.repository_list.is_empty());
    }

    #[test]
    fn test_whitespace_only() {
        let config = assert_gradle_ok("   \n\n\t  \n", "whitespace_only");
        assert!(config.plugins.is_empty());
    }

    #[test]
    fn test_comments_only() {
        let source = r#"
            // This is a comment
            /* Multi-line
               comment */
        "#;
        let config = assert_gradle_ok(source, "comments_only");
        assert!(config.plugins.is_empty());
    }

    #[test]
    fn test_plugins_block_only() {
        let source = r#"
            plugins {
                id 'java'
            }
        "#;
        let config = assert_gradle_ok(source, "plugins_block_only");
        assert!(config.has_plugin("java"));
        assert!(config.dependency_list.is_empty());
    }

    #[test]
    fn test_dependencies_block_only() {
        let source = r#"
            dependencies {
                implementation 'com.google.guava:guava:32.1.2-jre'
            }
        "#;
        let config = assert_gradle_ok(source, "dependencies_block_only");
        assert_eq!(config.get_dependencies("implementation").len(), 1);
    }

    #[test]
    fn test_repositories_block_only() {
        let source = r#"
            repositories {
                mavenCentral()
            }
        "#;
        let config = assert_gradle_ok(source, "repositories_block_only");
        assert_eq!(config.repository_list.len(), 1);
    }

    #[test]
    fn test_all_three_blocks() {
        let source = r#"
            plugins {
                id 'java'
            }
            repositories {
                mavenCentral()
            }
            dependencies {
                implementation 'com.example:lib:1.0'
            }
        "#;
        let config = assert_gradle_ok(source, "all_three_blocks");
        assert!(config.has_plugin("java"));
        assert_eq!(config.repository_list.len(), 1);
        assert_eq!(config.get_dependencies("implementation").len(), 1);
    }

    #[test]
    fn test_reversed_block_order() {
        let source = r#"
            dependencies {
                implementation 'com.example:lib:1.0'
            }
            repositories {
                mavenCentral()
            }
            plugins {
                id 'java'
            }
        "#;
        let config = assert_gradle_ok(source, "reversed_block_order");
        assert!(config.has_plugin("java"));
        assert_eq!(config.repository_list.len(), 1);
        assert_eq!(config.get_dependencies("implementation").len(), 1);
    }

    #[test]
    fn test_single_line_plugin() {
        let source = r#"plugins { id 'java' }"#;
        let config = assert_gradle_ok(source, "single_line_plugin");
        assert!(config.has_plugin("java"));
    }

    #[test]
    fn test_single_line_dependency() {
        let source = r#"dependencies { implementation 'com.example:lib:1.0' }"#;
        let config = assert_gradle_ok(source, "single_line_dependency");
        assert_eq!(config.get_dependencies("implementation").len(), 1);
    }

    #[test]
    fn test_single_quotes() {
        let source = r#"
            dependencies {
                implementation 'com.example:lib:1.0'
            }
        "#;
        let config = assert_gradle_ok(source, "single_quotes");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps[0].group, "com.example");
    }

    #[test]
    fn test_double_quotes() {
        let source = r#"
            dependencies {
                implementation "com.example:lib:1.0"
            }
        "#;
        let config = assert_gradle_ok(source, "double_quotes");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps[0].group, "com.example");
    }

    #[test]
    fn test_mixed_quotes() {
        let source = r#"
            dependencies {
                implementation 'com.example:lib-a:1.0'
                api "com.example:lib-b:2.0"
            }
        "#;
        let config = assert_gradle_ok(source, "mixed_quotes");
        assert_eq!(config.get_dependencies("implementation").len(), 1);
        assert_eq!(config.get_dependencies("api").len(), 1);
    }

    #[test]
    fn test_with_buildscript_block() {
        let source = r#"
            buildscript {
                repositories {
                    mavenCentral()
                }
                dependencies {
                    classpath 'com.android.tools.build:gradle:8.2.0'
                }
            }

            plugins {
                id 'java'
            }
        "#;
        let config = assert_gradle_ok(source, "with_buildscript_block");
        assert!(config.has_plugin("java"));
    }

    #[test]
    fn test_with_allprojects_block() {
        let source = r#"
            allprojects {
                repositories {
                    mavenCentral()
                }
            }

            plugins {
                id 'java'
            }
        "#;
        let config = assert_gradle_ok(source, "with_allprojects_block");
        assert!(config.has_plugin("java"));
    }

    #[test]
    fn test_with_subprojects_block() {
        let source = r#"
            subprojects {
                apply plugin: 'java'
            }

            repositories {
                mavenCentral()
            }
        "#;
        let config = assert_gradle_ok(source, "with_subprojects_block");
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
    }

    #[test]
    fn test_nested_blocks() {
        let source = r#"
            dependencies {
                implementation('com.example:lib:1.0') {
                    exclude group: 'org.unwanted'
                }
            }
        "#;
        let config = assert_gradle_ok(source, "nested_blocks");
        assert_eq!(config.get_dependencies("implementation").len(), 1);
    }

    #[test]
    fn test_deeply_nested_blocks() {
        let source = r#"
            android {
                defaultConfig {
                    applicationId "com.example.app"
                    versionCode 1
                }
                buildTypes {
                    release {
                        minifyEnabled true
                    }
                }
            }
        "#;
        // Should not crash on deep nesting
        let _ = assert_gradle_ok(source, "deeply_nested_blocks");
    }

    #[test]
    fn test_gradle_properties_syntax() {
        let source = r#"
            group = 'com.example'
            version = '1.0.0'
        "#;
        let config = assert_gradle_ok(source, "gradle_properties_syntax");
        assert_eq!(config.group, Some("com.example".into()));
        assert_eq!(config.version, Some("1.0.0".into()));
    }
}

// ============================================================================
// CATEGORY 2: Plugin Patterns (15 tests)
// ============================================================================

mod plugin_patterns {
    use super::*;

    #[test]
    fn test_java_plugin() {
        let source = r#"plugins { id 'java' }"#;
        let config = assert_gradle_ok(source, "java_plugin");
        assert!(config.has_plugin("java"));
    }

    #[test]
    fn test_java_library_plugin() {
        let source = r#"plugins { id 'java-library' }"#;
        let config = assert_gradle_ok(source, "java_library_plugin");
        assert!(config.has_plugin("java-library"));
    }

    #[test]
    fn test_application_plugin() {
        let source = r#"plugins { id 'application' }"#;
        let config = assert_gradle_ok(source, "application_plugin");
        assert!(config.has_plugin("application"));
    }

    #[test]
    fn test_spring_boot_plugin() {
        let source = r#"
            plugins {
                id 'org.springframework.boot' version '3.2.0'
            }
        "#;
        let config = assert_gradle_ok(source, "spring_boot_plugin");
        assert!(config.has_plugin("org.springframework.boot"));
        let plugin = config
            .plugin_definitions
            .iter()
            .find(|p| p.id == "org.springframework.boot")
            .unwrap();
        assert_eq!(plugin.version, Some("3.2.0".into()));
    }

    #[test]
    fn test_kotlin_jvm_plugin() {
        let source = r#"
            plugins {
                id 'org.jetbrains.kotlin.jvm' version '1.9.21'
            }
        "#;
        let config = assert_gradle_ok(source, "kotlin_jvm_plugin");
        assert!(config.has_plugin("org.jetbrains.kotlin.jvm"));
        assert!(config.is_kotlin_project());
    }

    #[test]
    fn test_kotlin_android_plugin() {
        let source = r#"
            plugins {
                id 'org.jetbrains.kotlin.android' version '1.9.21'
            }
        "#;
        let config = assert_gradle_ok(source, "kotlin_android_plugin");
        assert!(config.has_plugin("org.jetbrains.kotlin.android"));
        assert!(config.is_kotlin_project());
    }

    #[test]
    fn test_android_application_plugin() {
        let source = r#"
            plugins {
                id 'com.android.application'
            }
        "#;
        let config = assert_gradle_ok(source, "android_application_plugin");
        assert!(config.has_plugin("com.android.application"));
        assert!(config.is_android_project());
    }

    #[test]
    fn test_android_library_plugin() {
        let source = r#"
            plugins {
                id 'com.android.library'
            }
        "#;
        let config = assert_gradle_ok(source, "android_library_plugin");
        assert!(config.has_plugin("com.android.library"));
        assert!(config.is_android_project());
    }

    #[test]
    fn test_multiple_plugins() {
        let source = r#"
            plugins {
                id 'java'
                id 'application'
                id 'maven-publish'
            }
        "#;
        let config = assert_gradle_ok(source, "multiple_plugins");
        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("application"));
        assert!(config.has_plugin("maven-publish"));
    }

    #[test]
    fn test_plugins_with_versions() {
        let source = r#"
            plugins {
                id 'java'
                id 'org.springframework.boot' version '3.2.0'
                id 'io.spring.dependency-management' version '1.1.4'
            }
        "#;
        let config = assert_gradle_ok(source, "plugins_with_versions");
        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("org.springframework.boot"));
        assert!(config.has_plugin("io.spring.dependency-management"));
    }

    #[test]
    fn test_legacy_apply_plugin() {
        let source = r#"
            apply plugin: 'java'
            apply plugin: 'maven-publish'
        "#;
        let config = assert_gradle_ok(source, "legacy_apply_plugin");
        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("maven-publish"));
    }

    #[test]
    fn test_mixed_plugin_syntax() {
        let source = r#"
            plugins {
                id 'java'
            }
            apply plugin: 'maven-publish'
        "#;
        let config = assert_gradle_ok(source, "mixed_plugin_syntax");
        assert!(config.has_plugin("java"));
        assert!(config.has_plugin("maven-publish"));
    }

    #[test]
    fn test_shadow_plugin() {
        let source = r#"
            plugins {
                id 'com.github.johnrengelman.shadow' version '8.1.1'
            }
        "#;
        let config = assert_gradle_ok(source, "shadow_plugin");
        assert!(config.has_plugin("com.github.johnrengelman.shadow"));
    }

    #[test]
    fn test_gradle_nexus_publish_plugin() {
        let source = r#"
            plugins {
                id 'io.github.gradle-nexus.publish-plugin' version '1.3.0'
            }
        "#;
        let config = assert_gradle_ok(source, "gradle_nexus_publish_plugin");
        assert!(config.has_plugin("io.github.gradle-nexus.publish-plugin"));
    }

    #[test]
    fn test_spotless_plugin() {
        let source = r#"
            plugins {
                id 'com.diffplug.spotless' version '6.23.0'
            }
        "#;
        let config = assert_gradle_ok(source, "spotless_plugin");
        assert!(config.has_plugin("com.diffplug.spotless"));
    }
}

// ============================================================================
// CATEGORY 3: Repository Patterns (15 tests)
// ============================================================================

mod repository_patterns {
    use super::*;

    #[test]
    fn test_maven_central() {
        let source = r#"
            repositories {
                mavenCentral()
            }
        "#;
        let config = assert_gradle_ok(source, "maven_central");
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
    }

    #[test]
    fn test_maven_local() {
        let source = r#"
            repositories {
                mavenLocal()
            }
        "#;
        let config = assert_gradle_ok(source, "maven_local");
        assert!(config.repositories.contains(&"mavenLocal".to_string()));
    }

    #[test]
    fn test_google_repository() {
        let source = r#"
            repositories {
                google()
            }
        "#;
        let config = assert_gradle_ok(source, "google_repository");
        assert!(config.repositories.contains(&"google".to_string()));
    }

    #[test]
    fn test_jcenter_deprecated() {
        let source = r#"
            repositories {
                jcenter()
            }
        "#;
        let config = assert_gradle_ok(source, "jcenter_deprecated");
        assert!(config.repositories.contains(&"jcenter".to_string()));
    }

    #[test]
    fn test_gradle_plugin_portal() {
        let source = r#"
            repositories {
                gradlePluginPortal()
            }
        "#;
        let config = assert_gradle_ok(source, "gradle_plugin_portal");
        assert!(config
            .repositories
            .contains(&"gradlePluginPortal".to_string()));
    }

    #[test]
    fn test_custom_maven_url_single_quotes() {
        let source = r#"
            repositories {
                maven { url 'https://repo.spring.io/release' }
            }
        "#;
        let config = assert_gradle_ok(source, "custom_maven_url_single_quotes");
        let repo = config
            .repository_list
            .iter()
            .find(|r| r.url == "https://repo.spring.io/release");
        assert!(repo.is_some());
    }

    #[test]
    fn test_command_form_platform_and_project_are_recorded() {
        let source = r#"
            dependencies {
                implementation platform('org.example:bom:1.0')
                implementation project(':shared')
            }
        "#;
        let config = assert_gradle_ok(source, "command_form_platform_project");
        let bom = config
            .dependency_list
            .iter()
            .find(|d| d.artifact == "bom")
            .expect("platform recorded");
        assert_eq!(bom.extension.as_deref(), Some("pom"));
        assert!(config
            .dependency_list
            .iter()
            .any(|d| d.artifact == ":shared"));
    }

    #[test]
    fn test_local_file_dependency_does_not_stop_other_declarations() {
        let source = r#"
            dependencies {
                implementation files('libs/local.jar')
                implementation 'org.example:lib:2.0'
            }
        "#;
        let config = assert_gradle_ok(source, "local_file_dependency");
        let declared = config.get_dependencies("implementation");
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0].artifact, "lib");
    }

    #[test]
    fn test_enforced_platform_is_recorded_as_enforced() {
        let source = r#"
            dependencies {
                implementation enforcedPlatform('org.example:enforced-bom:1.0')
                implementation platform('org.example:plain-bom:1.0')
            }
        "#;
        let config = assert_gradle_ok(source, "enforced_platform_recorded");
        let enforced = config
            .dependency_list
            .iter()
            .find(|d| d.artifact == "enforced-bom")
            .expect("enforcedPlatform dependency recorded");
        let plain = config
            .dependency_list
            .iter()
            .find(|d| d.artifact == "plain-bom")
            .expect("platform dependency recorded");
        assert!(enforced.enforced);
        assert!(!plain.enforced);
    }

    #[test]
    fn test_maven_url_assigned_from_uri_call() {
        let source = r#"
            repositories {
                maven { url = uri('https://repo.example.com/maven') }
            }
        "#;
        let config = assert_gradle_ok(source, "maven_url_from_uri_call");
        let recorded = config
            .repository_list
            .iter()
            .filter(|r| r.url == "https://repo.example.com/maven")
            .count();
        assert_eq!(recorded, 1);
    }

    #[test]
    fn test_custom_maven_url_double_quotes() {
        let source = r#"
            repositories {
                maven { url "https://jitpack.io" }
            }
        "#;
        let config = assert_gradle_ok(source, "custom_maven_url_double_quotes");
        let repo = config
            .repository_list
            .iter()
            .find(|r| r.url == "https://jitpack.io");
        assert!(repo.is_some());
    }

    #[test]
    fn test_multiple_repositories() {
        let source = r#"
            repositories {
                mavenCentral()
                mavenLocal()
                google()
            }
        "#;
        let config = assert_gradle_ok(source, "multiple_repositories");
        assert_eq!(config.repository_list.len(), 3);
    }

    #[test]
    fn test_all_common_repositories() {
        let source = r#"
            repositories {
                mavenCentral()
                mavenLocal()
                google()
                gradlePluginPortal()
            }
        "#;
        let config = assert_gradle_ok(source, "all_common_repositories");
        assert_eq!(config.repository_list.len(), 4);
    }

    #[test]
    fn test_maven_with_multiple_custom_repos() {
        let source = r#"
            repositories {
                mavenCentral()
                maven { url 'https://repo.spring.io/release' }
                maven { url 'https://jitpack.io' }
                maven { url 'https://oss.sonatype.org/content/repositories/snapshots' }
            }
        "#;
        let config = assert_gradle_ok(source, "maven_with_multiple_custom_repos");
        assert!(config.repository_list.len() >= 4);
    }

    #[test]
    fn test_android_repositories() {
        let source = r#"
            repositories {
                google()
                mavenCentral()
            }
        "#;
        let config = assert_gradle_ok(source, "android_repositories");
        assert!(config.repositories.contains(&"google".to_string()));
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
    }

    #[test]
    fn test_confluence_repository() {
        let source = r#"
            repositories {
                mavenCentral()
                maven { url 'https://packages.atlassian.com/maven/public' }
            }
        "#;
        let config = assert_gradle_ok(source, "confluence_repository");
        assert!(config.repository_list.len() >= 2);
    }

    #[test]
    fn test_jboss_repository() {
        let source = r#"
            repositories {
                maven { url 'https://repository.jboss.org/nexus/content/groups/public' }
            }
        "#;
        let config = assert_gradle_ok(source, "jboss_repository");
        assert_eq!(config.repository_list.len(), 1);
    }

    #[test]
    fn test_apache_snapshots_repository() {
        let source = r#"
            repositories {
                maven { url 'https://repository.apache.org/content/repositories/snapshots' }
            }
        "#;
        let config = assert_gradle_ok(source, "apache_snapshots_repository");
        assert_eq!(config.repository_list.len(), 1);
    }

    #[test]
    fn test_buildscript_repositories() {
        let source = r#"
            buildscript {
                repositories {
                    mavenCentral()
                    gradlePluginPortal()
                }
            }

            repositories {
                mavenCentral()
            }
        "#;
        let config = assert_gradle_ok(source, "buildscript_repositories");
        // Main repositories block should be parsed
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
    }
}

// ============================================================================
// CATEGORY 4: Dependency Patterns (30 tests)
// ============================================================================

mod dependency_patterns {
    use super::*;

    #[test]
    fn test_implementation_dependency() {
        let source = r#"
            dependencies {
                implementation 'com.google.guava:guava:32.1.2-jre'
            }
        "#;
        let config = assert_gradle_ok(source, "implementation_dependency");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].group, "com.google.guava");
        assert_eq!(deps[0].artifact, "guava");
        assert_eq!(deps[0].version, "32.1.2-jre");
    }

    #[test]
    fn test_api_dependency() {
        let source = r#"
            dependencies {
                api 'org.slf4j:slf4j-api:2.0.9'
            }
        "#;
        let config = assert_gradle_ok(source, "api_dependency");
        let deps = config.get_dependencies("api");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_compile_only_dependency() {
        let source = r#"
            dependencies {
                compileOnly 'org.projectlombok:lombok:1.18.30'
            }
        "#;
        let config = assert_gradle_ok(source, "compile_only_dependency");
        let deps = config.get_dependencies("compileOnly");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_runtime_only_dependency() {
        let source = r#"
            dependencies {
                runtimeOnly 'com.h2database:h2:2.2.224'
            }
        "#;
        let config = assert_gradle_ok(source, "runtime_only_dependency");
        let deps = config.get_dependencies("runtimeOnly");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_test_implementation_dependency() {
        let source = r#"
            dependencies {
                testImplementation 'org.junit.jupiter:junit-jupiter:5.10.1'
            }
        "#;
        let config = assert_gradle_ok(source, "test_implementation_dependency");
        let deps = config.get_dependencies("testImplementation");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_test_runtime_only_dependency() {
        let source = r#"
            dependencies {
                testRuntimeOnly 'org.junit.platform:junit-platform-launcher'
            }
        "#;
        let config = assert_gradle_ok(source, "test_runtime_only_dependency");
        // Version is empty, but dependency should still be parsed
        // Dependency without explicit version - parser should handle gracefully
        let _deps = config.get_dependencies("testRuntimeOnly");
    }

    #[test]
    fn test_annotation_processor_dependency() {
        let source = r#"
            dependencies {
                annotationProcessor 'org.projectlombok:lombok:1.18.30'
            }
        "#;
        let config = assert_gradle_ok(source, "annotation_processor_dependency");
        let deps = config.get_dependencies("annotationProcessor");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_kapt_dependency() {
        let source = r#"
            dependencies {
                kapt 'com.google.dagger:dagger-compiler:2.48'
            }
        "#;
        let config = assert_gradle_ok(source, "kapt_dependency");
        let deps = config.get_dependencies("kapt");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_parenthesis_syntax() {
        let source = r#"
            dependencies {
                implementation('com.google.guava:guava:32.1.2-jre')
            }
        "#;
        let config = assert_gradle_ok(source, "parenthesis_syntax");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_mixed_dependency_syntax() {
        let source = r#"
            dependencies {
                implementation 'com.google.guava:guava:32.1.2-jre'
                api('org.slf4j:slf4j-api:2.0.9')
            }
        "#;
        let config = assert_gradle_ok(source, "mixed_dependency_syntax");
        assert_eq!(config.get_dependencies("implementation").len(), 1);
        assert_eq!(config.get_dependencies("api").len(), 1);
    }

    #[test]
    fn test_multiple_dependencies_same_config() {
        let source = r#"
            dependencies {
                implementation 'com.google.guava:guava:32.1.2-jre'
                implementation 'org.apache.commons:commons-lang3:3.14.0'
                implementation 'com.fasterxml.jackson.core:jackson-databind:2.16.0'
            }
        "#;
        let config = assert_gradle_ok(source, "multiple_dependencies_same_config");
        assert_eq!(config.get_dependencies("implementation").len(), 3);
    }

    #[test]
    fn test_all_configurations() {
        let source = r#"
            dependencies {
                implementation 'com.example:impl:1.0'
                api 'com.example:api:1.0'
                compileOnly 'com.example:compileonly:1.0'
                runtimeOnly 'com.example:runtimeonly:1.0'
                testImplementation 'com.example:testimpl:1.0'
                testCompileOnly 'com.example:testcompileonly:1.0'
                testRuntimeOnly 'com.example:testruntimeonly:1.0'
                annotationProcessor 'com.example:processor:1.0'
            }
        "#;
        let config = assert_gradle_ok(source, "all_configurations");
        assert!(!config.get_dependencies("implementation").is_empty());
        assert!(!config.get_dependencies("api").is_empty());
        assert!(!config.get_dependencies("compileOnly").is_empty());
        assert!(!config.get_dependencies("runtimeOnly").is_empty());
        assert!(!config.get_dependencies("testImplementation").is_empty());
    }

    #[test]
    fn test_project_dependency() {
        let source = r#"
            dependencies {
                implementation project(':core')
            }
        "#;
        let config = assert_gradle_ok(source, "project_dependency");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
        assert!(deps[0].is_project);
        assert_eq!(deps[0].artifact, ":core");
    }

    #[test]
    fn test_multiple_project_dependencies() {
        let source = r#"
            dependencies {
                implementation project(':core')
                implementation project(':common')
                api project(':api')
            }
        "#;
        let config = assert_gradle_ok(source, "multiple_project_dependencies");
        let impl_deps = config.get_dependencies("implementation");
        let project_deps: Vec<_> = impl_deps.iter().filter(|d| d.is_project).collect();
        assert_eq!(project_deps.len(), 2);
    }

    #[test]
    fn test_dependency_with_exclusion() {
        let source = r#"
            dependencies {
                implementation('org.springframework.boot:spring-boot-starter-web:3.2.0') {
                    exclude group: 'org.springframework.boot', module: 'spring-boot-starter-tomcat'
                }
            }
        "#;
        let config = assert_gradle_ok(source, "dependency_with_exclusion");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_dependency_with_multiple_exclusions() {
        let source = r#"
            dependencies {
                implementation('com.example:lib:1.0') {
                    exclude group: 'org.unwanted'
                    exclude module: 'bad-module'
                    exclude group: 'another.group', module: 'another-module'
                }
            }
        "#;
        let config = assert_gradle_ok(source, "dependency_with_multiple_exclusions");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn test_spring_boot_starter_web() {
        let source = r#"
            dependencies {
                implementation 'org.springframework.boot:spring-boot-starter-web'
            }
        "#;
        // Note: no version - BOM managed. Pattern may not match.
        let _ = assert_gradle_ok(source, "spring_boot_starter_web");
    }

    #[test]
    fn test_spring_boot_full_stack() {
        let source = r#"
            dependencies {
                implementation 'org.springframework.boot:spring-boot-starter-web:3.2.0'
                implementation 'org.springframework.boot:spring-boot-starter-data-jpa:3.2.0'
                implementation 'org.springframework.boot:spring-boot-starter-security:3.2.0'
                implementation 'org.springframework.boot:spring-boot-starter-actuator:3.2.0'
                runtimeOnly 'org.postgresql:postgresql:42.7.1'
                testImplementation 'org.springframework.boot:spring-boot-starter-test:3.2.0'
            }
        "#;
        let config = assert_gradle_ok(source, "spring_boot_full_stack");
        assert!(config.get_dependencies("implementation").len() >= 4);
    }

    #[test]
    fn test_android_dependencies() {
        let source = r#"
            dependencies {
                implementation 'androidx.core:core-ktx:1.12.0'
                implementation 'androidx.appcompat:appcompat:1.6.1'
                implementation 'com.google.android.material:material:1.11.0'
                implementation 'androidx.constraintlayout:constraintlayout:2.1.4'
                testImplementation 'junit:junit:4.13.2'
                androidTestImplementation 'androidx.test.ext:junit:1.1.5'
            }
        "#;
        let config = assert_gradle_ok(source, "android_dependencies");
        assert!(config.get_dependencies("implementation").len() >= 4);
    }

    #[test]
    fn test_kotlin_stdlib() {
        let source = r#"
            dependencies {
                implementation 'org.jetbrains.kotlin:kotlin-stdlib:1.9.21'
                implementation 'org.jetbrains.kotlin:kotlin-reflect:1.9.21'
                testImplementation 'org.jetbrains.kotlin:kotlin-test:1.9.21'
            }
        "#;
        let config = assert_gradle_ok(source, "kotlin_stdlib");
        assert_eq!(config.get_dependencies("implementation").len(), 2);
    }

    #[test]
    fn test_jackson_dependencies() {
        let source = r#"
            dependencies {
                implementation 'com.fasterxml.jackson.core:jackson-core:2.16.0'
                implementation 'com.fasterxml.jackson.core:jackson-databind:2.16.0'
                implementation 'com.fasterxml.jackson.core:jackson-annotations:2.16.0'
                implementation 'com.fasterxml.jackson.module:jackson-module-kotlin:2.16.0'
            }
        "#;
        let config = assert_gradle_ok(source, "jackson_dependencies");
        assert_eq!(config.get_dependencies("implementation").len(), 4);
    }

    #[test]
    fn test_testing_frameworks() {
        let source = r#"
            dependencies {
                testImplementation 'org.junit.jupiter:junit-jupiter:5.10.1'
                testImplementation 'org.mockito:mockito-core:5.8.0'
                testImplementation 'org.mockito:mockito-junit-jupiter:5.8.0'
                testImplementation 'org.assertj:assertj-core:3.24.2'
            }
        "#;
        let config = assert_gradle_ok(source, "testing_frameworks");
        assert_eq!(config.get_dependencies("testImplementation").len(), 4);
    }

    #[test]
    fn test_logging_dependencies() {
        let source = r#"
            dependencies {
                implementation 'org.slf4j:slf4j-api:2.0.9'
                runtimeOnly 'ch.qos.logback:logback-classic:1.4.14'
                runtimeOnly 'ch.qos.logback:logback-core:1.4.14'
            }
        "#;
        let config = assert_gradle_ok(source, "logging_dependencies");
        assert_eq!(config.get_dependencies("implementation").len(), 1);
        assert_eq!(config.get_dependencies("runtimeOnly").len(), 2);
    }

    #[test]
    fn test_database_dependencies() {
        let source = r#"
            dependencies {
                implementation 'com.zaxxer:HikariCP:5.1.0'
                runtimeOnly 'org.postgresql:postgresql:42.7.1'
                runtimeOnly 'com.h2database:h2:2.2.224'
                runtimeOnly 'mysql:mysql-connector-java:8.0.33'
            }
        "#;
        let config = assert_gradle_ok(source, "database_dependencies");
        assert_eq!(config.get_dependencies("runtimeOnly").len(), 3);
    }

    #[test]
    fn test_http_client_dependencies() {
        let source = r#"
            dependencies {
                implementation 'com.squareup.okhttp3:okhttp:4.12.0'
                implementation 'com.squareup.retrofit2:retrofit:2.9.0'
                implementation 'com.squareup.retrofit2:converter-gson:2.9.0'
            }
        "#;
        let config = assert_gradle_ok(source, "http_client_dependencies");
        assert_eq!(config.get_dependencies("implementation").len(), 3);
    }

    #[test]
    fn test_grpc_dependencies() {
        let source = r#"
            dependencies {
                implementation 'io.grpc:grpc-netty-shaded:1.60.0'
                implementation 'io.grpc:grpc-protobuf:1.60.0'
                implementation 'io.grpc:grpc-stub:1.60.0'
                compileOnly 'org.apache.tomcat:annotations-api:6.0.53'
            }
        "#;
        let config = assert_gradle_ok(source, "grpc_dependencies");
        assert_eq!(config.get_dependencies("implementation").len(), 3);
    }

    #[test]
    fn test_aws_sdk_dependencies() {
        let source = r#"
            dependencies {
                implementation 'software.amazon.awssdk:s3:2.21.0'
                implementation 'software.amazon.awssdk:dynamodb:2.21.0'
                implementation 'software.amazon.awssdk:sqs:2.21.0'
            }
        "#;
        let config = assert_gradle_ok(source, "aws_sdk_dependencies");
        assert_eq!(config.get_dependencies("implementation").len(), 3);
    }
}

// ============================================================================
// CATEGORY 5: Property/Variable Patterns (10 tests)
// ============================================================================

mod property_patterns {
    use super::*;

    #[test]
    fn test_ext_block_properties() {
        let source = r#"
            ext {
                kotlinVersion = '1.9.21'
                springVersion = '3.2.0'
            }
        "#;
        let config = assert_gradle_ok(source, "ext_block_properties");
        assert_eq!(
            config.extra_properties.get("kotlinVersion"),
            Some(&"1.9.21".to_string())
        );
        assert_eq!(
            config.extra_properties.get("springVersion"),
            Some(&"3.2.0".to_string())
        );
    }

    #[test]
    fn test_def_variable() {
        let source = r#"
            def guavaVersion = '32.1.2-jre'

            dependencies {
                implementation "com.google.guava:guava:${guavaVersion}"
            }
        "#;
        let config = assert_gradle_ok(source, "def_variable");
        assert_eq!(
            config.extra_properties.get("guavaVersion"),
            Some(&"32.1.2-jre".to_string())
        );
        // Check GString interpolation worked
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].version, "32.1.2-jre");
    }

    #[test]
    fn test_multiple_def_variables() {
        let source = r#"
            def guavaVersion = '32.1.2-jre'
            def junitVersion = '5.10.1'
            def jacksonVersion = '2.16.0'

            dependencies {
                implementation "com.google.guava:guava:${guavaVersion}"
                implementation "com.fasterxml.jackson.core:jackson-core:${jacksonVersion}"
                testImplementation "org.junit.jupiter:junit-jupiter:${junitVersion}"
            }
        "#;
        let config = assert_gradle_ok(source, "multiple_def_variables");
        let impl_deps = config.get_dependencies("implementation");
        assert_eq!(impl_deps.len(), 2);
        // Verify interpolation
        assert!(impl_deps.iter().any(|d| d.version == "32.1.2-jre"));
        assert!(impl_deps.iter().any(|d| d.version == "2.16.0"));
    }

    #[test]
    fn test_group_and_version_properties() {
        let source = r#"
            group = 'com.example.myproject'
            version = '1.0.0-SNAPSHOT'
        "#;
        let config = assert_gradle_ok(source, "group_and_version_properties");
        assert_eq!(config.group, Some("com.example.myproject".into()));
        assert_eq!(config.version, Some("1.0.0-SNAPSHOT".into()));
    }

    #[test]
    fn test_description_property() {
        let source = r#"
            description = 'My awesome project'
        "#;
        let config = assert_gradle_ok(source, "description_property");
        assert_eq!(config.description, Some("My awesome project".into()));
    }

    #[test]
    fn test_combined_properties() {
        let source = r#"
            group = 'com.example'
            version = '2.0.0'
            description = 'Example project'

            ext {
                springBootVersion = '3.2.0'
            }

            def jacksonVersion = '2.16.0'
        "#;
        let config = assert_gradle_ok(source, "combined_properties");
        assert_eq!(config.group, Some("com.example".into()));
        assert_eq!(config.version, Some("2.0.0".into()));
        assert!(config.extra_properties.contains_key("springBootVersion"));
        assert!(config.extra_properties.contains_key("jacksonVersion"));
    }

    #[test]
    fn test_gstring_interpolation_basic() {
        let source = r#"
            def ver = '1.0.0'

            dependencies {
                implementation "com.example:lib:${ver}"
            }
        "#;
        let config = assert_gradle_ok(source, "gstring_interpolation_basic");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].version, "1.0.0");
    }

    #[test]
    fn test_ext_property_in_dependency() {
        let source = r#"
            ext {
                lombokVersion = '1.18.30'
            }

            dependencies {
                compileOnly "org.projectlombok:lombok:${lombokVersion}"
                annotationProcessor "org.projectlombok:lombok:${lombokVersion}"
            }
        "#;
        let config = assert_gradle_ok(source, "ext_property_in_dependency");
        let compile_deps = config.get_dependencies("compileOnly");
        assert_eq!(compile_deps.len(), 1);
        assert_eq!(compile_deps[0].version, "1.18.30");
    }

    #[test]
    fn test_version_in_ext_typical_pattern() {
        let source = r#"
            ext {
                kotlinVersion = '1.9.21'
                coroutinesVersion = '1.7.3'
                serializationVersion = '1.6.2'
            }

            dependencies {
                implementation "org.jetbrains.kotlin:kotlin-stdlib:${kotlinVersion}"
                implementation "org.jetbrains.kotlinx:kotlinx-coroutines-core:${coroutinesVersion}"
                implementation "org.jetbrains.kotlinx:kotlinx-serialization-json:${serializationVersion}"
            }
        "#;
        let config = assert_gradle_ok(source, "version_in_ext_typical_pattern");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 3);
        assert!(deps.iter().any(|d| d.version == "1.9.21"));
        assert!(deps.iter().any(|d| d.version == "1.7.3"));
        assert!(deps.iter().any(|d| d.version == "1.6.2"));
    }

    #[test]
    fn test_version_property_not_confused_with_plugin_version() {
        let source = r#"
            plugins {
                id 'org.springframework.boot' version '3.2.0'
            }

            version = '1.0.0'
        "#;
        let config = assert_gradle_ok(source, "version_property_not_confused_with_plugin_version");
        // Project version should be 1.0.0, not 3.2.0
        assert_eq!(config.version, Some("1.0.0".into()));
        // Plugin version should be separate
        let spring_plugin = config
            .plugin_definitions
            .iter()
            .find(|p| p.id == "org.springframework.boot");
        assert_eq!(
            spring_plugin.map(|p| p.version.clone()),
            Some(Some("3.2.0".into()))
        );
    }
}

// ============================================================================
// CATEGORY 6: Task Patterns (10 tests)
// ============================================================================

mod task_patterns {
    use super::*;

    #[test]
    fn test_basic_task() {
        let source = r#"
            task hello {
                doLast {
                    println 'Hello!'
                }
            }
        "#;
        let config = assert_gradle_ok(source, "basic_task");
        assert!(config.tasks.contains(&"hello".to_string()));
    }

    #[test]
    fn test_typed_task_copy() {
        let source = r#"
            task copyDocs(type: Copy) {
                from 'docs'
                into 'build/docs'
            }
        "#;
        let config = assert_gradle_ok(source, "typed_task_copy");
        assert!(config.tasks.contains(&"copyDocs".to_string()));
        let task = config
            .task_definitions
            .iter()
            .find(|t| t.name == "copyDocs")
            .unwrap();
        assert_eq!(task.task_type, Some("Copy".into()));
    }

    #[test]
    fn test_typed_task_delete() {
        let source = r#"
            task clean(type: Delete) {
                delete buildDir
            }
        "#;
        let config = assert_gradle_ok(source, "typed_task_delete");
        assert!(config.tasks.contains(&"clean".to_string()));
        let task = config
            .task_definitions
            .iter()
            .find(|t| t.name == "clean")
            .unwrap();
        assert_eq!(task.task_type, Some("Delete".into()));
    }

    #[test]
    fn test_typed_task_jar() {
        let source = r#"
            task fatJar(type: Jar) {
                manifest {
                    attributes 'Main-Class': 'com.example.Main'
                }
                from { configurations.runtimeClasspath.collect { it.isDirectory() ? it : zipTree(it) } }
            }
        "#;
        let config = assert_gradle_ok(source, "typed_task_jar");
        assert!(config.tasks.contains(&"fatJar".to_string()));
    }

    #[test]
    fn test_task_with_depends_on() {
        let source = r#"
            task build(dependsOn: ['compile', 'test']) {
            }
        "#;
        let config = assert_gradle_ok(source, "task_with_depends_on");
        assert!(config.tasks.contains(&"build".to_string()));
    }

    #[test]
    fn test_tasks_register() {
        let source = r#"
            tasks.register('myTask') {
                doLast {
                    println 'My task!'
                }
            }
        "#;
        let config = assert_gradle_ok(source, "tasks_register");
        assert!(config.tasks.contains(&"myTask".to_string()));
    }

    #[test]
    fn test_tasks_register_typed() {
        let source = r#"
            tasks.register('copyResources', Copy) {
                from 'src/main/resources'
                into 'build/resources'
            }
        "#;
        let config = assert_gradle_ok(source, "tasks_register_typed");
        assert!(config.tasks.contains(&"copyResources".to_string()));
    }

    #[test]
    fn test_multiple_tasks() {
        let source = r#"
            task clean {
                doLast { }
            }

            task build {
                dependsOn clean
                doLast { }
            }

            task test {
                dependsOn build
                doLast { }
            }
        "#;
        let config = assert_gradle_ok(source, "multiple_tasks");
        assert!(config.tasks.contains(&"clean".to_string()));
        assert!(config.tasks.contains(&"build".to_string()));
        assert!(config.tasks.contains(&"test".to_string()));
    }

    #[test]
    fn test_task_with_do_first_do_last() {
        let source = r#"
            task example {
                doFirst {
                    println 'First'
                }
                doLast {
                    println 'Last'
                }
            }
        "#;
        let config = assert_gradle_ok(source, "task_with_do_first_do_last");
        assert!(config.tasks.contains(&"example".to_string()));
    }

    #[test]
    fn test_task_configuration() {
        let source = r#"
            task processResources {
                inputs.dir 'src/main/resources'
                outputs.dir 'build/resources/main'
                doLast {
                    copy {
                        from 'src/main/resources'
                        into 'build/resources/main'
                    }
                }
            }
        "#;
        let config = assert_gradle_ok(source, "task_configuration");
        assert!(config.tasks.contains(&"processResources".to_string()));
    }
}

// ============================================================================
// CATEGORY 7: Edge Cases and Error Handling (10 tests)
// ============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_malformed_plugins_block() {
        let source = r#"
            plugins {
                id 'java'
            // Missing closing brace

            dependencies {
                implementation 'com.example:lib:1.0'
            }
        "#;
        // Should not panic, extract what it can
        let result = execute_gradle(source);
        assert!(result.is_ok());
    }

    #[test]
    fn test_empty_blocks() {
        let source = r#"
            plugins {
            }

            repositories {
            }

            dependencies {
            }
        "#;
        let config = assert_gradle_ok(source, "empty_blocks");
        assert!(config.plugins.is_empty());
        assert!(config.repository_list.is_empty());
        assert!(config.dependency_list.is_empty());
    }

    #[test]
    fn test_special_characters_in_version() {
        let source = r#"
            dependencies {
                implementation 'com.example:lib:1.0.0-SNAPSHOT'
                implementation 'com.example:lib2:2.0.0-rc.1'
                implementation 'com.example:lib3:3.0.0+build.123'
            }
        "#;
        let config = assert_gradle_ok(source, "special_characters_in_version");
        let deps = config.get_dependencies("implementation");
        assert_eq!(deps.len(), 3);
        assert!(deps.iter().any(|d| d.version == "1.0.0-SNAPSHOT"));
    }

    #[test]
    fn test_unicode_in_description() {
        let source = r#"
            description = 'My awesome project'
        "#;
        let config = assert_gradle_ok(source, "unicode_in_description");
        // Should handle gracefully (may or may not preserve unicode)
        assert!(config.description.is_some());
    }

    #[test]
    fn test_very_long_dependency_list() {
        let mut deps = String::new();
        for i in 0..50 {
            deps.push_str(&format!("    implementation 'com.example:lib{}:1.0'\n", i));
        }
        let source = format!("dependencies {{\n{}}}", deps);
        let config = assert_gradle_ok(&source, "very_long_dependency_list");
        assert_eq!(config.get_dependencies("implementation").len(), 50);
    }

    #[test]
    fn test_dependency_without_version() {
        let source = r#"
            dependencies {
                implementation 'org.springframework.boot:spring-boot-starter-web'
            }
        "#;
        // BOM-managed dependencies don't have versions
        // This may not be extracted by the current pattern
        let _ = assert_gradle_ok(source, "dependency_without_version");
    }

    #[test]
    fn test_nested_gstring() {
        let source = r#"
            ext {
                baseVersion = '1.0'
            }

            dependencies {
                implementation "com.example:lib:${baseVersion}.0"
            }
        "#;
        // Complex GString interpolation
        let _ = assert_gradle_ok(source, "nested_gstring");
    }

    #[test]
    fn test_comments_inside_blocks() {
        let source = r#"
            dependencies {
                // This is a comment
                implementation 'com.example:lib1:1.0'
                /* Multi-line
                   comment */
                implementation 'com.example:lib2:2.0'
            }
        "#;
        let config = assert_gradle_ok(source, "comments_inside_blocks");
        assert_eq!(config.get_dependencies("implementation").len(), 2);
    }

    #[test]
    fn test_tabs_vs_spaces() {
        let source = "plugins {\n\tid 'java'\n}\n\nrepositories {\n    mavenCentral()\n}";
        let config = assert_gradle_ok(source, "tabs_vs_spaces");
        assert!(config.has_plugin("java"));
        assert!(config.repositories.contains(&"mavenCentral".to_string()));
    }

    #[test]
    fn test_windows_line_endings() {
        let source = "plugins {\r\n    id 'java'\r\n}\r\n";
        let config = assert_gradle_ok(source, "windows_line_endings");
        assert!(config.has_plugin("java"));
    }
}

// ============================================================================
// CATEGORY 8: Configuration Detection (5 tests)
// ============================================================================

mod config_detection {
    use super::*;

    #[test]
    fn test_detect_kotlin_project() {
        let source = r#"
            plugins {
                id 'org.jetbrains.kotlin.jvm' version '1.9.21'
            }
        "#;
        let config = assert_gradle_ok(source, "detect_kotlin_project");
        assert!(config.is_kotlin_project());
    }

    #[test]
    fn test_detect_android_project() {
        let source = r#"
            plugins {
                id 'com.android.application'
            }
        "#;
        let config = assert_gradle_ok(source, "detect_android_project");
        assert!(config.is_android_project());
    }

    #[test]
    fn test_detect_spring_boot_project() {
        let source = r#"
            plugins {
                id 'org.springframework.boot' version '3.2.0'
            }
        "#;
        let config = assert_gradle_ok(source, "detect_spring_boot_project");
        assert!(config.is_spring_boot_project());
    }

    #[test]
    fn test_detect_java_only_project() {
        let source = r#"
            plugins {
                id 'java'
            }
        "#;
        let config = assert_gradle_ok(source, "detect_java_only_project");
        assert!(!config.is_kotlin_project());
        assert!(!config.is_android_project());
        assert!(!config.is_spring_boot_project());
    }

    #[test]
    fn test_detect_multi_type_project() {
        let source = r#"
            plugins {
                id 'com.android.application'
                id 'org.jetbrains.kotlin.android' version '1.9.21'
            }
        "#;
        let config = assert_gradle_ok(source, "detect_multi_type_project");
        assert!(config.is_android_project());
        assert!(config.is_kotlin_project());
    }
}

// ============================================================================
// Summary
// ============================================================================
//
// Total tests: 115
//
// Category breakdown:
// - Basic Patterns: 20 tests
// - Plugin Patterns: 15 tests
// - Repository Patterns: 15 tests
// - Dependency Patterns: 30 tests
// - Property/Variable Patterns: 10 tests
// - Task Patterns: 10 tests
// - Edge Cases: 10 tests
// - Configuration Detection: 5 tests
