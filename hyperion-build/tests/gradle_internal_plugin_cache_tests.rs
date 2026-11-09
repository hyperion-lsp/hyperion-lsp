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

fn setup_warm_cache_fixture(root: &std::path::Path) {
    write(
        &root.join("settings.gradle"),
        r#"
rootProject.name = "demo"
include(":app")
includeBuild("build-logic")
"#,
    );

    write(
        &root.join("build.gradle"),
        r#"
plugins {
  id "com.example.internal"
}
"#,
    );

    write(
        &root.join(
            "build-logic/build/resources/main/META-INF/gradle-plugins/com.example.internal.properties",
        ),
        "implementation-class=com.example.InternalPlugin\n",
    );

    // InternalPlugin.class (precompiled test fixture) + inner Action class.
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
}

fn has_dep(project: &hyperion_build::gradle::model::GradleProject, version: &str) -> bool {
    project.dependencies.iter().any(|dep| {
        dep.group == "org.apache.commons"
            && dep.artifact == "commons-lang3"
            && dep.version.as_deref() == Some(version)
    })
}

#[tokio::test]
async fn internal_plugin_execution_cache_hits_and_invalidates_on_toml_change() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    setup_warm_cache_fixture(root);

    write(
        &root.join("gradle/libs.versions.toml"),
        r#"
[versions]
commons = "3.13.0"
"#,
    );

    let (project1, report1) = GradleParser::parse_project_with_report(root)
        .await
        .expect("parse_project");
    assert!(
        has_dep(&project1, "3.13.0"),
        "expected dep on first run; dependencies={:?}; notes={:?}",
        project1.dependencies,
        report1.notes
    );
    assert!(
        report1
            .notes
            .iter()
            .any(|n| n.contains("internal plugin cache miss")),
        "expected cache miss note on first run; notes={:?}",
        report1.notes
    );

    let (project2, report2) = GradleParser::parse_project_with_report(root)
        .await
        .expect("parse_project");
    assert!(has_dep(&project2, "3.13.0"), "expected dep on second run");
    assert!(
        report2
            .notes
            .iter()
            .any(|n| n.contains("internal plugin cache hit")),
        "expected cache hit note on second run; notes={:?}",
        report2.notes
    );

    std::thread::sleep(std::time::Duration::from_millis(2));
    write(
        &root.join("gradle/libs.versions.toml"),
        r#"
[versions]
commons = "3.14.0"
"#,
    );

    let (project3, report3) = GradleParser::parse_project_with_report(root)
        .await
        .expect("parse_project");
    assert!(
        has_dep(&project3, "3.14.0"),
        "expected dep after toml change"
    );
    assert!(
        report3
            .notes
            .iter()
            .any(|n| n.contains("internal plugin cache miss")),
        "expected cache miss note after invalidation; notes={:?}",
        report3.notes
    );
}
