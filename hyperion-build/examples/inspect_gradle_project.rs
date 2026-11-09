// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test the Gradle parser with a real project

use hyperion_build::gradle::parser::GradleParser;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let project_path = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    println!("Parsing Gradle project at: {:?}", project_path);
    println!("{}", "=".repeat(80));

    let project = GradleParser::parse_project(&project_path).await?;

    println!("\n📦 Project Information:");
    println!("  Name: {}", project.name);
    println!("  Group: {:?}", project.group);
    println!("  Version: {:?}", project.version);
    println!("  Root Dir: {:?}", project.root_dir);
    println!("  Has Wrapper: {}", project.has_wrapper);

    println!("\n🔌 Plugins ({}):", project.plugins.len());
    for plugin in &project.plugins {
        println!("  - {} {:?}", plugin.id, plugin.version);
    }

    println!("\n📚 Repositories ({}):", project.repositories.len());
    for repo in &project.repositories {
        println!("  - {}: {}", repo.name, repo.url);
    }

    println!("\n📦 Dependencies ({}):", project.dependencies.len());
    for dep in &project.dependencies {
        println!("  - {:?}: {}", dep.configuration, dep.coordinates());
    }

    println!("\n🏢 Subprojects ({}):", project.subprojects.len());
    for subproject in &project.subprojects {
        println!("  - {}", subproject);
    }

    println!("\n📁 Subproject Dirs ({}):", project.subproject_dirs.len());
    for (name, dir) in &project.subproject_dirs {
        println!("  - {}: {:?}", name, dir);
    }

    println!("\n✅ Gradle parsing completed!");

    Ok(())
}
