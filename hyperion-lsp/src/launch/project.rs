// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The build-tool facts a launch needs on top of the shared module locator
//! (`hyperion_layout`): Gradle project path, wrapper selection and the
//! conventional compiled-output directories.

use std::path::{Path, PathBuf};

use hyperion_build::gradle::GradleParser;
use hyperion_layout::same_dir;
pub(crate) use hyperion_layout::{BuildTool, ModuleContext};

/// Gradle project path (`:app`, `:libs:core`, `:` for the root) of
/// `module_dir`, read from the settings file's `include` and `projectDir`
/// statements and otherwise derived from the directory layout.
pub(crate) fn gradle_project_path(build_root: &Path, module_dir: &Path) -> String {
    if same_dir(build_root, module_dir) {
        return ":".to_owned();
    }
    GradleParser::settings_module_dirs(build_root)
        .into_iter()
        .find(|(_, dir)| same_dir(dir, module_dir))
        .map_or_else(
            || relative_gradle_path(build_root, module_dir),
            |(path, _)| path,
        )
}

fn relative_gradle_path(build_root: &Path, module_dir: &Path) -> String {
    let relative = module_dir.strip_prefix(build_root).unwrap_or(module_dir);
    let mut path = String::new();
    for component in relative.components() {
        path.push(':');
        path.push_str(&component.as_os_str().to_string_lossy());
    }
    if path.is_empty() {
        ":".to_owned()
    } else {
        path
    }
}

/// `gradlew` only when the wrapper is complete, else `gradle` on PATH.
pub(crate) fn gradle_executable(build_root: &Path) -> String {
    let wrapper = if cfg!(windows) {
        "gradlew.bat"
    } else {
        "gradlew"
    };
    let script = build_root.join(wrapper);
    if script.is_file()
        && build_root
            .join("gradle/wrapper/gradle-wrapper.jar")
            .is_file()
    {
        script.to_string_lossy().into_owned()
    } else {
        "gradle".to_owned()
    }
}

/// `mvnw` when `.mvn/wrapper` exists, else `mvn` on PATH.
pub(crate) fn maven_executable(build_root: &Path) -> String {
    let wrapper = if cfg!(windows) { "mvnw.cmd" } else { "mvnw" };
    let script = build_root.join(wrapper);
    if script.is_file() && build_root.join(".mvn/wrapper").is_dir() {
        script.to_string_lossy().into_owned()
    } else {
        "mvn".to_owned()
    }
}

/// Conventional compiled-output directories of one source set, in classpath
/// order. They are listed whether or not they exist yet: the pre-launch build
/// creates them.
pub(crate) fn conventional_outputs(ctx: &ModuleContext) -> Vec<PathBuf> {
    let module = &ctx.module_dir;
    let mut sets = vec![ctx.source_set.as_str()];
    if ctx.is_test_set() {
        sets.push("main");
    }
    let mut dirs = Vec::new();
    for set in sets {
        match ctx.build_tool {
            BuildTool::Gradle => {
                for language in hyperion_layout::JVM_LANGUAGES {
                    let output = hyperion_layout::gradle_class_dir(module, language, set);
                    // Java and Kotlin outputs are always listed (the build
                    // may not have run yet); Groovy and Scala only when the
                    // module has that language, so plain modules do not
                    // carry two dead entries.
                    let in_use = matches!(language, "java" | "kotlin")
                        || output.is_dir()
                        || module.join("src").join(set).join(language).is_dir();
                    if in_use {
                        dirs.push(output);
                    }
                }
                dirs.push(hyperion_layout::gradle_resources_dir(module, set));
            }
            BuildTool::Maven => dirs.push(hyperion_layout::maven_class_dir(module, set)),
            BuildTool::None => {}
        }
    }
    dirs
}

pub(crate) const fn path_separator() -> &'static str {
    if cfg!(windows) {
        ";"
    } else {
        ":"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "").unwrap();
        path
    }

    fn locate(root: &Path, file: &str, package: Option<&str>) -> ModuleContext {
        let file = touch(root, file);
        ModuleContext::locate(&file, package, root)
    }

    #[test]
    fn locate_groovy_source_root_and_its_conventional_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "build.gradle");
        let ctx = locate(root, "src/main/groovy/Foo.groovy", None);
        assert_eq!(ctx.source_set, "main");
        assert_eq!(ctx.module_dir, root);
        let outputs = conventional_outputs(&ctx);
        assert!(outputs.contains(&root.join("build/classes/java/main")));
        assert!(outputs.contains(&root.join("build/classes/kotlin/main")));
        // The Groovy compile output is on the classpath too.
        assert!(outputs.contains(&root.join("build/classes/groovy/main")));
    }

    #[test]
    fn test_sets_also_get_the_main_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        touch(root, "build.gradle");
        let ctx = locate(root, "src/test/java/FooTest.java", None);
        let outputs = conventional_outputs(&ctx);
        assert!(outputs.contains(&root.join("build/classes/java/test")));
        assert!(outputs.contains(&root.join("build/classes/java/main")));
        assert!(outputs.contains(&root.join("build/resources/main")));
    }

    #[test]
    fn project_path_of_the_settings_directory_is_the_root_path() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(gradle_project_path(temp.path(), temp.path()), ":");
    }

    #[test]
    fn project_path_follows_include_and_project_dir_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("settings.gradle"),
            "include ':app', ':libs:core'\n\
             include('tools')\n\
             project(':tools').projectDir = file(\"$rootDir/build-tools\")\n",
        )
        .unwrap();
        assert_eq!(gradle_project_path(root, &root.join("app")), ":app");
        assert_eq!(
            gradle_project_path(root, &root.join("libs/core")),
            ":libs:core"
        );
        assert_eq!(
            gradle_project_path(root, &root.join("build-tools")),
            ":tools"
        );
        // Not declared anywhere: derived from the directory layout.
        assert_eq!(gradle_project_path(root, &root.join("x/y")), ":x:y");
    }

    #[test]
    fn project_path_honours_java_file_constructor_overrides() {
        // `File(...)` is accepted by launch's regex but not by hyperion-build's.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::write(
            root.join("settings.gradle.kts"),
            "include(\":tools\")\nproject(\":tools\").projectDir = File(\"build-tools\")\n",
        )
        .unwrap();
        assert_eq!(
            gradle_project_path(root, &root.join("build-tools")),
            ":tools"
        );
    }
}
