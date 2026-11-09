// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_build::gradle::parser::GradleParser;
use tempfile::TempDir;

fn write(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    std::fs::write(path, content).expect("write file");
}

fn write_b64(path: &std::path::Path, b64: &str) {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs");
    }
    let bytes = STANDARD.decode(b64).expect("decode base64");
    std::fs::write(path, bytes).expect("write bytes");
}

/// This is a common Gradle pattern we want the sniffer to handle:
///
/// - Root build uses Groovy DSL (`build.gradle`) as the entrypoint.
/// - The build is multi-module (`settings.gradle` includes `:app`).
/// - A composite `includeBuild("build-logic")` provides an internal plugin implemented in Kotlin.
/// - The plugin adds dependencies using versions read from `gradle/libs.versions.toml`
///   by calling a Kotlin utility function defined in another source file.
///
/// Today, Hyperion's offline parser and bytecode sniffer do not execute Kotlin plugin code,
/// so this is expected to fail until the sniffer can evaluate internal plugin builds.
#[tokio::test]
async fn test_mixed_dsl_internal_plugin_adds_dependencies_via_toml_util() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Version catalog (what the Kotlin plugin reads).
    write(
        &root.join("gradle/libs.versions.toml"),
        r#"
[versions]
commons = "3.13.0"
"#,
    );

    // Multi-module settings, Groovy entrypoint.
    write(
        &root.join("settings.gradle"),
        r#"
rootProject.name = "demo"
include(":app")
includeBuild("build-logic")
"#,
    );

    // Root entrypoint is Groovy; it applies the internal plugin.
    write(
        &root.join("build.gradle"),
        r#"
plugins {
  id "com.example.internal"
}

// No explicit dependencies here; the internal plugin is responsible for adding them.
"#,
    );

    // App subproject uses Kotlin DSL but does not declare dependencies itself.
    write(
        &root.join("app/build.gradle.kts"),
        r#"
plugins { java }
"#,
    );

    // Composite build providing the internal plugin.
    write(
        &root.join("build-logic/settings.gradle.kts"),
        r#"
rootProject.name = "build-logic"
"#,
    );

    write(
        &root.join("build-logic/build.gradle.kts"),
        r#"
plugins { `kotlin-dsl` }

repositories { mavenCentral() }

// In a real build this would register the plugin id and point to the implementation class.
// The sniffer would need to compile/load this build to execute the plugin.
"#,
    );

    // Warm-cache: compiled plugin outputs + descriptor present (no Gradle execution required).
    //
    // The .class files are tiny precompiled fixtures (Java bytecode is fine for this test);
    // what matters is that Hyperion can load and execute the plugin offline and resolve
    // versions via a whitelisted intrinsic rather than arbitrary file I/O.
    write(
        &root.join(
            "build-logic/build/resources/main/META-INF/gradle-plugins/com.example.internal.properties",
        ),
        "implementation-class=com.example.InternalPlugin\n",
    );

    // InternalPlugin: calls TomlUtil.readVersion(File,\"commons\") and then adds the dependency
    // in a subprojects(Action<Project>) block.
    write_b64(
        &root.join("build-logic/build/classes/kotlin/main/com/example/InternalPlugin.class"),
        "yv66vgAAADQALgoAAgADBwAEDAAFAAYBABBqYXZhL2xhbmcvT2JqZWN0AQAGPGluaXQ+AQADKClWCwAIAAkHAAoMAAsADAEAFm9yZy9ncmFkbGUvYXBpL1Byb2plY3QBAApnZXRSb290RGlyAQAQKClMamF2YS9pby9GaWxlOwgADgEAB2NvbW1vbnMKABAAEQcAEgwAEwAUAQAUY29tL2V4YW1wbGUvVG9tbFV0aWwBAAtyZWFkVmVyc2lvbgEANChMamF2YS9pby9GaWxlO0xqYXZhL2xhbmcvU3RyaW5nOylMamF2YS9sYW5nL1N0cmluZzsHABYBABxjb20vZXhhbXBsZS9JbnRlcm5hbFBsdWdpbiQxCgAVABgMAAUAGQEAMShMY29tL2V4YW1wbGUvSW50ZXJuYWxQbHVnaW47TGphdmEvbGFuZy9TdHJpbmc7KVYLAAgAGwwAHAAdAQALc3VicHJvamVjdHMBABooTG9yZy9ncmFkbGUvYXBpL0FjdGlvbjspVgoAHwAgBwAhDAAiACMBABpjb20vZXhhbXBsZS9JbnRlcm5hbFBsdWdpbgEABWFwcGx5AQAbKExvcmcvZ3JhZGxlL2FwaS9Qcm9qZWN0OylWBwAlAQAVb3JnL2dyYWRsZS9hcGkvUGx1Z2luAQAEQ29kZQEAD0xpbmVOdW1iZXJUYWJsZQEAFShMamF2YS9sYW5nL09iamVjdDspVgEACVNpZ25hdHVyZQEAQ0xqYXZhL2xhbmcvT2JqZWN0O0xvcmcvZ3JhZGxlL2FwaS9QbHVnaW48TG9yZy9ncmFkbGUvYXBpL1Byb2plY3Q7PjsBAApTb3VyY2VGaWxlAQATSW50ZXJuYWxQbHVnaW4uamF2YQEADElubmVyQ2xhc3NlcwAhAB8AAgABACQAAAADAAEABQAGAAEAJgAAAB0AAQABAAAABSq3AAGxAAAAAQAnAAAABgABAAAABwABACIAIwABACYAAAA8AAUAAwAAABwruQAHAQASDbgAD00ruwAVWSostwAXuQAaAgCxAAAAAQAnAAAADgADAAAACwAMAA0AGwAUEEEAIgAoAAEAJgAAACEAAgACAAAACSorwAAItgAesQAAAAEAJwAAAAYAAQAAAAcAAwApAAAAAgAqACsAAAACACwALQAAAAoAAQAVAAAAAAAA",
    );
    write_b64(
        &root.join("build-logic/build/classes/kotlin/main/com/example/InternalPlugin$1.class"),
        "yv66vgAAADQASwkAAgADBwAEDAAFAAYBABxjb20vZXhhbXBsZS9JbnRlcm5hbFBsdWdpbiQxAQAGdGhpcyQwAQAcTGNvbS9leGFtcGxlL0ludGVybmFsUGx1Z2luOwkAAgAIDAAJAAoBAAt2YWwkdmVyc2lvbgEAEkxqYXZhL2xhbmcvU3RyaW5nOwoADAANBwAODAAPABABABBqYXZhL2xhbmcvT2JqZWN0AQAGPGluaXQ+AQADKClWCwASABMHABQMABUAFgEAFm9yZy9ncmFkbGUvYXBpL1Byb2plY3QBAApnZXRQbHVnaW5zAQAqKClMb3JnL2dyYWRsZS9hcGkvcGx1Z2lucy9QbHVnaW5Db250YWluZXI7CAAYAQAEamF2YQsAGgAbBwAcDAAdAB4BACZvcmcvZ3JhZGxlL2FwaS9wbHVnaW5zL1BsdWdpbkNvbnRhaW5lcgEABWFwcGx5AQAVKExqYXZhL2xhbmcvU3RyaW5nOylWCwASACAMACEAIgEAD2dldERlcGVuZGVuY2llcwEAMigpTG9yZy9ncmFkbGUvYXBpL2FydGlmYWN0cy9kc2wvRGVwZW5kZW5jeUhhbmRsZXI7CAAkAQAOaW1wbGVtZW50YXRpb24HACYBABdqYXZhL2xhbmcvU3RyaW5nQnVpbGRlcgoAJQANCAApAQAhb3JnLmFwYWNoZS5jb21tb25zOmNvbW1vbnMtbGFuZzM6CgAlACsMACwALQEABmFwcGVuZAEALShMamF2YS9sYW5nL1N0cmluZzspTGphdmEvbGFuZy9TdHJpbmdCdWlsZGVyOwoAJQAvDAAwADEBAAh0b1N0cmluZwEAFCgpTGphdmEvbGFuZy9TdHJpbmc7CwAzADQHADUMADYANwEALm9yZy9ncmFkbGUvYXBpL2FydGlmYWN0cy9kc2wvRGVwZW5kZW5jeUhhbmRsZXIBAANhZGQBADgoTGphdmEvbGFuZy9TdHJpbmc7TGphdmEvbGFuZy9PYmplY3Q7KUxqYXZhL2xhbmcvT2JqZWN0OwoAAgA5DAA6ADsBAAdleGVjdXRlAQAbKExvcmcvZ3JhZGxlL2FwaS9Qcm9qZWN0OylWBwA9AQAVb3JnL2dyYWRsZS9hcGkvQWN0aW9uAQAxKExjb20vZXhhbXBsZS9JbnRlcm5hbFBsdWdpbjtMamF2YS9sYW5nL1N0cmluZzspVgEABENvZGUBAA9MaW5lTnVtYmVyVGFibGUBAAlTaWduYXR1cmUBABUoTGphdmEvbGFuZy9PYmplY3Q7KVYBAENMamF2YS9sYW5nL09iamVjdDtMb3JnL2dyYWRsZS9hcGkvQWN0aW9uPExvcmcvZ3JhZGxlL2FwaS9Qcm9qZWN0Oz47AQAKU291cmNlRmlsZQEAE0ludGVybmFsUGx1Z2luLmphdmEBAA9FbmNsb3NpbmdNZXRob2QHAEgBABpjb20vZXhhbXBsZS9JbnRlcm5hbFBsdWdpbgwAHQA7AQAMSW5uZXJDbGFzc2VzACAAAgAMAAEAPAACEBAACQAKAAAQEAAFAAYAAAADAAAADwA+AAIAPwAAACcAAgADAAAADyortQABKiy1AAcqtwALsQAAAAEAQAAAAAYAAQAAAA0AQQAAAAIAEAABADoAOwABAD8AAABSAAQAAgAAADIruQARAQASF7kAGQIAK7kAHwEAEiO7ACVZtwAnEii2ACoqtAAHtgAqtgAuuQAyAwBXsQAAAAEAQAAAAA4AAwAAABAADQARADEAEhBBADoAQgABAD8AAAAhAAIAAgAAAAkqK8AAErYAOLEAAAABAEAAAAAGAAEAAAANAAQAQQAAAAIAQwBEAAAAAgBFAEYAAAAEAEcASQBKAAAACgABAAIAAAAAAAA=",
    );
    write_b64(
        &root.join("build-logic/build/classes/kotlin/main/com/example/TomlUtil.class"),
        "yv66vgAAADQAEQoAAgADBwAEDAAFAAYBABBqYXZhL2xhbmcvT2JqZWN0AQAGPGluaXQ+AQADKClWCAAIAQAOX19VTkVYUEVDVEVEX18HAAoBABRjb20vZXhhbXBsZS9Ub21sVXRpbAEABENvZGUBAA9MaW5lTnVtYmVyVGFibGUBAAtyZWFkVmVyc2lvbgEANChMamF2YS9pby9GaWxlO0xqYXZhL2xhbmcvU3RyaW5nOylMamF2YS9sYW5nL1N0cmluZzsBAApTb3VyY2VGaWxlAQANVG9tbFV0aWwuamF2YQAhAAkAAgAAAAAAAgABAAUABgABAAsAAAAdAAEAAQAAAAUqtwABsQAAAAEADAAAAAYAAQAAAAMACQANAA4AAQALAAAAGwABAAIAAAADEgewAAAAAQAMAAAABgABAAAABgABAA8AAAACABA=",
    );

    write(
        &root.join("build-logic/src/main/kotlin/com/example/InternalPlugin.kt"),
        r#"
package com.example

import org.gradle.api.Plugin
import org.gradle.api.Project

class InternalPlugin : Plugin<Project> {
    override fun apply(target: Project) {
        val version = Versions.commonsVersion(target.rootDir)

        target.subprojects {
            plugins.apply("java")
            dependencies.add("implementation", "org.apache.commons:commons-lang3:$version")
        }
    }
}
"#,
    );

    write(
        &root.join("build-logic/src/main/kotlin/com/example/Versions.kt"),
        r#"
package com.example

import java.io.File

object Versions {
    fun commonsVersion(rootDir: File): String {
        val toml = rootDir.resolve("gradle/libs.versions.toml")
        return TomlUtil.readVersion(toml, "commons")
    }
}
"#,
    );

    write(
        &root.join("build-logic/src/main/kotlin/com/example/TomlUtil.kt"),
        r#"
package com.example

import java.io.File

object TomlUtil {
    fun readVersion(file: File, key: String): String {
        // Extremely simplified for fixture purposes.
        val line = file.readLines().first { it.trimStart().startsWith("$key") }
        return line.substringAfter("=").trim().trim('"')
    }
}
"#,
    );

    let (project, report) = GradleParser::parse_project_with_report(root)
        .await
        .expect("parse_project");

    // Sanity: subprojects were discovered from settings.gradle.
    assert!(
        project.subprojects.iter().any(|p| p == "app"),
        "expected :app subproject"
    );
    assert!(
        project
            .subprojects
            .iter()
            .any(|p| p == "@build:build-logic"),
        "expected includeBuild build-logic to be discovered"
    );

    // Desired behavior: the sniffer should execute the internal Kotlin plugin and observe the
    // dependency added to `implementation`.
    let has = project.dependencies.iter().any(|dep| {
        dep.group == "org.apache.commons"
            && dep.artifact == "commons-lang3"
            && dep.version.as_deref() == Some("3.13.0")
    });

    assert!(
        has,
        "expected dependency from internal plugin to be present; got deps: {:?}\nreport:\n{}",
        project
            .dependencies
            .iter()
            .map(|d| format!("{}:{}:{:?}", d.group, d.artifact, d.version))
            .collect::<Vec<_>>(),
        report.debug_details(),
    );
}
