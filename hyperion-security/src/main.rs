// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hyperion Security Scanner CLI

use clap::Parser;
use hyperion_security::{
    diff_findings, group_findings, CodeHighlighter, Finding, FindingGroup, FrameworkRegistry,
    ScanConfig, SecurityScanner, Severity,
};
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "hyperion-security")]
#[command(about = "Type-aware security scanner for JVM languages")]
#[command(version)]
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// Directory or file to scan
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Minimum severity to report (info, low, medium, high, critical)
    #[arg(short, long, default_value = "low")]
    severity: String,

    /// Output format (text, json, sarif)
    #[arg(short, long, default_value = "text")]
    format: String,

    /// Output to file instead of stdout
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Enable incremental scanning with cache
    #[arg(long)]
    incremental: bool,

    /// Path to cache file (default: .hyperion-cache.json)
    #[arg(long, default_value = ".hyperion-cache.json")]
    cache_file: PathBuf,

    /// Maximum taint analysis depth
    #[arg(long, default_value = "50")]
    max_depth: usize,

    /// File scan timeout in seconds (0 = no timeout)
    #[arg(long, default_value = "10")]
    timeout: u64,

    /// Maximum number of parallel workers (0 = use all CPUs)
    #[arg(short = 'j', long, default_value = "0")]
    jobs: usize,

    /// Show progress bar
    #[arg(long)]
    progress: bool,

    /// Print statistics after scan
    #[arg(long)]
    stats: bool,

    /// Baseline file to diff against (JSON array of findings)
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Write findings to a baseline file (JSON array of findings)
    #[arg(long)]
    write_baseline: Option<PathBuf>,

    /// Manually specify frameworks to use (comma-separated: spring,servlet,jaxrs)
    #[arg(long, value_delimiter = ',')]
    frameworks: Option<Vec<String>>,

    /// Show detected frameworks and confidence scores, then exit
    #[arg(long)]
    detect_frameworks: bool,

    /// List all available frameworks and exit
    #[arg(long)]
    list_frameworks: bool,

    /// Exclude test directories and test files from scanning
    #[arg(long)]
    exclude_tests: bool,

    /// Exclude specific paths (glob patterns, comma-separated)
    /// Example: --exclude-path "**/test/**,**/*Test.java"
    #[arg(long, value_delimiter = ',')]
    exclude_path: Option<Vec<String>>,

    /// Disable grouping of similar findings (show individual findings instead)
    #[arg(long)]
    no_group_findings: bool,
}

fn main() {
    let cli = Cli::parse();

    // Handle --list-frameworks (list all available frameworks)
    if cli.list_frameworks {
        print_available_frameworks();
        return;
    }

    // Handle --detect-frameworks (detect and show frameworks, then exit)
    if cli.detect_frameworks {
        detect_and_print_frameworks(&cli.path, cli.verbose);
        return;
    }

    let min_severity = match cli.severity.to_lowercase().as_str() {
        "info" => Severity::Info,
        "low" => Severity::Low,
        "medium" | "med" => Severity::Medium,
        "high" => Severity::High,
        "critical" | "crit" => Severity::Critical,
        _ => {
            eprintln!("Invalid severity: {}. Using 'low'", cli.severity);
            Severity::Low
        }
    };

    let mut config = ScanConfig {
        min_severity,
        output_file: cli.output.clone(),
        max_taint_depth: cli.max_depth,
        file_timeout: if cli.timeout > 0 {
            Some(Duration::from_secs(cli.timeout))
        } else {
            None
        },
        max_workers: cli.jobs,
        // Start with default exclude patterns (target, build, node_modules)
        // but remove test directories - those will be added back if --exclude-tests is set
        exclude_patterns: vec![
            "**/target/**".to_string(),
            "**/build/**".to_string(),
            "**/node_modules/**".to_string(),
        ],
        exclude_tests: false,
        ..ScanConfig::default()
    };

    // Configure incremental scanning
    if cli.incremental {
        config = config.with_incremental(cli.cache_file.clone());
        if cli.verbose {
            eprintln!(
                "Incremental mode enabled, cache: {}",
                cli.cache_file.display()
            );
        }
    }

    // Configure test exclusion
    if cli.exclude_tests {
        config = config.with_exclude_tests(true);
        if cli.verbose {
            eprintln!("Excluding test directories and test files from scan");
        }
    }

    // Configure custom path exclusions
    if let Some(ref patterns) = cli.exclude_path {
        config = config.with_exclude_patterns(patterns.clone());
        if cli.verbose {
            eprintln!("Excluding paths matching: {}", patterns.join(", "));
        }
    }

    // Add progress callback if requested
    if cli.progress {
        let verbose = cli.verbose;
        let progress_printed = Arc::new(AtomicBool::new(false));
        config = config.with_progress_callback(move |current, total, path| {
            // Clear previous line if we've printed before
            if progress_printed.load(Ordering::Relaxed) {
                eprint!("\r\x1b[K");
            }
            progress_printed.store(true, Ordering::Relaxed);

            if verbose {
                eprintln!("Scanning [{current}/{total}]: {path}");
            } else {
                eprint!("Scanning [{current}/{total}]...");
            }
        });
    }

    // Setup framework registry
    let mut framework_registry = FrameworkRegistry::with_builtin();

    // Handle framework selection
    if let Some(ref framework_ids) = cli.frameworks {
        // Manually specified frameworks
        let framework_ids_str: Vec<&str> = framework_ids
            .iter()
            .map(std::string::String::as_str)
            .collect();
        framework_registry.enable_frameworks(&framework_ids_str);

        if cli.verbose {
            eprintln!("Using frameworks: {}", framework_ids.join(", "));
        }
    } else {
        // Auto-detect frameworks
        let scan_root = if cli.path.is_dir() {
            cli.path.clone()
        } else {
            cli.path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf()
        };

        let detection_results = framework_registry.detect_frameworks(&scan_root);

        if cli.verbose {
            eprintln!("\nFramework Detection:");
            for result in &detection_results {
                let status = if result.confidence.exceeds_threshold() {
                    "ENABLED"
                } else {
                    "-"
                };
                eprintln!(
                    "  {:20} [{:.2}] {}",
                    result.framework_name,
                    result.confidence.value(),
                    status
                );
            }
            eprintln!();
        }
    }

    // Build registries from framework registry
    let (source_registry, sink_registry, sanitizer_registry) = framework_registry.into_registries();

    let scanner = SecurityScanner::with_frameworks(
        config,
        source_registry,
        sink_registry,
        sanitizer_registry,
    );

    if cli.verbose {
        eprintln!("Scanning: {}", cli.path.display());
    }

    let (mut findings, scan_stats) = if cli.path.is_dir() {
        scanner.scan_directory_with_stats(&cli.path)
    } else {
        // Single file scan doesn't have stats yet, use wrapper
        let findings = scanner.scan_file(&cli.path);
        let stats = hyperion_security::ScanStats {
            files_scanned: 1,
            files_cached: 0,
            parse_errors: 0,
            timeouts: 0,
            total_findings: findings.len(),
            findings_by_rule: {
                let mut map = std::collections::BTreeMap::new();
                for finding in &findings {
                    let key = finding
                        .rule_id
                        .clone()
                        .unwrap_or_else(|| finding.vuln_type.as_str().to_string());
                    *map.entry(key).or_insert(0) += 1;
                }
                map
            },
        };
        (findings, stats)
    };

    if let Some(path) = cli.write_baseline.as_ref() {
        if let Ok(file) = File::create(path) {
            if let Err(err) = serde_json::to_writer_pretty(file, &findings) {
                eprintln!("Failed to write baseline {}: {}", path.display(), err);
            }
        } else {
            eprintln!("Failed to create baseline file: {}", path.display());
        }
    }

    if let Some(path) = cli.baseline.as_ref() {
        let baseline_findings: Vec<Finding> = match File::open(path) {
            Ok(file) => match serde_json::from_reader(file) {
                Ok(findings) => findings,
                Err(err) => {
                    eprintln!("Failed to parse baseline {}: {}", path.display(), err);
                    Vec::new()
                }
            },
            Err(err) => {
                eprintln!("Failed to read baseline {}: {}", path.display(), err);
                Vec::new()
            }
        };
        findings = diff_findings(&findings, &baseline_findings);
    }

    // Clear progress line if shown
    if cli.progress {
        eprintln!();
    }

    if cli.verbose || cli.stats {
        eprintln!("\nScan Statistics:");
        eprintln!("  Files scanned: {}", scan_stats.files_scanned);
        if scan_stats.files_cached > 0 {
            eprintln!("  Files cached: {}", scan_stats.files_cached);
        }
        if scan_stats.parse_errors > 0 {
            eprintln!("  Parse errors: {}", scan_stats.parse_errors);
        }
        if scan_stats.timeouts > 0 {
            eprintln!("  Timeouts: {}", scan_stats.timeouts);
        }
        eprintln!("  Total findings: {}", scan_stats.total_findings);
        if !scan_stats.findings_by_rule.is_empty() {
            eprintln!("  Findings by rule:");
            for (rule, count) in &scan_stats.findings_by_rule {
                eprintln!("    - {rule}: {count}");
            }
        }
        eprintln!();
    }

    // Create syntax highlighter (auto-detects TTY)
    let highlighter = CodeHighlighter::new();

    match cli.format.to_lowercase().as_str() {
        "json" => print_json(&findings),
        "sarif" => print_sarif(&findings),
        _ => {
            if cli.no_group_findings {
                print_text(&findings, &highlighter);
            } else {
                print_text_grouped(&findings, &highlighter);
            }
        }
    }

    // Exit with error code if findings
    if !findings.is_empty() {
        std::process::exit(1);
    }
}

fn print_text(findings: &[Finding], highlighter: &CodeHighlighter) {
    if findings.is_empty() {
        println!("No security issues found.");
        return;
    }

    println!("\n{} security issue(s) found:\n", findings.len());
    println!("{}", "=".repeat(80));

    for (i, finding) in findings.iter().enumerate() {
        println!("\n[{}] {} - {}", i + 1, finding.severity, finding.vuln_type);
        println!("    {}", finding.message);
        println!("    Location: {}", finding.source_location);
        println!("    CWE: {}", finding.vuln_type.cwe_id());
        if let Some(ref rule_id) = finding.rule_id {
            println!("    Rule: {rule_id}");
        }
        if let Some(ref source_kind) = finding.source_kind {
            println!("    Source: {source_kind}");
        }
        if let Some(ref sink_kind) = finding.sink_kind {
            println!("    Sink: {sink_kind}");
        }
        if !finding.sanitizers.is_empty() {
            println!("    Sanitizers: {}", finding.sanitizers.join(", "));
        }
        println!("    Confidence: {}%", (finding.confidence * 100.0).round());
        println!();
        println!("    Code:");

        // Apply syntax highlighting
        let highlighted_code =
            highlighter.highlight(&finding.code_snippet, &finding.source_location.file);
        for line in highlighted_code.lines() {
            println!("      {line}");
        }

        println!();
        println!("    Remediation: {}", finding.remediation);
        println!("{}", "-".repeat(80));
    }
}

fn print_text_grouped(findings: &[Finding], highlighter: &CodeHighlighter) {
    if findings.is_empty() {
        println!("No security issues found.");
        return;
    }

    let groups = group_findings(findings);
    let total_findings = findings.len();
    let total_groups = groups.len();

    println!("\n{total_findings} security issue(s) found ({total_groups} unique patterns):\n");
    println!("{}", "=".repeat(80));

    for (i, group) in groups.iter().enumerate() {
        print_finding_group(i + 1, group, highlighter);
    }
}

fn print_finding_group(index: usize, group: &FindingGroup, highlighter: &CodeHighlighter) {
    println!(
        "\n[{}] {} - {} ({} occurrence{})",
        index,
        group.severity,
        group.vuln_type,
        group.count,
        if group.count == 1 { "" } else { "s" }
    );

    // Print the message from the example finding
    println!("    {}", group.example_finding.message);
    println!("    CWE: {}", group.vuln_type.cwe_id());

    // Show locations (up to 10, then summarize)
    println!("\n    Locations:");
    let show_count = 10.min(group.count);
    for loc in &group.locations[..show_count] {
        println!("      - {}:{}", loc.file.display(), loc.line);
    }
    if group.count > show_count {
        println!("      ... ({} more)", group.count - show_count);
    }

    // Show example code with syntax highlighting
    println!("\n    Example Code:");
    let highlighted_code = highlighter.highlight(
        &group.example_finding.code_snippet,
        &group.example_finding.source_location.file,
    );
    for line in highlighted_code.lines() {
        println!("      {line}");
    }

    // Show remediation
    println!("\n    Remediation:");
    for line in group.example_finding.remediation.lines() {
        println!("      {line}");
    }

    println!("{}", "-".repeat(80));
}

fn print_json(findings: &[Finding]) {
    match serde_json::to_string_pretty(findings) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("Error serializing to JSON: {e}"),
    }
}

fn print_sarif(findings: &[Finding]) {
    // SARIF 2.1.0 format for integration with code scanning tools
    let sarif = serde_json::json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "hyperion-security",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/hyperion-lsp/hyperion-lsp",
                    "rules": get_sarif_rules(findings)
                }
            },
            "results": findings.iter().map(|f| {
                serde_json::json!({
                    "ruleId": f.rule_id.as_deref().unwrap_or_else(|| f.vuln_type.cwe_id()),
                    "level": match f.severity {
                        Severity::Critical | Severity::High => "error",
                        Severity::Medium => "warning",
                        _ => "note"
                    },
                    "message": {
                        "text": f.message
                    },
                    "properties": {
                        "confidence": f.confidence,
                        "source_kind": f.source_kind,
                        "sink_kind": f.sink_kind,
                        "sanitizers": f.sanitizers
                    },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": {
                                "uri": f.source_location.file.display().to_string()
                            },
                            "region": {
                                "startLine": f.source_location.line,
                                "startColumn": f.source_location.column,
                                "endLine": f.source_location.end_line,
                                "endColumn": f.source_location.end_column
                            }
                        }
                    }]
                })
            }).collect::<Vec<_>>()
        }]
    });

    match serde_json::to_string_pretty(&sarif) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("Error serializing to SARIF: {e}"),
    }
}

fn get_sarif_rules(findings: &[Finding]) -> Vec<serde_json::Value> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    findings
        .iter()
        .filter(|f| seen.insert(f.vuln_type))
        .map(|f| {
            serde_json::json!({
                "id": f.vuln_type.cwe_id(),
                "name": format!("{:?}", f.vuln_type),
                "shortDescription": {
                    "text": f.vuln_type.description()
                },
                "helpUri": format!("https://cwe.mitre.org/data/definitions/{}.html",
                    f.vuln_type.cwe_id().trim_start_matches("CWE-")),
                "defaultConfiguration": {
                    "level": match f.vuln_type.default_severity() {
                        Severity::Critical | Severity::High => "error",
                        Severity::Medium => "warning",
                        _ => "note"
                    }
                }
            })
        })
        .collect()
}

fn print_available_frameworks() {
    let registry = FrameworkRegistry::with_builtin();
    let mut frameworks: Vec<_> = registry.all_frameworks();
    frameworks.sort_by_key(|f| f.id());

    println!("Available frameworks:");
    for framework in frameworks {
        println!("  {:12} {}", framework.id(), framework.name());
    }
}

fn detect_and_print_frameworks(path: &std::path::Path, verbose: bool) {
    let scan_root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .to_path_buf()
    };

    let mut registry = FrameworkRegistry::with_builtin();
    let mut results = registry.detect_frameworks(&scan_root);

    // Sort by confidence (highest first)
    results.sort_by(|a, b| {
        b.confidence
            .value()
            .partial_cmp(&a.confidence.value())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("Detected frameworks in {}:", scan_root.display());
    for result in &results {
        let status = if result.confidence.exceeds_threshold() {
            "ENABLED"
        } else {
            "-"
        };
        println!(
            "  {:20} [{:.2}] {}",
            result.framework_name,
            result.confidence.value(),
            status
        );

        if verbose && !result.evidence.dependencies.is_empty() {
            println!(
                "    Dependencies: {}",
                result.evidence.dependencies.join(", ")
            );
        }
        if verbose && !result.evidence.imports.is_empty() {
            let imports: Vec<_> = result.evidence.imports.iter().take(3).collect();
            println!(
                "    Imports: {}{}",
                imports
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if result.evidence.imports.len() > 3 {
                    ", ..."
                } else {
                    ""
                }
            );
        }
        if verbose && !result.evidence.annotations.is_empty() {
            let annotations: Vec<_> = result.evidence.annotations.iter().take(3).collect();
            println!(
                "    Annotations: {}{}",
                annotations
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if result.evidence.annotations.len() > 3 {
                    ", ..."
                } else {
                    ""
                }
            );
        }
    }
}
