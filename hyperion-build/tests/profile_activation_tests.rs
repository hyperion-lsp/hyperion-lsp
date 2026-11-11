// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Maven Profile Activation Tests
//!
//! Tests that profile activation conditions are correctly evaluated:
//! - Always activation
//! - Property-based activation (existence and value matching)
//! - OS-based activation (name and architecture)
//! - JDK version activation (exact, range, prefix)
//! - File-based activation (exists/missing)
//! - Compound activation (All)

use anyhow::Result;
use hyperion_build::maven::{
    MavenProfile, ProfileActivation, ProfileActivationContext, ProfileEvaluator,
};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_test_context() -> ProfileActivationContext {
    let temp_dir = TempDir::new().unwrap();
    ProfileActivationContext::new(temp_dir.path().to_path_buf())
}

fn create_test_profile(id: &str, activation: ProfileActivation) -> MavenProfile {
    MavenProfile {
        id: id.to_string(),
        activation,
        active_by_default: false,
        dependencies: Vec::new(),
        properties: HashMap::new(),
    }
}

#[test]
fn test_always_activation() -> Result<()> {
    let ctx = create_test_context();
    let profile = create_test_profile("test", ProfileActivation::Always);

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_property_existence_activation_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.set_property("skipTests".to_string(), "true".to_string());

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByProperty {
            name: "skipTests".into(),
            value: None,
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_property_existence_activation_no_match() -> Result<()> {
    let ctx = create_test_context();

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByProperty {
            name: "skipTests".into(),
            value: None,
        },
    );

    // Property doesn't exist, but we're only checking existence
    // Maven's behavior: if property doesn't exist and we're only checking existence, it doesn't activate
    assert!(!ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_property_value_activation_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.set_property("env".to_string(), "dev".to_string());

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByProperty {
            name: "env".into(),
            value: Some("dev".into()),
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_property_value_activation_no_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.set_property("env".to_string(), "prod".to_string());

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByProperty {
            name: "env".into(),
            value: Some("dev".into()),
        },
    );

    assert!(!ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_os_name_activation_linux() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.os_name = "linux".to_string();

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByOS {
            name: Some("Linux".into()),
            arch: None,
            version: None,
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_os_name_activation_windows() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.os_name = "windows".to_string();

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByOS {
            name: Some("Windows".into()),
            arch: None,
            version: None,
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_os_arch_activation() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.os_arch = "x86_64".to_string();

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByOS {
            name: None,
            arch: Some("x86_64".into()),
            version: None,
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_os_arch_activation_amd64_alias() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.os_arch = "x86_64".to_string();

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByOS {
            name: None,
            arch: Some("amd64".into()),
            version: None,
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_os_combined_activation() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.os_name = "linux".to_string();
    ctx.os_arch = "x86_64".to_string();

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByOS {
            name: Some("Linux".into()),
            arch: Some("x86_64".into()),
            version: None,
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_jdk_exact_version_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "17".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("17".to_string()));

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_jdk_major_version_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "17.0.8".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("17".to_string()));

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_jdk_version_no_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "11.0.16".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("17".to_string()));

    assert!(!ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_jdk_version_range_inclusive_min() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "11".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("[11,)".to_string()));

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_jdk_version_range_exclusive_max() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "17".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("[11,17)".to_string()));

    assert!(!ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_jdk_version_range_within() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "15".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("[11,17)".to_string()));

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn incomplete_jdk_version_ranges_do_not_panic_or_activate() {
    let mut ctx = create_test_context();
    ctx.java_version = "17".into();
    for range in ["[", "(", "[11,", "[11,é", "(é", "[11,21x", "[]", "(17]"] {
        let profile = create_test_profile("editing", ProfileActivation::ByJDK(range.into()));
        assert!(!ProfileEvaluator::evaluate(&profile, &ctx), "{range}");
    }
}

#[test]
fn test_jdk_version_range_open_ended() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "21".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("[11,)".to_string()));

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_jdk_legacy_version_format() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.java_version = "1.8.0_292".to_string();

    let profile = create_test_profile("test", ProfileActivation::ByJDK("8".to_string()));

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_file_exists_activation_match() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let test_file = temp_dir.path().join("local.properties");
    fs::write(&test_file, "test content")?;

    let ctx = ProfileActivationContext::new(temp_dir.path().to_path_buf());

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByFile {
            exists: Some(PathBuf::from("local.properties")),
            missing: None,
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_file_exists_activation_no_match() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let ctx = ProfileActivationContext::new(temp_dir.path().to_path_buf());

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByFile {
            exists: Some(PathBuf::from("local.properties")),
            missing: None,
        },
    );

    assert!(!ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_file_missing_activation_match() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let ctx = ProfileActivationContext::new(temp_dir.path().to_path_buf());

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByFile {
            exists: None,
            missing: Some(PathBuf::from("ci.properties")),
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_file_combined_activation() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let exists_file = temp_dir.path().join("local.properties");
    fs::write(&exists_file, "test")?;

    let ctx = ProfileActivationContext::new(temp_dir.path().to_path_buf());

    let profile = create_test_profile(
        "test",
        ProfileActivation::ByFile {
            exists: Some(PathBuf::from("local.properties")),
            missing: Some(PathBuf::from("ci.properties")),
        },
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_all_activation_all_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.set_property("env".to_string(), "dev".to_string());
    ctx.java_version = "17".to_string();

    let profile = create_test_profile(
        "test",
        ProfileActivation::All(vec![
            ProfileActivation::ByProperty {
                name: "env".into(),
                value: Some("dev".into()),
            },
            ProfileActivation::ByJDK("17".to_string()),
        ]),
    );

    assert!(ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

#[test]
fn test_all_activation_partial_match() -> Result<()> {
    let mut ctx = create_test_context();
    ctx.set_property("env".to_string(), "prod".to_string());
    ctx.java_version = "17".to_string();

    let profile = create_test_profile(
        "test",
        ProfileActivation::All(vec![
            ProfileActivation::ByProperty {
                name: "env".into(),
                value: Some("dev".into()),
            },
            ProfileActivation::ByJDK("17".to_string()),
        ]),
    );

    assert!(!ProfileEvaluator::evaluate(&profile, &ctx));

    Ok(())
}

fn os_version_profile(version: &str) -> MavenProfile {
    create_test_profile(
        "test",
        ProfileActivation::ByOS {
            name: None,
            arch: None,
            version: Some(version.into()),
        },
    )
}

#[test]
fn test_os_version_activation_matches_literally() {
    let mut ctx = create_test_context();
    ctx.os_version = "6.8.0-45-generic".to_string();

    assert!(ProfileEvaluator::evaluate(
        &os_version_profile("6.8.0-45-generic"),
        &ctx
    ));
    assert!(!ProfileEvaluator::evaluate(
        &os_version_profile("6.8.0"),
        &ctx
    ));
    // Maven compares the value literally; it is not a regular expression.
    assert!(!ProfileEvaluator::evaluate(
        &os_version_profile("6\\.8\\..*"),
        &ctx
    ));
}

#[test]
fn test_os_version_negation_inverts_the_comparison() {
    let mut ctx = create_test_context();
    ctx.os_version = "6.8.0-45-generic".to_string();

    assert!(!ProfileEvaluator::evaluate(
        &os_version_profile("!6.8.0-45-generic"),
        &ctx
    ));
    assert!(ProfileEvaluator::evaluate(
        &os_version_profile("!6.1.0"),
        &ctx
    ));
}

#[test]
fn test_os_version_unknown_never_matches_positively() {
    let mut unknown = create_test_context();
    unknown.os_version = String::new();
    assert!(!ProfileEvaluator::evaluate(
        &os_version_profile("6.8.0"),
        &unknown
    ));
    assert!(ProfileEvaluator::evaluate(
        &os_version_profile("!6.8.0"),
        &unknown
    ));
}
