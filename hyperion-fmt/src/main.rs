// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! hyperion-fmt CLI - Java formatter implementing palantir-java-format style

use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use walkdir::WalkDir;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        eprintln!("Usage: hyperion-fmt [OPTIONS] <PATH>");
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  <PATH>    Java file or directory to format (use - for stdin)");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  -w, --write        Write result to file(s) instead of stdout");
        eprintln!("      --style=NAME   Style preset: palantir (default), google");
        eprintln!("      --batch        Read 'src<TAB>dst' lines from stdin, format src into dst.");
        eprintln!(
            "                     Prints one status line per file: OK <src> | ERR <src> <msg>"
        );
        eprintln!("  -h, --help         Print help");
        eprintln!();
        eprintln!("When PATH is a directory, all .java files are formatted recursively.");
        return ExitCode::SUCCESS;
    }

    let mut write_in_place = false;
    let mut batch = false;
    let mut file_path: Option<PathBuf> = None;
    let mut config = hyperion_fmt::FormatConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-w" | "--write" => write_in_place = true,
            "--batch" => batch = true,
            "-f" | "--file" => {
                // Support -f flag for file (backwards compat)
                i += 1;
                if i < args.len() {
                    file_path = Some(PathBuf::from(&args[i]));
                }
            }
            arg if arg.starts_with("--style=") => {
                config = match &arg["--style=".len()..] {
                    "palantir" => hyperion_fmt::FormatConfig::palantir(),
                    "google" => hyperion_fmt::FormatConfig::google(),
                    other => {
                        eprintln!("Unknown style: {other} (expected palantir or google)");
                        return ExitCode::FAILURE;
                    }
                };
            }
            arg if arg == "-" || !arg.starts_with('-') => {
                file_path = Some(PathBuf::from(arg));
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    if batch {
        return format_batch(&config);
    }

    let Some(path) = file_path else {
        eprintln!("Error: No file specified");
        return ExitCode::FAILURE;
    };

    if path.to_str() == Some("-") {
        return format_stdin(&config);
    }

    if path.is_dir() {
        return format_directory(&path, write_in_place, &config);
    }

    format_file(&path, write_in_place, &config)
}

/// Batch mode: read `src<TAB>dst` lines from stdin, format each src into dst.
///
/// One status line per file on stdout (`OK <src>` or `ERR <src> <msg>`), so a
/// driver can score a whole corpus from a single process invocation.
fn format_batch(config: &hyperion_fmt::FormatConfig) -> ExitCode {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            eprintln!("Error: Failed to read from stdin");
            return ExitCode::FAILURE;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Some((src, dst)) = line.split_once('\t') else {
            println!("ERR {line} missing-tab-separator");
            continue;
        };
        let result = fs::read_to_string(src)
            .map_err(|e| format!("read: {e}"))
            .and_then(|source| {
                hyperion_fmt::format_source(&source, config).map_err(|e| format!("format: {e}"))
            })
            .and_then(|formatted| {
                if let Some(parent) = Path::new(dst).parent() {
                    fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
                }
                fs::write(dst, formatted).map_err(|e| format!("write: {e}"))
            });
        match result {
            Ok(()) => println!("OK {src}"),
            Err(msg) => println!("ERR {src} {}", msg.replace('\n', " ")),
        }
    }
    ExitCode::SUCCESS
}

fn format_stdin(config: &hyperion_fmt::FormatConfig) -> ExitCode {
    let mut buf = String::new();
    if io::stdin().read_to_string(&mut buf).is_err() {
        eprintln!("Error: Failed to read from stdin");
        return ExitCode::FAILURE;
    }

    match hyperion_fmt::format_source(&buf, config) {
        Ok(formatted) => {
            print!("{formatted}");
            let _ = io::stdout().flush();
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: Failed to format: {e}");
            ExitCode::FAILURE
        }
    }
}

fn format_file(path: &Path, write_in_place: bool, config: &hyperion_fmt::FormatConfig) -> ExitCode {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Failed to read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    let formatted = match hyperion_fmt::format_source(&source, config) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: Failed to format {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    if write_in_place {
        if formatted != source {
            if let Err(e) = fs::write(path, &formatted) {
                eprintln!("Error: Failed to write {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
            eprintln!("Formatted {}", path.display());
        }
    } else {
        print!("{formatted}");
        let _ = io::stdout().flush();
    }

    ExitCode::SUCCESS
}

fn format_directory(
    dir: &Path,
    write_in_place: bool,
    config: &hyperion_fmt::FormatConfig,
) -> ExitCode {
    if !write_in_place {
        eprintln!("Error: Formatting a directory requires -w/--write flag");
        return ExitCode::FAILURE;
    }
    let mut formatted_count = 0u64;
    let mut error_count = 0u64;

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !is_hidden_or_build_dir(e))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Warning: {e}");
                error_count += 1;
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("java") {
            continue;
        }

        let source = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: Failed to read {}: {e}", path.display());
                error_count += 1;
                continue;
            }
        };

        let formatted = match hyperion_fmt::format_source(&source, config) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error: Failed to format {}: {e}", path.display());
                error_count += 1;
                continue;
            }
        };

        if formatted != source {
            if let Err(e) = fs::write(path, &formatted) {
                eprintln!("Error: Failed to write {}: {e}", path.display());
                error_count += 1;
                continue;
            }
            formatted_count += 1;
            eprintln!("Formatted {}", path.display());
        }
    }

    eprintln!("{formatted_count} file(s) formatted");
    if error_count > 0 {
        eprintln!("{error_count} error(s)");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn is_hidden_or_build_dir(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_str().unwrap_or("");
    if entry.depth() == 0 {
        return false;
    }
    name.starts_with('.')
        || name == "target"
        || name == "build"
        || name == "node_modules"
        || name == ".gradle"
}
