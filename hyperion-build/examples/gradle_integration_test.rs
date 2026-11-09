// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test the Gradle parser with the example project

use hyperion_build::gradle::parser::GradleParser;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let project_path = Path::new("/workspace/examples/gradle-project");

    println!("Parsing Gradle project at: {:?}", project_path);
    println!("{}", "=".repeat(80));

    let project = GradleParser::parse_project(project_path).await?;

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

    println!("\n📁 Source Sets ({}):", project.source_sets.len());
    for source_set in &project.source_sets {
        println!("  - {}:", source_set.name);
        for java_dir in &source_set.java_dirs {
            println!("    Java: {:?}", java_dir);
        }
        for res_dir in &source_set.resource_dirs {
            println!("    Resources: {:?}", res_dir);
        }
        if let Some(ref output) = source_set.output_dir {
            println!("    Output: {:?}", output);
        }
    }

    println!("\n🏢 Subprojects ({}):", project.subprojects.len());
    for subproject in &project.subprojects {
        println!("  - {}", subproject);
    }

    // Now test the resolver
    println!("\n\n🔍 Testing Dependency Resolution:");
    println!("{}", "=".repeat(80));

    use hyperion_build::gradle::resolver::GradleResolver;
    let resolver = GradleResolver::new();

    let (gradle_cache, maven_local) = resolver.cache_dirs();
    println!("Gradle cache: {:?}", gradle_cache);
    println!("Maven local: {:?}", maven_local);

    // Try to resolve a few dependencies
    println!("\nAttempting to resolve some dependencies...");
    let mut resolved_count = 0;
    for dep in project.dependencies.iter().take(5) {
        match resolver.resolve_dependency(dep).await {
            Ok(jar_path) => {
                println!("  ✓ {} -> {:?}", dep.coordinates(), jar_path);
                resolved_count += 1;
            }
            Err(e) => {
                println!("  ✗ {}: {}", dep.coordinates(), e);
            }
        }
    }

    println!(
        "\nResolved {}/{} dependencies from local cache",
        resolved_count,
        project.dependencies.len().min(5)
    );

    // Test classpath building
    println!("\n\n🛠️  Building Classpath:");
    println!("{}", "=".repeat(80));

    use hyperion_build::gradle::classpath::GradleClasspathBuilder;
    let builder = GradleClasspathBuilder::new();
    let classpath = builder.build(&project).await?;

    println!("Project: {}", classpath.project_name);
    println!(
        "Main classpath entries: {}",
        classpath.main_classpath().len()
    );
    println!(
        "Test classpath entries: {}",
        classpath.test_classpath().len()
    );
    println!("Total entry count: {}", classpath.entry_count());

    println!("\nSource directories:");
    for source_dir in &classpath.source_dirs {
        println!("  - {:?}", source_dir);
    }

    println!("\nOutput directories:");
    for output_dir in &classpath.output_dirs {
        println!("  - {:?}", output_dir);
    }

    println!("\nDependency JARs ({}):", classpath.dependency_jars.len());
    for jar in classpath.dependency_jars.iter().take(10) {
        println!("  - {:?}", jar);
    }
    if classpath.dependency_jars.len() > 10 {
        println!("  ... and {} more", classpath.dependency_jars.len() - 10);
    }

    println!("\n✅ Gradle integration test completed successfully!");

    Ok(())
}
