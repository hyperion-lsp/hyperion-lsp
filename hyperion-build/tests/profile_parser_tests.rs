// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maven Profile Parsing Tests
//!
//! Tests that Maven profiles are correctly parsed from pom.xml:
//! - Profile ID extraction
//! - Profile dependencies
//! - Profile properties
//! - Activation conditions (property, OS, JDK, file)

use anyhow::Result;
use hyperion_build::maven::{MavenParser, ProfileActivation};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_pom(dir: &TempDir, content: &str) -> PathBuf {
    let pom_path = dir.path().join("pom.xml");
    fs::write(&pom_path, content).unwrap();
    pom_path
}

#[tokio::test]
async fn test_parse_simple_profile_with_id() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>development</id>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);
    assert_eq!(project.profiles[0].id, "development");
    assert_eq!(project.profiles[0].activation, ProfileActivation::Always);

    Ok(())
}

#[tokio::test]
async fn test_parse_profile_with_dependencies() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>testing</id>
            <dependencies>
                <dependency>
                    <groupId>org.junit.jupiter</groupId>
                    <artifactId>junit-jupiter-api</artifactId>
                    <version>5.10.0</version>
                    <scope>test</scope>
                </dependency>
            </dependencies>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);
    assert_eq!(project.profiles[0].id, "testing");
    assert_eq!(project.profiles[0].dependencies.len(), 1);

    let dep = &project.profiles[0].dependencies[0];
    assert_eq!(dep.group_id, "org.junit.jupiter");
    assert_eq!(dep.artifact_id, "junit-jupiter-api");
    assert_eq!(dep.version, Some("5.10.0".into()));

    Ok(())
}

#[tokio::test]
async fn test_parse_profile_with_properties() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>production</id>
            <properties>
                <env>prod</env>
                <log.level>WARN</log.level>
            </properties>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);
    assert_eq!(project.profiles[0].id, "production");
    assert_eq!(project.profiles[0].properties.len(), 2);
    assert_eq!(
        project.profiles[0].properties.get("env"),
        Some(&"prod".to_string())
    );
    assert_eq!(
        project.profiles[0].properties.get("log.level"),
        Some(&"WARN".to_string())
    );

    Ok(())
}

#[tokio::test]
async fn test_parse_profile_activation_by_property() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>custom-profile</id>
            <activation>
                <property>
                    <name>env</name>
                    <value>dev</value>
                </property>
            </activation>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);
    assert_eq!(project.profiles[0].id, "custom-profile");

    match &project.profiles[0].activation {
        ProfileActivation::ByProperty { name, value } => {
            assert_eq!(name, "env");
            assert_eq!(value, &Some("dev".into()));
        }
        _ => panic!("Expected ByProperty activation"),
    }

    Ok(())
}

#[tokio::test]
async fn test_parse_profile_activation_by_property_existence() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>skip-tests</id>
            <activation>
                <property>
                    <name>skipTests</name>
                </property>
            </activation>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);

    match &project.profiles[0].activation {
        ProfileActivation::ByProperty { name, value } => {
            assert_eq!(name, "skipTests");
            assert_eq!(value, &None);
        }
        _ => panic!("Expected ByProperty activation"),
    }

    Ok(())
}

#[tokio::test]
async fn test_parse_profile_activation_by_os() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>windows-profile</id>
            <activation>
                <os>
                    <name>Windows</name>
                    <arch>x86_64</arch>
                </os>
            </activation>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);

    match &project.profiles[0].activation {
        ProfileActivation::ByOS {
            name,
            arch,
            version,
        } => {
            assert_eq!(name, &Some("Windows".into()));
            assert_eq!(arch, &Some("x86_64".into()));
            assert_eq!(version, &None);
        }
        _ => panic!("Expected ByOS activation"),
    }

    Ok(())
}

#[tokio::test]
async fn test_parse_profile_activation_by_jdk() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>java17</id>
            <activation>
                <jdk>17</jdk>
            </activation>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);

    match &project.profiles[0].activation {
        ProfileActivation::ByJDK(version) => {
            assert_eq!(version, "17");
        }
        _ => panic!("Expected ByJDK activation"),
    }

    Ok(())
}

#[tokio::test]
async fn test_parse_profile_activation_by_file() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>local-dev</id>
            <activation>
                <file>
                    <exists>local.properties</exists>
                </file>
            </activation>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);

    match &project.profiles[0].activation {
        ProfileActivation::ByFile { exists, missing } => {
            assert_eq!(exists, &Some(PathBuf::from("local.properties")));
            assert_eq!(missing, &None);
        }
        _ => panic!("Expected ByFile activation"),
    }

    Ok(())
}

#[tokio::test]
async fn test_parse_multiple_profiles() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>dev</id>
        </profile>
        <profile>
            <id>test</id>
        </profile>
        <profile>
            <id>prod</id>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 3);
    assert_eq!(project.profiles[0].id, "dev");
    assert_eq!(project.profiles[1].id, "test");
    assert_eq!(project.profiles[2].id, "prod");

    Ok(())
}

#[tokio::test]
async fn test_parse_complex_activation() -> Result<()> {
    // Test profile with JDK version range
    let temp_dir = TempDir::new()?;
    let pom_content = r#"
<?xml version="1.0" encoding="UTF-8"?>
<project>
    <modelVersion>4.0.0</modelVersion>
    <groupId>com.example</groupId>
    <artifactId>test-project</artifactId>
    <version>1.0.0</version>

    <profiles>
        <profile>
            <id>java11-plus</id>
            <activation>
                <jdk>[11,)</jdk>
            </activation>
            <dependencies>
                <dependency>
                    <groupId>org.example</groupId>
                    <artifactId>modern-lib</artifactId>
                    <version>2.0.0</version>
                </dependency>
            </dependencies>
        </profile>
    </profiles>
</project>
"#;

    let pom_path = create_test_pom(&temp_dir, pom_content);
    let mut project = hyperion_build::maven::MavenProject::new(
        "test-project".into(),
        temp_dir.path().to_path_buf(),
    );
    MavenParser::parse_pom(&pom_path, &mut project)?;

    assert_eq!(project.profiles.len(), 1);
    match &project.profiles[0].activation {
        ProfileActivation::ByJDK(version) => {
            assert_eq!(version, "[11,)");
        }
        _ => panic!("Expected ByJDK activation"),
    }

    Ok(())
}
