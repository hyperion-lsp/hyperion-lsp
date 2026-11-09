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
async fn test_multiple_profiles_activated() {
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

    // Should have all dependencies (base + both profiles)
    assert_eq!(project.dependencies.len(), 3, "Should have 3 dependencies");

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
    assert!(has_profile_1, "Should have profile-1 dependency");
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
