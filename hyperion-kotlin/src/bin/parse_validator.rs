// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_kotlin::parse;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: parse_validator <file>");
        std::process::exit(1);
    }

    let file_path = &args[1];

    match fs::read_to_string(file_path) {
        Ok(source) => {
            let parse_tree = parse(&source);

            // Check if there are any parse errors
            if parse_tree.errors.is_empty() {
                // Success - no parse errors
                let file_name = Path::new(file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(file_path);
                println!("✓ {file_name}");
                std::process::exit(0);
            } else {
                // Parse errors found
                eprintln!(
                    "✗ {} - {} parse error(s):",
                    Path::new(file_path).display(),
                    parse_tree.errors.len()
                );

                for (i, error) in parse_tree.errors.iter().enumerate().take(3) {
                    eprintln!("  Error {}: {} at {:?}", i + 1, error.message, error.range);
                }

                if parse_tree.errors.len() > 3 {
                    eprintln!("  ... and {} more errors", parse_tree.errors.len() - 3);
                }

                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("✗ {} - File error: {}", Path::new(file_path).display(), e);
            std::process::exit(1);
        }
    }
}
