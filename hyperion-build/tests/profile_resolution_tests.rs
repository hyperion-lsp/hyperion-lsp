// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for Maven profile resolution
//!
//! These tests verify that profile activation correctly merges dependencies
//! and properties into the project.

use hyperion_build::maven::{MavenParser, MavenResolver, ProfileActivationContext};
use tempfile::TempDir;

/// Helper to create a test project with a pom.xml
fn create_test_project(pom_xml: &str) -> TempDir {
    let temp_dir = tempfile::tempdir().unwrap();
    let pom_path = temp_dir.path().join("pom.xml");
    std::fs::write(&pom_path, pom_xml).unwrap();
    temp_dir
}

#[tokio::test]
async fn test_profile_dependencies_merged() {
    let pom_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <dependencies>
        <dependency>
            <groupId>com.example</groupId>
            <artifactId>base-lib</artifactId>
            <version>1.0</version>
        </dependency>
    </dependencies>

    <profiles>
        <profile>
            <id>test-profile</id>
            <activation>
                <activeByDefault>true</activeByDefault>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>com.example</groupId>
                    <artifactId>profile-lib</artifactId>
                    <version>2.0</version>
                </dependency>
            </dependencies>
        </profile>
    </profiles>
</project>
"#;

    let temp_dir = create_test_project(pom_xml);
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();

    // Verify initial state
    assert_eq!(
        project.dependencies.len(),
        1,
        "Should have 1 base dependency before activation"
    );
    assert_eq!(project.profiles.len(), 1, "Should have 1 profile");

    // Activate profiles
    let context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    let resolver = MavenResolver::new();
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();

    // Verify both base and profile dependencies are present
    assert_eq!(
        project.dependencies.len(),
        2,
        "Should have 2 dependencies after activation"
    );

    let has_base = project
        .dependencies
        .iter()
        .any(|d| d.artifact_id == "base-lib");
    let has_profile = project
        .dependencies
        .iter()
        .any(|d| d.artifact_id == "profile-lib");

    assert!(has_base, "Should have base dependency");
    assert!(has_profile, "Should have profile dependency");
}

#[tokio::test]
async fn test_inactive_profile_not_merged() {
    let pom_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <dependencies>
        <dependency>
            <groupId>com.example</groupId>
            <artifactId>base-lib</artifactId>
            <version>1.0</version>
        </dependency>
    </dependencies>

    <profiles>
        <profile>
            <id>windows-only</id>
            <activation>
                <os>
                    <name>Windows</name>
                </os>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>com.example</groupId>
                    <artifactId>windows-lib</artifactId>
                    <version>1.0</version>
                </dependency>
            </dependencies>
        </profile>
    </profiles>
</project>
"#;

    let temp_dir = create_test_project(pom_xml);
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();

    // Activate with Linux context (will not match Windows OS)
    let mut context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    context.os_name = "Linux".to_string();

    let resolver = MavenResolver::new();
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();

    // Should only have base dependency
    assert_eq!(
        project.dependencies.len(),
        1,
        "Should only have base dependency"
    );
    assert_eq!(project.dependencies[0].artifact_id, "base-lib");
}

#[tokio::test]
async fn test_profile_properties_override() {
    let pom_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <properties>
        <my.property>base-value</my.property>
        <other.property>unchanged</other.property>
    </properties>

    <profiles>
        <profile>
            <id>override-profile</id>
            <activation>
                <activeByDefault>true</activeByDefault>
            </activation>
            <properties>
                <my.property>profile-value</my.property>
            </properties>
        </profile>
    </profiles>
</project>
"#;

    let temp_dir = create_test_project(pom_xml);
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();

    // Check base properties
    assert_eq!(project.properties.get("my.property").unwrap(), "base-value");
    assert_eq!(
        project.properties.get("other.property").unwrap(),
        "unchanged"
    );

    // Activate profiles
    let context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    let resolver = MavenResolver::new();
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();

    // Profile property should override base
    assert_eq!(
        project.properties.get("my.property").unwrap(),
        "profile-value"
    );
    assert_eq!(
        project.properties.get("other.property").unwrap(),
        "unchanged"
    );
}

#[tokio::test]
async fn test_jdk_activation_suppresses_default_profile() {
    let pom_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <dependencies>
        <dependency>
            <groupId>com.example</groupId>
            <artifactId>base-lib</artifactId>
            <version>1.0</version>
        </dependency>
    </dependencies>

    <profiles>
        <profile>
            <id>profile-1</id>
            <activation>
                <activeByDefault>true</activeByDefault>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>com.example</groupId>
                    <artifactId>profile-1-lib</artifactId>
                    <version>1.0</version>
                </dependency>
            </dependencies>
        </profile>
        <profile>
            <id>profile-2</id>
            <activation>
                <jdk>17</jdk>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>com.example</groupId>
                    <artifactId>profile-2-lib</artifactId>
                    <version>2.0</version>
                </dependency>
            </dependencies>
        </profile>
    </profiles>
</project>
"#;

    let temp_dir = create_test_project(pom_xml);
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();

    // Activate with JDK 17 context
    let mut context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    context.java_version = "17".to_string();

    let resolver = MavenResolver::new();
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();

    // A matching implicit condition disables default profiles from this POM.
    assert_eq!(project.dependencies.len(), 2, "Should have 2 dependencies");

    let has_base = project
        .dependencies
        .iter()
        .any(|d| d.artifact_id == "base-lib");
    let has_profile_1 = project
        .dependencies
        .iter()
        .any(|d| d.artifact_id == "profile-1-lib");
    let has_profile_2 = project
        .dependencies
        .iter()
        .any(|d| d.artifact_id == "profile-2-lib");

    assert!(has_base, "Should have base dependency");
    assert!(!has_profile_1, "Default profile must be inactive");
    assert!(has_profile_2, "Should have profile-2 dependency");
}

#[tokio::test]
async fn test_jdk_version_profile_activation() {
    let pom_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>jdk11-profile</id>
            <activation>
                <jdk>11</jdk>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>com.example</groupId>
                    <artifactId>jdk11-lib</artifactId>
                    <version>1.0</version>
                </dependency>
            </dependencies>
        </profile>
        <profile>
            <id>jdk17-profile</id>
            <activation>
                <jdk>17</jdk>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>com.example</groupId>
                    <artifactId>jdk17-lib</artifactId>
                    <version>1.0</version>
                </dependency>
            </dependencies>
        </profile>
    </profiles>
</project>
"#;

    let temp_dir = create_test_project(pom_xml);
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();

    // Activate with JDK 17 context
    let mut context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    context.java_version = "17".to_string();

    let resolver = MavenResolver::new();
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();

    // Should only have JDK 17 dependency
    assert_eq!(project.dependencies.len(), 1, "Should have 1 dependency");
    assert_eq!(project.dependencies[0].artifact_id, "jdk17-lib");
}

#[tokio::test]
async fn test_property_based_profile_activation() {
    let pom_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>native-profile</id>
            <activation>
                <property>
                    <name>native</name>
                </property>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>com.example</groupId>
                    <artifactId>native-lib</artifactId>
                    <version>1.0</version>
                </dependency>
            </dependencies>
        </profile>
    </profiles>
</project>
"#;

    let temp_dir = create_test_project(pom_xml);
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();

    // Test without the property - profile should NOT activate
    let context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    let resolver = MavenResolver::new();
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();
    assert_eq!(
        project.dependencies.len(),
        0,
        "Should have no dependencies without property"
    );

    // Test with the property - profile should activate
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();
    let mut context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    context.set_property("native".to_string(), "true".to_string());

    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();
    assert_eq!(
        project.dependencies.len(),
        1,
        "Should have 1 dependency with property"
    );
    assert_eq!(project.dependencies[0].artifact_id, "native-lib");
}

#[tokio::test]
async fn test_no_profiles_empty_project() {
    let pom_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <dependencies>
        <dependency>
            <groupId>com.example</groupId>
            <artifactId>base-lib</artifactId>
            <version>1.0</version>
        </dependency>
    </dependencies>
</project>
"#;

    let temp_dir = create_test_project(pom_xml);
    let mut project = MavenParser::parse_project(temp_dir.path()).await.unwrap();

    let initial_deps = project.dependencies.len();

    // Activate profiles (should be no-op)
    let context = ProfileActivationContext::new(temp_dir.path().to_path_buf());
    let resolver = MavenResolver::new();
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();

    // Dependencies should be unchanged
    assert_eq!(project.dependencies.len(), initial_deps);
}

#[tokio::test]
async fn profiles_without_implicit_activation_do_not_change_the_project() {
    for activation in [
        "",
        "<activation/>",
        "<activation><activeByDefault>false</activeByDefault></activation>",
    ] {
        let fixture = create_test_project(&format!(
            r#"
<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>
<artifactId>service</artifactId><version>1</version>
<properties><environment>development</environment></properties>
<profiles><profile><id>production</id>{activation}
<properties><environment>production</environment></properties>
<dependencies><dependency><groupId>org.example</groupId><artifactId>production-driver</artifactId><version>1</version></dependency></dependencies>
</profile></profiles></project>"#
        ));
        let mut project = MavenParser::parse_project(fixture.path()).await.unwrap();
        let context = ProfileActivationContext::new(fixture.path().into());
        MavenResolver::with_repository(fixture.path().join("repository"))
            .activate_profiles(&mut project, &context)
            .await
            .unwrap();
        assert!(
            project.dependencies.is_empty(),
            "Unexpected production dependency with {activation:?}"
        );
        assert_eq!(project.properties["environment"], "development");
    }
}

#[tokio::test]
async fn active_by_default_is_a_fallback_for_the_profiles_in_one_pom() {
    let fixture = create_test_project(
        r#"
<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>
<artifactId>service</artifactId><version>1</version>
<profiles>
<profile><id>development</id><activation><activeByDefault>true</activeByDefault></activation>
<properties><environment>development</environment></properties>
<dependencies><dependency><groupId>org.example</groupId><artifactId>development-driver</artifactId><version>1</version></dependency></dependencies></profile>
<profile><id>defaults</id><activation><activeByDefault>true</activeByDefault></activation><properties><defaults>yes</defaults></properties></profile>
<profile><id>production</id><activation><property><name>environment</name><value>production</value></property></activation>
<properties><environment>production</environment></properties>
<dependencies><dependency><groupId>org.example</groupId><artifactId>production-driver</artifactId><version>1</version></dependency></dependencies></profile>
</profiles></project>"#,
    );
    let repository = fixture.path().join("repository");
    for artifact in ["development-driver", "production-driver"] {
        let dir = repository.join(format!("org/example/{artifact}/1"));
        std::fs::create_dir_all(&dir).unwrap();
        zip::ZipWriter::new(std::fs::File::create(dir.join(format!("{artifact}-1.jar"))).unwrap())
            .finish()
            .unwrap();
        std::fs::write(dir.join(format!("{artifact}-1.pom")), format!(
            "<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId><artifactId>{artifact}</artifactId><version>1</version></project>"
        )).unwrap();
    }
    let resolver = MavenResolver::with_repository(repository.clone());
    for production in [false, true] {
        let mut project = MavenParser::parse_project(fixture.path()).await.unwrap();
        let mut context = ProfileActivationContext::new(fixture.path().into());
        if production {
            context.set_property("environment".into(), "production".into());
        }
        resolver
            .activate_profiles(&mut project, &context)
            .await
            .unwrap();
        let expected = if production {
            "production"
        } else {
            "development"
        };
        assert_eq!(
            project
                .dependencies
                .iter()
                .map(|d| d.artifact_id.as_str())
                .collect::<Vec<_>>(),
            [format!("{expected}-driver")]
        );
        assert_eq!(project.properties["environment"], expected);
        assert_eq!(project.properties.contains_key("defaults"), !production);
        let report = resolver
            .resolve_dependencies_with_report(&project, usize::MAX)
            .await;
        assert!(report.complete, "{:?}", report.issues);
        assert_eq!(
            report.resolved_jars,
            [repository.join(format!(
                "org/example/{expected}-driver/1/{expected}-driver-1.jar"
            ))]
        );
        assert_eq!(report.main_jars, report.resolved_jars);
    }
}

#[tokio::test]
async fn implicit_profile_activation_requires_every_condition() {
    for java_version in ["17", "21"] {
        let fixture = create_test_project(
            r#"
<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>
<artifactId>service</artifactId><version>1</version><profiles>
<profile><id>native</id><activation><jdk>21</jdk><property><name>native</name></property></activation>
<properties><selected>yes</selected></properties></profile>
</profiles></project>"#,
        );
        let mut project = MavenParser::parse_project(fixture.path()).await.unwrap();
        let mut context = ProfileActivationContext::new(fixture.path().into());
        context.java_version = java_version.into();
        context.set_property("native".into(), "true".into());
        MavenResolver::new()
            .activate_profiles(&mut project, &context)
            .await
            .unwrap();
        assert_eq!(
            project.properties.contains_key("selected"),
            java_version == "21"
        );
    }
}

#[tokio::test]
async fn a_default_profile_can_also_activate_by_its_own_conditions() {
    let fixture = create_test_project(
        r#"
<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>
<artifactId>service</artifactId><version>1</version><profiles>
<profile><id>fallback</id><activation><activeByDefault>true</activeByDefault></activation><properties><fallback>yes</fallback></properties></profile>
<profile><id>conditional-default</id><activation><activeByDefault>true</activeByDefault><property><name>native</name></property></activation><properties><native>yes</native></properties></profile>
</profiles></project>"#,
    );
    for native in [false, true] {
        let mut project = MavenParser::parse_project(fixture.path()).await.unwrap();
        let mut context = ProfileActivationContext::new(fixture.path().into());
        if native {
            context.set_property("native".into(), "true".into());
        }
        MavenResolver::new()
            .activate_profiles(&mut project, &context)
            .await
            .unwrap();
        assert!(project.properties.contains_key("native"));
        assert_eq!(project.properties.contains_key("fallback"), !native);
    }
}

#[tokio::test]
async fn unsupported_or_invalid_activation_does_not_apply_a_profile() {
    for condition in [
        "<os><family>windows</family></os>",
        "<futureActivator/>",
        "<futureActivator>value</futureActivator>",
        "<property><value>value</value></property>",
        "<activeByDefault>${unknown}</activeByDefault>",
    ] {
        let fixture = create_test_project(&format!(
            r#"
<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>
<artifactId>service</artifactId><version>1</version><profiles>
<profile><id>unmodeled</id><activation><activeByDefault>true</activeByDefault>{condition}</activation><properties><selected>yes</selected></properties></profile>
</profiles></project>"#
        ));
        let mut project = MavenParser::parse_project(fixture.path()).await.unwrap();
        let context = ProfileActivationContext::new(fixture.path().into());
        MavenResolver::new()
            .activate_profiles(&mut project, &context)
            .await
            .unwrap();
        assert!(!project.properties.contains_key("selected"), "{condition}");
        assert!(project.unmodeled_features.iter().any(|feature|
            feature.contains("activation") || feature.contains("static boolean")
        ), "{condition}: {:?}", project.unmodeled_features);
    }
}

#[tokio::test]
async fn parent_default_profiles_are_selected_independently_of_child_profiles() {
    let fixture = create_test_project(
        r#"
<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>
<artifactId>parent</artifactId><version>1</version><packaging>pom</packaging>
<build><sourceDirectory>custom-source</sourceDirectory></build><profiles>
<profile><id>defaults</id><activation><activeByDefault>true</activeByDefault></activation><properties><parent.default>yes</parent.default></properties></profile>
</profiles></project>"#,
    );
    let child = fixture.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(child.join("pom.xml"), r#"
<project><modelVersion>4.0.0</modelVersion>
<parent><groupId>org.example</groupId><artifactId>parent</artifactId><version>1</version></parent>
<artifactId>child</artifactId><profiles>
<profile><id>defaults</id><activation><activeByDefault>true</activeByDefault></activation><properties><child.default>yes</child.default></properties></profile>
<profile><id>selected</id><activation><file><exists>pom.xml</exists></file></activation><properties><child.selected>yes</child.selected></properties></profile>
</profiles></project>"#).unwrap();
    let resolver = MavenResolver::with_repository(fixture.path().join("repository"));
    let mut project = MavenParser::parse_project(&child).await.unwrap();
    let context = ProfileActivationContext::new(child);
    resolver
        .activate_profiles(&mut project, &context)
        .await
        .unwrap();
    resolver.resolve_parent_pom(&mut project).await.unwrap();
    assert_eq!(project.properties["parent.default"], "yes");
    assert_eq!(project.properties["child.selected"], "yes");
    assert!(!project.properties.contains_key("child.default"));
    assert!(
        project
            .unmodeled_features
            .iter()
            .any(|feature| feature.contains("custom build source/output paths")),
        "parent completeness blockers must survive inheritance"
    );
    assert!(!resolver.finalize_project_dependencies(&mut project));
}

#[tokio::test]
async fn active_profile_dependencies_replace_base_before_management() {
    // Compared with Maven 3.9.11 help:effective-pom: profiles replace a whole
    // dependency declaration; management supplies omitted attributes afterwards.
    for later_profile in [false, true] {
        let extra_profile = if later_profile {
            r#"<profile><id>exclusions</id><activation><activeByDefault>true</activeByDefault></activation>
<dependencies><dependency><groupId>org.example</groupId><artifactId>driver</artifactId><exclusions><exclusion><groupId>org.example</groupId><artifactId>profile-exclusion</artifactId></exclusion></exclusions></dependency></dependencies></profile>"#
        } else {
            ""
        };
        let fixture = create_test_project(&format!(
            r#"
<project><modelVersion>4.0.0</modelVersion><groupId>org.example</groupId>
<artifactId>service</artifactId><version>1</version>
<dependencyManagement><dependencies><dependency><groupId>org.example</groupId><artifactId>driver</artifactId><version>9</version><scope>runtime</scope><optional>false</optional></dependency></dependencies></dependencyManagement>
<dependencies>
<dependency><groupId>org.example</groupId><artifactId>driver</artifactId><version>1</version><scope>test</scope><optional>true</optional><exclusions><exclusion><groupId>org.example</groupId><artifactId>base-exclusion</artifactId></exclusion></exclusions></dependency>
<dependency><groupId>org.example</groupId><artifactId>driver</artifactId><version>1</version><classifier>tests</classifier></dependency>
</dependencies><profiles>
<profile><id>version</id><activation><activeByDefault>true</activeByDefault></activation>
<dependencies><dependency><groupId>org.example</groupId><artifactId>driver</artifactId><version>2</version></dependency></dependencies></profile>
{extra_profile}
</profiles></project>"#
        ));
        let mut project = MavenParser::parse_project(fixture.path()).await.unwrap();
        let context = ProfileActivationContext::new(fixture.path().into());
        let resolver = MavenResolver::new();
        resolver
            .activate_profiles(&mut project, &context)
            .await
            .unwrap();
        resolver.finalize_project_dependencies(&mut project);
        assert_eq!(
            project.dependencies.len(),
            2,
            "same-variant declarations must merge before mediation"
        );
        let driver = &project.dependencies[0];
        assert_eq!(
            driver.version.as_deref(),
            Some(if later_profile { "9" } else { "2" })
        );
        assert_eq!(
            driver.scope,
            hyperion_build::maven::DependencyScope::Runtime
        );
        assert!(!driver.optional);
        assert!(!driver.is_excluded("org.example", "base-exclusion"));
        assert_eq!(
            driver.is_excluded("org.example", "profile-exclusion"),
            later_profile
        );
        assert_eq!(project.dependencies[1].classifier.as_deref(), Some("tests"));
        assert_eq!(project.dependencies[1].version.as_deref(), Some("1"));
    }
}
