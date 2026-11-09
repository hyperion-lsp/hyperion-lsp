// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maven POM parser for extracting dependencies and project information.

use super::model::{DependencyScope, MavenDependency, MavenProject};
use crate::io::{read_utf8_file_bounded, MAX_BUILD_FILE_BYTES};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Errors that can occur during Maven parsing
#[derive(Debug, thiserror::Error)]
pub enum MavenParserError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("XML parse error: {0}")]
    XmlError(String),

    #[error("POM file not found: {0}")]
    PomNotFound(PathBuf),

    #[error("Parse error: {0}")]
    ParseError(String),
}

pub type Result<T> = std::result::Result<T, MavenParserError>;

/// Maven POM parser
pub struct MavenParser;

impl MavenParser {
    /// Parse a Maven project from a directory
    pub async fn parse_project(project_dir: &Path) -> Result<MavenProject> {
        info!("Parsing Maven project at: {:?}", project_dir);

        let pom_path = project_dir.join("pom.xml");
        if !pom_path.exists() {
            return Err(MavenParserError::PomNotFound(pom_path));
        }

        let artifact_id = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let mut project = MavenProject::new(artifact_id, project_dir.to_path_buf());

        Self::parse_pom(&pom_path, &mut project)?;

        info!(
            "Parsed Maven project '{}' with {} dependencies",
            project.artifact_id,
            project.dependencies.len()
        );

        Ok(project)
    }

    /// Parse pom.xml file
    pub fn parse_pom(path: &Path, project: &mut MavenProject) -> Result<()> {
        let content = read_utf8_file_bounded(path, MAX_BUILD_FILE_BYTES)?;
        let mut reader = Reader::from_str(&content);
        reader.config_mut().trim_text(true);

        let mut buf = Vec::new();
        let mut current_path: Vec<String> = Vec::new();
        let mut current_text = String::new();

        // Dependency being parsed
        let mut current_dep: Option<MavenDependency> = None;
        let mut in_dependency = false;
        let mut in_dependency_management = false;

        // Exclusion being parsed
        let mut current_exclusion: Option<(String, String)> = None; // (groupId, artifactId)
        let mut in_exclusions = false;
        let mut in_exclusion = false;

        // Parent POM being parsed
        let mut current_parent: Option<(String, String, String, Option<String>)> = None; // (groupId, artifactId, version, relativePath)
        let mut in_parent = false;

        // Profile being parsed
        let mut current_profiles: Vec<super::model::MavenProfile> = Vec::new();
        let mut current_profile: Option<super::model::MavenProfile> = None;
        let mut in_profiles = false;
        let mut in_profile = false;
        let mut in_profile_activation = false;
        let mut in_profile_activation_property = false;
        let mut in_profile_activation_os = false;
        let mut in_profile_activation_file = false;
        let mut current_activation_property: Option<(String, Option<String>)> = None; // (name, value)
        let mut current_activation_os: Option<(Option<String>, Option<String>, Option<String>)> =
            None; // (name, arch, version)
        let mut current_activation_file: Option<(Option<String>, Option<String>)> = None; // (exists, missing)
        let mut current_profile_activation_conditions = 0usize;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    current_path.push(tag_name.clone());

                    let parent_tag = current_path
                        .len()
                        .checked_sub(2)
                        .and_then(|index| current_path.get(index))
                        .map(String::as_str);
                    let unmodeled_build_feature = match tag_name.as_str() {
                        "plugins" if parent_tag == Some("build") => Some("build plugins"),
                        "pluginManagement" if parent_tag == Some("build") => {
                            Some("build pluginManagement")
                        }
                        "extensions" if parent_tag == Some("build") => Some("build extensions"),
                        "sourceDirectory"
                        | "testSourceDirectory"
                        | "resources"
                        | "testResources"
                        | "directory"
                            if current_path[..current_path.len() - 1]
                                .iter()
                                .any(|component| component == "build") =>
                        {
                            Some("custom build source/output paths")
                        }
                        "annotationProcessorPaths" => Some("compiler annotationProcessorPaths"),
                        _ => None,
                    };
                    if let Some(feature) = unmodeled_build_feature {
                        let description = format!("{feature} are not modeled");
                        if !project.unmodeled_features.contains(&description) {
                            project.unmodeled_features.push(description);
                        }
                    }

                    if tag_name == "dependency" {
                        in_dependency = true;
                        current_dep = Some(MavenDependency {
                            group_id: String::new(),
                            artifact_id: String::new(),
                            version: None,
                            scope: DependencyScope::default(),
                            scope_explicit: false,
                            classifier: None,
                            packaging: "jar".to_string(),
                            packaging_explicit: false,
                            system_path: None,
                            optional: false,
                            optional_explicit: false,
                            exclusions: Vec::new(),
                        });
                    } else if tag_name == "exclusions" && in_dependency {
                        in_exclusions = true;
                    } else if tag_name == "exclusion" && in_exclusions {
                        in_exclusion = true;
                        current_exclusion = Some((String::new(), String::new()));
                    } else if tag_name == "dependencyManagement" {
                        if in_profile {
                            project.unmodeled_features.push(
                                "profile-scoped dependencyManagement is not modeled".to_string(),
                            );
                        }
                        in_dependency_management = true;
                    } else if tag_name == "parent" && current_path.len() == 2 {
                        // Top-level <parent> tag
                        in_parent = true;
                        current_parent = Some((String::new(), String::new(), String::new(), None));
                    } else if tag_name == "profiles" {
                        in_profiles = true;
                    } else if tag_name == "profile" && in_profiles {
                        project
                            .unmodeled_features
                            .push("explicit Maven profile selection is not modeled".to_string());
                        in_profile = true;
                        current_profile_activation_conditions = 0;
                        current_profile = Some(super::model::MavenProfile {
                            id: String::new(),
                            activation: super::model::ProfileActivation::Always,
                            dependencies: Vec::new(),
                            properties: std::collections::HashMap::new(),
                        });
                    } else if tag_name == "activation" && in_profile {
                        in_profile_activation = true;
                    } else if tag_name == "property" && in_profile_activation {
                        in_profile_activation_property = true;
                        current_activation_property = Some((String::new(), None));
                    } else if tag_name == "os" && in_profile_activation {
                        in_profile_activation_os = true;
                        current_activation_os = Some((None, None, None));
                    } else if tag_name == "file" && in_profile_activation {
                        in_profile_activation_file = true;
                        current_activation_file = Some((None, None));
                    }

                    current_text.clear();
                }
                Ok(Event::Empty(e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if in_parent && tag_name == "relativePath" {
                        // Maven uses an empty relativePath to disable the
                        // conventional ../pom.xml reactor-parent lookup.
                        if let Some(parent) = current_parent.as_mut() {
                            parent.3 = Some(String::new());
                        }
                    }
                    current_text.clear();
                }
                Ok(Event::Text(e)) => {
                    current_text = match e.unescape() {
                        Ok(text) => text.to_string(),
                        Err(error) => {
                            // Keep the raw text: a malformed entity must not erase the value.
                            tracing::warn!("POM text has an invalid entity: {error}");
                            String::from_utf8_lossy(&e).into_owned()
                        }
                    }
                    .trim()
                    .to_string();
                }
                Ok(Event::End(e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    // Handle exclusion fields
                    let was_in_exclusion = in_exclusion;
                    if was_in_exclusion {
                        if let Some(ref mut excl) = current_exclusion {
                            match tag_name.as_str() {
                                "groupId" => excl.0 = current_text.clone(),
                                "artifactId" => excl.1 = current_text.clone(),
                                "exclusion" => {
                                    // End of exclusion
                                    in_exclusion = false;
                                    if let Some((group_id, artifact_id)) = current_exclusion.take()
                                    {
                                        if !group_id.is_empty() && !artifact_id.is_empty() {
                                            if let Some(ref mut dep) = current_dep {
                                                debug!(
                                                    "Found exclusion: {}:{}",
                                                    group_id, artifact_id
                                                );
                                                dep.exclusions.push((group_id, artifact_id));
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }

                    // Handle dependency fields
                    if in_dependency && !was_in_exclusion {
                        if let Some(ref mut dep) = current_dep {
                            match tag_name.as_str() {
                                "groupId" => dep.group_id = project.resolve_property(&current_text),
                                "artifactId" => {
                                    dep.artifact_id = project.resolve_property(&current_text)
                                }
                                "version" => {
                                    dep.version = Some(project.resolve_property(&current_text))
                                }
                                "scope" => {
                                    let scope = project.resolve_property(&current_text);
                                    if scope.contains("${") {
                                        project.unmodeled_features.push(format!(
                                            "dependency scope remains dynamic: {scope}"
                                        ));
                                    }
                                    dep.scope = DependencyScope::from_scope_str(&scope);
                                    dep.scope_explicit = true;
                                }
                                "classifier" => dep.classifier = Some(current_text.clone()),
                                "type" => {
                                    dep.packaging = current_text.clone();
                                    dep.packaging_explicit = true;
                                }
                                "systemPath" => {
                                    dep.system_path = Some(project.resolve_property(&current_text));
                                }
                                "optional" => {
                                    let optional = project.resolve_property(&current_text);
                                    if !matches!(optional.as_str(), "true" | "false") {
                                        project.unmodeled_features.push(format!(
                                            "dependency optional flag is not static: {optional}"
                                        ));
                                    }
                                    dep.optional = optional == "true";
                                    dep.optional_explicit = true;
                                }
                                "exclusions" => {
                                    // End of exclusions block
                                    in_exclusions = false;
                                }
                                "dependency" => {
                                    // End of dependency
                                    in_dependency = false;
                                    if let Some(dep) = current_dep.take() {
                                        if !dep.group_id.is_empty() && !dep.artifact_id.is_empty() {
                                            if in_profile && in_dependency_management {
                                                // Kept out of profile.dependencies: treating
                                                // managed versions as real edges would corrupt
                                                // even the useful partial model. The feature
                                                // marker set on entry keeps readiness false.
                                            } else if in_profile {
                                                // Add dependency to current profile
                                                if let Some(ref mut profile) = current_profile {
                                                    let mut dep = dep;
                                                    project.apply_managed_dependency(&mut dep);
                                                    debug!(
                                                        "Found profile dependency: {}",
                                                        dep.coordinates()
                                                    );
                                                    profile.dependencies.push(dep);
                                                }
                                            } else if in_dependency_management {
                                                // Check if this is a BOM import
                                                if dep.packaging == "pom"
                                                    && dep.scope == DependencyScope::Import
                                                {
                                                    // This is a BOM import - add to bom_imports
                                                    debug!(
                                                        "Found BOM import: {}",
                                                        dep.coordinates()
                                                    );
                                                    project.bom_imports.push(dep);
                                                } else {
                                                    // Regular managed dependency - add to managed dependencies
                                                    project.insert_managed_dependency(dep);
                                                }
                                            } else {
                                                // Resolve version from dependency management if not specified
                                                let mut dep = dep;
                                                project.apply_managed_dependency(&mut dep);
                                                debug!("Found dependency: {}", dep.coordinates());
                                                project.dependencies.push(dep);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if in_parent {
                        // Handle parent fields
                        if let Some(ref mut parent) = current_parent {
                            match tag_name.as_str() {
                                "groupId" => parent.0 = current_text.clone(),
                                "artifactId" => parent.1 = current_text.clone(),
                                "version" => parent.2 = current_text.clone(),
                                "relativePath" => parent.3 = Some(current_text.clone()),
                                "parent" => {
                                    // End of parent
                                    in_parent = false;
                                    if let Some((group_id, artifact_id, version, relative_path)) =
                                        current_parent.take()
                                    {
                                        if !group_id.is_empty()
                                            && !artifact_id.is_empty()
                                            && !version.is_empty()
                                        {
                                            use super::model::ParentPom;
                                            project.parent = Some(ParentPom {
                                                group_id,
                                                artifact_id,
                                                version,
                                                relative_path,
                                            });
                                            debug!("Found parent POM: {:?}", project.parent);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if in_profile {
                        // Handle profile end tag first (special case to avoid borrow issues)
                        if tag_name == "profile" {
                            in_profile = false;
                            if current_profile_activation_conditions > 1 {
                                project.unmodeled_features.push(
                                    "profile with multiple activation conditions is not modeled"
                                        .to_string(),
                                );
                            }
                            if let Some(profile) = current_profile.take() {
                                if !profile.id.is_empty() {
                                    debug!("Found profile: {}", profile.id);
                                    current_profiles.push(profile);
                                }
                            }
                        } else if tag_name == "profiles" {
                            in_profiles = false;
                        } else if tag_name == "dependencyManagement" {
                            in_dependency_management = false;
                        } else if tag_name == "module" {
                            project
                                .unmodeled_features
                                .push("profile-scoped reactor modules are not modeled".to_string());
                        } else if let Some(ref mut profile) = current_profile {
                            // Handle profile fields
                            if in_profile_activation {
                                // Handle activation fields
                                if in_profile_activation_property {
                                    if let Some(ref mut prop) = current_activation_property {
                                        match tag_name.as_str() {
                                            "name" => prop.0 = current_text.clone(),
                                            "value" => prop.1 = Some(current_text.clone()),
                                            "property" => {
                                                in_profile_activation_property = false;
                                                current_profile_activation_conditions += 1;
                                                if let Some((name, value)) =
                                                    current_activation_property.take()
                                                {
                                                    if !name.is_empty() {
                                                        profile.activation =
                                                            super::model::ProfileActivation::ByProperty {
                                                                name,
                                                                value,
                                                            };
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                } else if in_profile_activation_os {
                                    if let Some(ref mut os) = current_activation_os {
                                        match tag_name.as_str() {
                                            "name" => os.0 = Some(current_text.clone()),
                                            "arch" => os.1 = Some(current_text.clone()),
                                            "version" => os.2 = Some(current_text.clone()),
                                            "os" => {
                                                in_profile_activation_os = false;
                                                current_profile_activation_conditions += 1;
                                                if let Some((name, arch, version)) =
                                                    current_activation_os.take()
                                                {
                                                    profile.activation =
                                                        super::model::ProfileActivation::ByOS {
                                                            name,
                                                            arch,
                                                            version,
                                                        };
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                } else if in_profile_activation_file {
                                    if let Some(ref mut file) = current_activation_file {
                                        match tag_name.as_str() {
                                            "exists" => {
                                                file.0 = Some(current_text.clone());
                                            }
                                            "missing" => {
                                                file.1 = Some(current_text.clone());
                                            }
                                            "file" => {
                                                in_profile_activation_file = false;
                                                current_profile_activation_conditions += 1;
                                                if let Some((exists, missing)) =
                                                    current_activation_file.take()
                                                {
                                                    let exists_path =
                                                        exists.map(std::path::PathBuf::from);
                                                    let missing_path =
                                                        missing.map(std::path::PathBuf::from);
                                                    profile.activation =
                                                        super::model::ProfileActivation::ByFile {
                                                            exists: exists_path,
                                                            missing: missing_path,
                                                        };
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                } else {
                                    match tag_name.as_str() {
                                        "jdk" => {
                                            current_profile_activation_conditions += 1;
                                            profile.activation =
                                                super::model::ProfileActivation::ByJDK(
                                                    current_text.clone(),
                                                );
                                        }
                                        "activeByDefault" => {
                                            project.unmodeled_features.push(
                                                "profile activeByDefault activation is not modeled"
                                                    .to_string(),
                                            );
                                        }
                                        "activation" => {
                                            in_profile_activation = false;
                                        }
                                        _ => {}
                                    }
                                }
                            } else {
                                if tag_name.as_str() == "id" {
                                    profile.id = current_text.clone();
                                    debug!("Profile ID: {}", current_text);
                                }

                                // Handle profile properties
                                if current_path.len() > 3
                                    && current_path[current_path.len() - 2] == "properties"
                                {
                                    profile
                                        .properties
                                        .insert(tag_name.clone(), current_text.clone());
                                    debug!("Profile property: {} = {}", tag_name, current_text);
                                }
                            }
                        }
                    } else {
                        // Handle project-level fields
                        match tag_name.as_str() {
                            "groupId" if current_path.len() == 2 => {
                                // Top-level groupId (not in parent or dependency)
                                project.group_id = Some(project.resolve_property(&current_text));
                            }
                            "artifactId" if current_path.len() == 2 => {
                                project.artifact_id = project.resolve_property(&current_text);
                            }
                            "version" if current_path.len() == 2 => {
                                project.version = Some(project.resolve_property(&current_text));
                            }
                            "packaging" => {
                                project.packaging = current_text.clone();
                            }
                            "module" => {
                                project.modules.push(current_text.clone());
                                debug!("Found module: {}", current_text);
                            }
                            "dependencyManagement" => {
                                in_dependency_management = false;
                            }
                            _ => {}
                        }

                        // Handle properties
                        if current_path.len() > 2
                            && current_path[current_path.len() - 2] == "properties"
                        {
                            project
                                .properties
                                .insert(tag_name.clone(), current_text.clone());
                            debug!("Property: {} = {}", tag_name, current_text);
                        }
                    }

                    current_path.pop();
                    current_text.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(MavenParserError::XmlError(format!(
                        "Error at position {}: {:?}",
                        reader.buffer_position(),
                        e
                    )));
                }
                _ => {}
            }

            buf.clear();
        }

        // Assign parsed profiles to project
        project.profiles = current_profiles;

        Ok(())
    }
}

#[cfg(test)]
mod completeness_tests {
    use super::*;

    #[test]
    fn top_level_coordinates_are_not_confused_with_nested_coordinates() {
        let temp = tempfile::tempdir().unwrap();
        let pom = temp.path().join("pom.xml");
        std::fs::write(
            &pom,
            r"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>root</artifactId><version>1</version><dependencies><dependency><groupId>org.other</groupId><artifactId>child</artifactId><version>2</version></dependency></dependencies></project>",
        )
        .unwrap();
        let mut project = MavenProject::new("placeholder".to_string(), temp.path().to_path_buf());

        MavenParser::parse_pom(&pom, &mut project).unwrap();

        assert_eq!(project.group_id.as_deref(), Some("org.example"));
        assert_eq!(project.artifact_id, "root");
        assert_eq!(project.version.as_deref(), Some("1"));
        assert_eq!(project.dependencies.len(), 1);
        assert_eq!(project.dependencies[0].group_id, "org.other");
        assert_eq!(project.dependencies[0].artifact_id, "child");
        assert_eq!(project.dependencies[0].version.as_deref(), Some("2"));
    }

    #[test]
    fn unsupported_profile_model_features_are_explicit_and_not_fake_edges() {
        let temp = tempfile::tempdir().unwrap();
        let pom = temp.path().join("pom.xml");
        std::fs::write(
            &pom,
            r"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>root</artifactId><version>1</version><profiles><profile><id>complex</id><activation><activeByDefault>true</activeByDefault></activation><modules><module>child</module></modules><dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>managed</artifactId><version>2</version></dependency></dependencies></dependencyManagement></profile></profiles></project>",
        )
        .unwrap();
        let mut project = MavenProject::new("root".to_string(), temp.path().to_path_buf());

        MavenParser::parse_pom(&pom, &mut project).unwrap();

        assert!(project
            .unmodeled_features
            .iter()
            .any(|feature| feature.contains("profile-scoped dependencyManagement")));
        assert!(project
            .unmodeled_features
            .iter()
            .any(|feature| feature.contains("profile-scoped reactor modules")));
        assert!(project
            .unmodeled_features
            .iter()
            .any(|feature| feature.contains("activeByDefault")));
        assert!(project.dependencies.is_empty());
    }

    #[test]
    fn build_plugins_extensions_and_processor_paths_fail_completeness_closed() {
        for (construct, expected_feature) in [
            (
                "<build><plugins><plugin><groupId>org.example</groupId><artifactId>generator</artifactId></plugin></plugins></build>",
                "build plugins",
            ),
            (
                "<build><pluginManagement><plugins/></pluginManagement></build>",
                "pluginManagement",
            ),
            (
                "<build><extensions><extension><groupId>org.example</groupId><artifactId>extension</artifactId><version>1</version></extension></extensions></build>",
                "build extensions",
            ),
            (
                "<build><plugins><plugin><configuration><annotationProcessorPaths><path><groupId>org.example</groupId><artifactId>processor</artifactId><version>1</version></path></annotationProcessorPaths></configuration></plugin></plugins></build>",
                "annotationProcessorPaths",
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let pom = temp.path().join("pom.xml");
            std::fs::write(
                &pom,
                format!(
                    "<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>root</artifactId><version>1</version>{construct}</project>"
                ),
            )
            .unwrap();
            let mut project =
                MavenProject::new("root".to_string(), temp.path().to_path_buf());

            MavenParser::parse_pom(&pom, &mut project).unwrap();

            assert!(
                project
                    .unmodeled_features
                    .iter()
                    .any(|feature| feature.contains(expected_feature)),
                "{construct} must retain an explicit completeness blocker: {:?}",
                project.unmodeled_features
            );
            assert!(
                !super::super::resolver::MavenResolver::new()
                    .finalize_project_dependencies(&mut project),
                "{construct} must never produce a complete effective model"
            );
        }
    }

    #[test]
    fn dependency_management_uses_full_variant_key_and_inherits_attributes() {
        let temp = tempfile::tempdir().unwrap();
        let pom = temp.path().join("pom.xml");
        std::fs::write(
            &pom,
            r#"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>root</artifactId><version>1</version>
<dependencyManagement><dependencies>
  <dependency><groupId>org.example</groupId><artifactId>library</artifactId><version>1</version><scope>runtime</scope><optional>true</optional><exclusions><exclusion><groupId>bad</groupId><artifactId>transitive</artifactId></exclusion></exclusions></dependency>
  <dependency><groupId>org.example</groupId><artifactId>library</artifactId><version>2</version><type>test-jar</type><classifier>tests</classifier></dependency>
</dependencies></dependencyManagement>
<dependencies>
  <dependency><groupId>org.example</groupId><artifactId>library</artifactId></dependency>
  <dependency><groupId>org.example</groupId><artifactId>library</artifactId><type>test-jar</type><classifier>tests</classifier></dependency>
</dependencies></project>"#,
        )
        .unwrap();
        let mut project = MavenProject::new("root".to_string(), temp.path().to_path_buf());

        MavenParser::parse_pom(&pom, &mut project).unwrap();

        assert_eq!(
            project.dependencies[0].version.as_deref(),
            Some("1"),
            "managed dependencies: {:#?}; direct dependencies: {:#?}",
            project.managed_dependencies,
            project.dependencies,
        );
        assert_eq!(project.dependencies[0].scope, DependencyScope::Runtime);
        assert!(project.dependencies[0].optional);
        assert_eq!(
            project.dependencies[0].exclusions,
            vec![("bad".to_string(), "transitive".to_string())]
        );
        assert_eq!(project.dependencies[1].version.as_deref(), Some("2"));
    }

    #[test]
    fn system_path_is_retained_as_the_dependency_artifact_identity() {
        let temp = tempfile::tempdir().unwrap();
        let exact = temp.path().join("vendor/system.jar");
        let pom = temp.path().join("pom.xml");
        std::fs::write(
            &pom,
            r#"<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>root</artifactId><version>1</version><dependencies><dependency><groupId>org.example</groupId><artifactId>system-library</artifactId><version>1</version><scope>system</scope><systemPath>${project.basedir}/vendor/system.jar</systemPath></dependency></dependencies></project>"#,
        )
        .unwrap();
        let mut project = MavenProject::new("root".to_string(), temp.path().to_path_buf());

        MavenParser::parse_pom(&pom, &mut project).unwrap();

        assert_eq!(project.dependencies.len(), 1);
        assert_eq!(project.dependencies[0].scope, DependencyScope::System);
        assert_eq!(
            project.dependencies[0].system_path.as_deref(),
            exact.to_str()
        );
    }
}
